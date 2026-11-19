//! Export use case: review, then rebuild the translated EPUB to a
//! user-chosen destination.

use std::path::{Path, PathBuf};

use crate::domain::{ExportOutcome, ExportReview, ImportWarning};
use crate::epub::export;
use crate::error::AppError;
use crate::services::workspace::open_db;
use crate::storage::{self, translations};

pub fn review_export(data_dir: &Path, project_id: &str) -> Result<ExportReview, AppError> {
    let conn = open_db(data_dir)?;
    storage::documents::review(&conn, project_id)
}

pub struct ExportRequest {
    pub project_id: String,
    pub destination: std::path::PathBuf,
    pub mode: String,
    pub title: Option<String>,
    pub language: Option<String>,
    /// Draft-only: when false, chapters without any saved translation are
    /// omitted from the output instead of falling back to original content.
    pub include_original: bool,
    /// `epub` (default) or `kfx`.
    pub format: String,
}

pub fn export_epub(data_dir: &Path, request: ExportRequest) -> Result<ExportOutcome, AppError> {
    let conn = open_db(data_dir)?;
    if !storage::projects::project_exists(&conn, &request.project_id)? {
        return Err(AppError::new(
            "project_not_found",
            "This project no longer exists in local storage.",
        ));
    }

    // Persist metadata first so the export and the project stay consistent.
    if let Some(title) = &request.title {
        let _ = &title;
    }
    let translated_title = match &request.title {
        Some(title) => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => None,
    };
    if translated_title.is_some() || request.language.is_some() {
        let current: (Option<String>, String) = conn.query_row(
            "SELECT translated_title, target_language FROM projects WHERE id = ?1",
            rusqlite::params![request.project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let language = request.language.clone().unwrap_or(current.1);
        crate::services::workspace::update_project_metadata(
            data_dir,
            &request.project_id,
            translated_title.clone(),
            Some(language),
            None,
        )?;
    }

    let review = storage::documents::review(&conn, &request.project_id)?;
    match request.mode.as_str() {
        "completed" if !review.complete => {
            return Err(AppError::with_details(
                "export_blocked",
                "This book still has untranslated or unreviewed chapters. \
                 Export a draft or finish the review first.",
                serde_json::to_value(&review).unwrap_or(serde_json::Value::Null),
            ));
        }
        "completed" | "draft" => {}
        other => {
            return Err(AppError::new(
                "invalid_export_mode",
                format!("“{other}” is not a valid export mode."),
            ));
        }
    }

    let (destination, snapshot_canonical) =
        safe_destination(&conn, data_dir, &request.project_id, &request.destination)?;
    let format = ExportFormat::parse(&request.format)?;
    // KFX is converted from a validated EPUB written to a private temp dir.
    let staging = tempfile::tempdir()?;
    let epub_destination = match format {
        ExportFormat::Epub => destination.clone(),
        ExportFormat::Kfx => staging.path().join("translated.epub"),
    };

    let mut chapters = Vec::new();
    let chapter_rows = storage::projects::list_chapters(&conn, &request.project_id)?;
    for chapter in &chapter_rows {
        chapters.push((chapter.id.clone(), chapter.resource_href.clone()));
    }

    let project_id = request.project_id.clone();
    // Completed exports always include every chapter; the translated-only
    // filter is a draft-mode option.
    let include_original = if request.mode == "completed" {
        true
    } else {
        request.include_original
    };
    let saved_metadata: (Option<String>, Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT translated_title, export_author, export_publisher, export_description FROM projects WHERE id=?1",
        [&project_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))?;
    let metadata = [
        ("creator", saved_metadata.1),
        ("publisher", saved_metadata.2),
        ("description", saved_metadata.3),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|v| (name.to_string(), v)))
    .collect();
    let outcome = export::run_export(export::ExportInput {
        snapshot_path: snapshot_canonical.clone(),
        destination: epub_destination.clone(),
        translated_title: saved_metadata.0,
        metadata,
        custom_cover: conn.query_row(
            "SELECT custom_cover FROM projects WHERE id=?1",
            [&project_id],
            |row| row.get(0),
        )?,
        target_language: {
            conn.query_row(
                "SELECT target_language FROM projects WHERE id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )?
        },
        chapters,
        include_original,
        resource_overrides: crate::services::stylesheets::export_overrides(&conn, &project_id)?,
        load_blocks: Box::new(move |chapter_id: &str| {
            if let Some(document) = storage::documents::authored(&conn, chapter_id)? {
                return Ok(vec![translations::BlockForExport {
                    structural_locator: "__chapter_document__".into(),
                    element_name: "body".into(),
                    target_fragment: if matches!(document.status.as_str(), "skipped" | "pending") {
                        None
                    } else {
                        document.html
                    },
                    status: document.status,
                }]);
            }
            translations::blocks_for_export(&conn, chapter_id)
        }),
    })?;

    let mut warnings = outcome.warnings;
    if format == ExportFormat::Kfx {
        warnings.extend(write_kfx(&epub_destination, &destination)?);
    }

    let mode = if request.mode == "completed" {
        "completed"
    } else {
        "draft"
    };
    Ok(ExportOutcome {
        output_path: destination.to_string_lossy().into_owned(),
        mode: mode.to_string(),
        chapters_exported: outcome.chapters_exported,
        blocks_translated: outcome.blocks_translated,
        warnings,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Epub,
    Kfx,
}

impl ExportFormat {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "epub" => Ok(Self::Epub),
            "kfx" => Ok(Self::Kfx),
            other => Err(AppError::new(
                "invalid_export_format",
                format!("“{other}” is not a supported export format."),
            )),
        }
    }
}

/// Rejects destinations that would overwrite the user's original book or the
/// app-owned snapshot. Returns the canonical destination and snapshot path.
fn safe_destination(
    conn: &rusqlite::Connection,
    data_dir: &Path,
    project_id: &str,
    requested: &Path,
) -> Result<(PathBuf, PathBuf), AppError> {
    // Never write onto the app-owned snapshot or its directory-local temp.
    let snapshot_rel: String = conn.query_row(
        "SELECT source_snapshot_path FROM projects WHERE id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;
    let snapshot_abs = data_dir.join(&snapshot_rel);
    let destination = requested
        .canonicalize()
        .unwrap_or_else(|_| requested.to_path_buf());
    let snapshot_canonical = snapshot_abs.canonicalize().unwrap_or(snapshot_abs);
    let (original_path, source_hash): (Option<String>, String) = conn.query_row(
        "SELECT original_source_path, source_sha256 FROM projects WHERE id=?1",
        [project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let is_original_path = original_path.is_some_and(|path| {
        let path = std::path::PathBuf::from(path);
        destination == path.canonicalize().unwrap_or(path)
    });
    // Legacy projects lack a source path. Protect unchanged originals (and
    // identical copies) by their persisted import hash as well.
    let is_original_content =
        destination.is_file() && crate::services::import::hash_file(&destination)? == source_hash;
    if destination == snapshot_canonical || is_original_path || is_original_content {
        return Err(AppError::new(
            "destination_unsafe",
            "Choose a separate output file. Export cannot overwrite the original book or its stored copy.",
        ));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok((destination, snapshot_canonical))
}

/// Writes `bytes` beside `destination` and renames it into place, so an
/// interrupted export never leaves a partial file at the chosen path.
fn write_atomically(destination: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    let temp = destination.with_file_name(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    if let Err(error) = std::fs::write(&temp, bytes) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    if let Err(error) = std::fs::rename(&temp, destination) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

fn write_kfx(epub: &Path, destination: &Path) -> Result<Vec<ImportWarning>, AppError> {
    let (bytes, warnings) = crate::kfx::convert_epub(epub)?;
    write_atomically(destination, &bytes)?;
    Ok(warnings)
}

pub struct OriginalExportRequest {
    pub project_id: String,
    pub destination: PathBuf,
    pub format: String,
}

/// Exports the untouched source book: a byte-for-byte copy of the stored
/// snapshot (EPUB) or its KFX conversion. Translations are not applied.
pub fn export_original(
    data_dir: &Path,
    request: OriginalExportRequest,
) -> Result<ExportOutcome, AppError> {
    let conn = open_db(data_dir)?;
    if !storage::projects::project_exists(&conn, &request.project_id)? {
        return Err(AppError::new(
            "project_not_found",
            "This project no longer exists in local storage.",
        ));
    }
    let format = ExportFormat::parse(&request.format)?;
    let (destination, snapshot) =
        safe_destination(&conn, data_dir, &request.project_id, &request.destination)?;
    let chapters_exported = storage::projects::list_chapters(&conn, &request.project_id)?.len();
    let warnings = match format {
        ExportFormat::Epub => {
            write_atomically(&destination, &std::fs::read(&snapshot)?)?;
            Vec::new()
        }
        ExportFormat::Kfx => write_kfx(&snapshot, &destination)?,
    };
    Ok(ExportOutcome {
        output_path: destination.to_string_lossy().into_owned(),
        mode: "original".into(),
        chapters_exported,
        blocks_translated: 0,
        warnings,
    })
}
