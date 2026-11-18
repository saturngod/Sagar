//! Per-book export metadata and translator guidance. Source snapshots remain immutable.
use crate::error::AppError;
use crate::services::workspace::{open_db, snapshot_path, validate_language_tag};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSettings {
    pub title: String,
    pub author: String,
    pub publisher: String,
    pub description: String,
    pub source_language: String,
    pub target_language: String,
    pub instructions: String,
    #[serde(default)]
    pub custom_cover: Option<String>,
    /// Selected genre translation skill (see `services::skills`).
    #[serde(default)]
    pub genre_skill_id: Option<String>,
    /// Selected target-language translation skill.
    #[serde(default)]
    pub language_skill_id: Option<String>,
}

fn checked(value: &str, name: &str, limit: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.chars().count() > limit
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(AppError::new(
            "invalid_book_settings",
            format!(
                "{name} is too long or contains unsupported characters (maximum {limit} characters)."
            ),
        ));
    }
    Ok(value.to_string())
}

pub fn validate_instructions(value: &str) -> Result<String, AppError> {
    checked(value, "Translation instructions", 20_000)
}

pub fn get(data_dir: &Path, project_id: &str) -> Result<BookSettings, AppError> {
    let conn = open_db(data_dir)?;
    let project = crate::storage::projects::get_project(&conn, project_id)?;
    let mut settings = conn.query_row(
        "SELECT COALESCE(translated_title,title), COALESCE(export_author,author,''),
        export_publisher, export_description, COALESCE(source_language,''), target_language, translation_instructions, custom_cover,
        translation_skill_id, language_skill_id
        FROM projects WHERE id=?1", [project_id], |row| Ok(BookSettings {
            title: row.get(0)?, author: row.get(1)?, publisher: row.get::<_,Option<String>>(2)?.unwrap_or_default(),
            description: row.get::<_,Option<String>>(3)?.unwrap_or_default(), source_language: row.get(4)?,
            target_language: row.get(5)?, instructions: row.get(6)?, custom_cover: row.get(7)?,
            genre_skill_id: row.get(8)?, language_skill_id: row.get(9)?,
        }))?;
    // Read original optional fields for the editor without changing import data.
    let mut archive = crate::epub::archive::SafeArchive::open(
        &snapshot_path(data_dir, &project.id)?,
        &crate::security::ArchiveLimits::default(),
    )?;
    let opf = crate::epub::container::find_opf_path(&mut archive)?;
    let bytes = archive.read_entry(&opf)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::new("invalid_epub", "The book metadata could not be read."))?;
    let doc = roxmltree::Document::parse(text)
        .map_err(|_| AppError::new("invalid_epub", "The book metadata could not be read."))?;
    for (name, field) in [
        ("publisher", &mut settings.publisher),
        ("description", &mut settings.description),
    ] {
        if field.is_empty() {
            if let Some(node) = doc
                .descendants()
                .find(|n| n.has_tag_name(("http://purl.org/dc/elements/1.1/", name)))
            {
                *field = node
                    .descendants()
                    .filter_map(|n| n.text().filter(|_| n.is_text()))
                    .collect();
            }
        }
    }
    Ok(settings)
}

pub fn save(data_dir: &Path, project_id: &str, settings: BookSettings) -> Result<(), AppError> {
    let title = checked(&settings.title, "Title", 1024)?;
    if title.is_empty() {
        return Err(AppError::new(
            "invalid_book_settings",
            "Enter an export title.",
        ));
    }
    let author = checked(&settings.author, "Author", 1024)?;
    let publisher = checked(&settings.publisher, "Publisher", 1024)?;
    let description = checked(&settings.description, "Description", 20_000)?;
    let instructions = validate_instructions(&settings.instructions)?;
    let source = if settings.source_language.trim().is_empty() {
        None
    } else {
        Some(validate_language_tag(&settings.source_language)?)
    };
    let target = validate_language_tag(&settings.target_language)?;
    let genre_skill = crate::services::skills::validate_selection(
        data_dir,
        settings.genre_skill_id.as_deref(),
        crate::services::skills::SkillKind::Genre,
        &target,
    )?;
    let language_skill = crate::services::skills::validate_selection(
        data_dir,
        settings.language_skill_id.as_deref(),
        crate::services::skills::SkillKind::Language,
        &target,
    )?;
    if let Some(cover) = &settings.custom_cover {
        crate::epub::rich_text::image_bytes(cover)?;
    }
    let mut conn = open_db(data_dir)?;
    let tx = crate::storage::begin_write(&mut conn)?;
    let changed = tx.execute("UPDATE projects SET translated_title=?1, export_author=?2, export_publisher=?3,
        export_description=?4, source_language=?5, target_language=?6, translation_instructions=?7, updated_at=?8, custom_cover=?10,
        translation_skill_id=?11, language_skill_id=?12 WHERE id=?9",
        rusqlite::params![title, nonblank(&author), nonblank(&publisher), nonblank(&description), source, target, instructions, chrono::Utc::now().to_rfc3339(), project_id, settings.custom_cover, genre_skill, language_skill])?;
    if changed == 0 {
        return Err(AppError::new(
            "project_not_found",
            "This book no longer exists.",
        ));
    }
    tx.commit()?;
    Ok(())
}
fn nonblank(value: &str) -> Option<&str> {
    if value.is_empty() { None } else { Some(value) }
}
