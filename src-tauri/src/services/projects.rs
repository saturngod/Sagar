//! Project queries for the library and workspace, plus cover thumbnails
//! served from the app-owned immutable snapshot.

use std::path::{Path, PathBuf};

use base64::Engine as _;

use crate::domain::{ChapterSummary, CoverImage, ProjectSummary};
use crate::epub;
use crate::error::AppError;
use crate::storage::{self, projects};

/// Maximum cover bytes turned into a thumbnail payload.
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

pub fn list_projects(data_dir: &Path) -> Result<Vec<ProjectSummary>, AppError> {
    let conn = open_project_database(data_dir)?;
    projects::list_projects(&conn)
}

pub fn get_project(data_dir: &Path, project_id: &str) -> Result<ProjectSummary, AppError> {
    let conn = open_project_database(data_dir)?;
    projects::get_project(&conn, project_id)
}

pub fn list_chapters(data_dir: &Path, project_id: &str) -> Result<Vec<ChapterSummary>, AppError> {
    let conn = open_project_database(data_dir)?;
    if !projects::project_exists(&conn, project_id)? {
        return Err(AppError::new(
            "project_not_found",
            "This project no longer exists in local storage.",
        ));
    }
    let mut chapters = projects::list_chapters(&conn, project_id)?;
    // Navigation is immutable source metadata, so existing projects need no migration.
    // Books already imported with missing/unreadable navigation keep the flat list.
    let navigation = read_navigation(&conn, data_dir, project_id);
    if let Ok(entries) = navigation {
        let mut by_resource = std::collections::HashMap::new();
        for entry in &entries {
            by_resource.entry(entry.resource.as_str()).or_insert(entry);
        }
        for chapter in &mut chapters {
            if let Some(entry) = by_resource.get(chapter.resource_href.as_str()) {
                chapter.navigation_groups = entry.groups.clone();
            }
        }
    }
    Ok(chapters)
}

/// The table of contents (EPUB 3 nav, else EPUB 2 NCX) from the project's
/// immutable snapshot, in reading order. A book without one returns no entries.
pub fn navigation(data_dir: &Path, project_id: &str) -> Result<Vec<epub::nav::NavEntry>, AppError> {
    let conn = open_project_database(data_dir)?;
    read_navigation(&conn, data_dir, project_id)
}

fn read_navigation(
    conn: &rusqlite::Connection,
    data_dir: &Path,
    project_id: &str,
) -> Result<Vec<epub::nav::NavEntry>, AppError> {
    let relative: String = conn.query_row(
        "SELECT source_snapshot_path FROM projects WHERE id = ?1",
        [project_id],
        |row| row.get(0),
    )?;
    let mut archive = epub::archive::SafeArchive::open(
        &snapshot_abs_path(data_dir, &relative)?,
        &crate::security::ArchiveLimits::default(),
    )?;
    let opf_path = epub::container::find_opf_path(&mut archive)?;
    let opf = epub::opf::parse_opf(&archive.read_entry(&opf_path)?, &opf_path)?;
    match opf.nav_href {
        Some(path) => epub::nav::parse_nav(&archive.read_entry(&path)?, &path),
        None => match opf.ncx_href {
            Some(path) => epub::nav::parse_ncx(&archive.read_entry(&path)?, &path),
            None => Ok(Vec::new()),
        },
    }
}

/// Reads the declared cover image from the project's immutable snapshot.
/// The cover reference is re-derived from the snapshot's own OPF, so the
/// schema never duplicates book data.
/// Removes a project from the library: database rows first, then the
/// app-owned snapshot directory. The user's original EPUB file is never
/// touched.
pub fn delete_project(data_dir: &Path, project_id: &str) -> Result<(), AppError> {
    if project_id.is_empty()
        || project_id.contains('/')
        || project_id.contains('\\')
        || project_id == "."
        || project_id == ".."
    {
        return Err(AppError::new(
            "invalid_project_id",
            "This project cannot be removed safely.",
        ));
    }
    let conn = open_project_database(data_dir)?;
    if !projects::project_exists(&conn, project_id)? {
        return Err(AppError::new(
            "project_not_found",
            "This project no longer exists in local storage.",
        ));
    }
    projects::delete_project_rows(&conn, project_id)?;

    let snapshot_dir = data_dir.join("projects").join(project_id);
    if snapshot_dir.exists() && std::fs::remove_dir_all(&snapshot_dir).is_err() {
        return Err(AppError::new(
            "cleanup_failed",
            "The project was removed, but its stored book files could not \
             be deleted from app storage.",
        ));
    }
    Ok(())
}

pub fn get_cover(data_dir: &Path, project_id: &str) -> Result<Option<CoverImage>, AppError> {
    let conn = open_project_database(data_dir)?;
    let custom: Option<String> = conn.query_row(
        "SELECT custom_cover FROM projects WHERE id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    if let Some(cover) = custom {
        let (mime, bytes) = epub::rich_text::image_bytes(&cover)?;
        return Ok(Some(CoverImage {
            media_type: mime.into(),
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }));
    }
    let snapshot_path: String = conn.query_row(
        "SELECT source_snapshot_path FROM projects WHERE id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;
    let snapshot_abs = snapshot_abs_path(data_dir, &snapshot_path)?;
    let mut archive = epub::archive::SafeArchive::open(
        &snapshot_abs,
        &crate::security::ArchiveLimits::default(),
    )?;

    let opf_path = epub::container::find_opf_path(&mut archive)?;
    let opf_bytes = archive.read_entry(&opf_path)?;
    let parsed_opf = epub::opf::parse_opf(&opf_bytes, &opf_path)?;
    let Some(cover_href) = parsed_opf.cover_href.filter(|href| archive.contains(href)) else {
        return Ok(None);
    };

    let bytes = archive.read_entry(&cover_href)?;
    if bytes.len() as u64 > MAX_COVER_BYTES {
        return Ok(None);
    }
    let media_type = parsed_opf
        .manifest
        .values()
        .find(|item| item.href == cover_href)
        .map(|item| item.media_type.clone())
        .unwrap_or_else(|| match cover_href.rsplit('.').next().unwrap_or("") {
            "png" => "image/png".to_string(),
            "gif" => "image/gif".to_string(),
            "svg" => "image/svg+xml".to_string(),
            _ => "image/jpeg".to_string(),
        });
    Ok(Some(CoverImage {
        media_type,
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

fn open_project_database(data_dir: &Path) -> Result<rusqlite::Connection, AppError> {
    // Always route through initialize so a fresh install (or a database from
    // an older schema) is migrated before the first query runs.
    storage::initialize(data_dir)?;
    storage::open_database(&data_dir.join("sagar.db"))
}

fn snapshot_abs_path(data_dir: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let path = data_dir.join(relative);
    // The relative path comes from our own database, but never trust stored
    // strings as filesystem paths without re-validating.
    let mut resolved = data_dir.to_path_buf();
    for segment in relative.split('/') {
        crate::security::is_safe_entry_name(segment)
            .map_err(|_| AppError::new("invalid_source", "The stored snapshot path is invalid."))?;
        resolved.push(segment);
    }
    if resolved != path {
        return Err(AppError::new(
            "invalid_source",
            "The stored snapshot path is invalid.",
        ));
    }
    Ok(resolved)
}
