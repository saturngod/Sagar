//! Per-book stylesheet editing. The source snapshot stays immutable: edited
//! CSS is stored separately in SQLite and substituted only when a translated
//! EPUB is exported.

use std::collections::HashMap;
use std::path::Path;

use crate::epub::archive::SafeArchive;
use crate::epub::{container, opf};
use crate::error::AppError;
use crate::security::ArchiveLimits;
use crate::services::workspace::{open_db, snapshot_path};

/// Generous for hand-written book CSS; anything larger is not hand-edited.
pub const MAX_STYLESHEET_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StylesheetSummary {
    /// ZIP-root path; the stable identity used by every other command.
    pub href: String,
    /// Path relative to the package document, for display.
    pub name: String,
    pub size: usize,
    pub modified: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StylesheetContent {
    pub href: String,
    pub css: String,
    pub modified: bool,
    pub updated_at: Option<String>,
}

struct BookStyles {
    archive: SafeArchive,
    opf_dir: String,
    /// ZIP-root hrefs of manifest `text/css` items, in manifest href order.
    hrefs: Vec<String>,
}

fn open_book_styles(data_dir: &Path, project_id: &str) -> Result<BookStyles, AppError> {
    let snapshot = snapshot_path(data_dir, project_id)?;
    let mut archive = SafeArchive::open(&snapshot, &ArchiveLimits::default())?;
    let opf_path = container::find_opf_path(&mut archive)?;
    let parsed = opf::parse_opf(&archive.read_entry(&opf_path)?, &opf_path)?;
    let mut hrefs: Vec<String> = parsed
        .manifest
        .values()
        .filter(|item| item.media_type.eq_ignore_ascii_case("text/css"))
        .map(|item| item.href.clone())
        .filter(|href| archive.contains(href))
        .collect();
    hrefs.sort();
    hrefs.dedup();
    let opf_dir = opf_path
        .rfind('/')
        .map(|index| opf_path[..=index].to_string())
        .unwrap_or_default();
    Ok(BookStyles {
        archive,
        opf_dir,
        hrefs,
    })
}

fn require_known(styles: &BookStyles, href: &str) -> Result<(), AppError> {
    if styles.hrefs.iter().any(|known| known == href) {
        Ok(())
    } else {
        Err(AppError::new(
            "stylesheet_not_found",
            "This stylesheet is not part of the book.",
        ))
    }
}

fn decode_original(bytes: Vec<u8>) -> Result<String, AppError> {
    String::from_utf8(bytes).map_err(|_| {
        AppError::new(
            "stylesheet_not_text",
            "This stylesheet is not UTF-8 text, so it cannot be edited here.",
        )
    })
}

fn saved_overrides(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<HashMap<String, (String, String)>, AppError> {
    let mut statement = conn
        .prepare("SELECT href, css, updated_at FROM stylesheet_overrides WHERE project_id = ?1")?;
    let rows = statement.query_map([project_id], |row| {
        Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (href, value) = row?;
        map.insert(href, value);
    }
    Ok(map)
}

fn ensure_project(conn: &rusqlite::Connection, project_id: &str) -> Result<(), AppError> {
    if crate::storage::projects::project_exists(conn, project_id)? {
        Ok(())
    } else {
        Err(AppError::new(
            "project_not_found",
            "This project no longer exists in local storage.",
        ))
    }
}

pub fn list(data_dir: &Path, project_id: &str) -> Result<Vec<StylesheetSummary>, AppError> {
    let conn = open_db(data_dir)?;
    ensure_project(&conn, project_id)?;
    let mut styles = open_book_styles(data_dir, project_id)?;
    let overrides = saved_overrides(&conn, project_id)?;
    let mut summaries = Vec::new();
    for href in styles.hrefs.clone() {
        let saved = overrides.get(&href);
        let size = match saved {
            Some((css, _)) => css.len(),
            None => styles.archive.read_entry(&href)?.len(),
        };
        let name = href
            .strip_prefix(&styles.opf_dir)
            .unwrap_or(&href)
            .to_string();
        summaries.push(StylesheetSummary {
            name,
            size,
            modified: saved.is_some(),
            updated_at: saved.map(|(_, at)| at.clone()),
            href,
        });
    }
    Ok(summaries)
}

pub fn read(data_dir: &Path, project_id: &str, href: &str) -> Result<StylesheetContent, AppError> {
    let conn = open_db(data_dir)?;
    ensure_project(&conn, project_id)?;
    let mut styles = open_book_styles(data_dir, project_id)?;
    require_known(&styles, href)?;
    if let Some((css, updated_at)) = saved_overrides(&conn, project_id)?.remove(href) {
        return Ok(StylesheetContent {
            href: href.to_string(),
            css,
            modified: true,
            updated_at: Some(updated_at),
        });
    }
    Ok(StylesheetContent {
        href: href.to_string(),
        css: decode_original(styles.archive.read_entry(href)?)?,
        modified: false,
        updated_at: None,
    })
}

pub fn save(
    data_dir: &Path,
    project_id: &str,
    href: &str,
    css: &str,
) -> Result<StylesheetContent, AppError> {
    if css.len() > MAX_STYLESHEET_BYTES {
        return Err(AppError::new(
            "stylesheet_too_large",
            "This stylesheet is larger than 2 MB and cannot be saved.",
        ));
    }
    if css.contains('\0') {
        return Err(AppError::new(
            "invalid_stylesheet",
            "The stylesheet contains unsupported characters.",
        ));
    }
    let mut conn = open_db(data_dir)?;
    ensure_project(&conn, project_id)?;
    let mut styles = open_book_styles(data_dir, project_id)?;
    require_known(&styles, href)?;
    // Saving the untouched original text clears the override instead of
    // storing a duplicate, so "Modified" stays meaningful.
    let original = styles.archive.read_entry(href)?;
    let tx = crate::storage::begin_write(&mut conn)?;
    if original == css.as_bytes() {
        tx.execute(
            "DELETE FROM stylesheet_overrides WHERE project_id = ?1 AND href = ?2",
            rusqlite::params![project_id, href],
        )?;
        tx.commit()?;
        return Ok(StylesheetContent {
            href: href.to_string(),
            css: css.to_string(),
            modified: false,
            updated_at: None,
        });
    }
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO stylesheet_overrides (project_id, href, css, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(project_id, href) DO UPDATE SET css = excluded.css, updated_at = excluded.updated_at",
        rusqlite::params![project_id, href, css, now],
    )?;
    tx.commit()?;
    Ok(StylesheetContent {
        href: href.to_string(),
        css: css.to_string(),
        modified: true,
        updated_at: Some(now),
    })
}

/// Drops the saved edit and returns the original stylesheet text.
pub fn reset(data_dir: &Path, project_id: &str, href: &str) -> Result<StylesheetContent, AppError> {
    let mut conn = open_db(data_dir)?;
    ensure_project(&conn, project_id)?;
    let mut styles = open_book_styles(data_dir, project_id)?;
    require_known(&styles, href)?;
    let css = decode_original(styles.archive.read_entry(href)?)?;
    let tx = crate::storage::begin_write(&mut conn)?;
    tx.execute(
        "DELETE FROM stylesheet_overrides WHERE project_id = ?1 AND href = ?2",
        rusqlite::params![project_id, href],
    )?;
    tx.commit()?;
    Ok(StylesheetContent {
        href: href.to_string(),
        css,
        modified: false,
        updated_at: None,
    })
}

/// Saved stylesheet edits for export, keyed by ZIP-root path.
pub fn export_overrides(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Result<HashMap<String, Vec<u8>>, AppError> {
    Ok(saved_overrides(conn, project_id)?
        .into_iter()
        .map(|(href, (css, _))| (href, css.into_bytes()))
        .collect())
}
