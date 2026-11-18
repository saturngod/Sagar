//! Source material for AI-assisted translation proposals. Reads the
//! chapter's saved source text plus the project's language tags. No book
//! content is ever logged; callers send it only to the user's chosen local
//! tool after explicit consent.

use rusqlite::Connection;

use crate::error::AppError;

#[derive(Debug)]
pub struct TranslationSource {
    pub project_id: String,
    /// ZIP-root-relative XHTML resource of the chapter (for snapshot reads).
    pub resource_href: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub instructions: String,
    /// Book Settings translation skills (rendered by `services::skills`).
    pub genre_skill_id: Option<String>,
    pub language_skill_id: Option<String>,
    /// Non-blank source paragraphs in reading order (emptiness check only;
    /// the HTML body travels as translation source).
    pub paragraphs: Vec<String>,
}

/// Loads the translatable source of one chapter. Errors when the chapter is
/// unknown; an empty paragraph list is left for the caller to reject.
pub fn translation_source(
    conn: &Connection,
    chapter_id: &str,
) -> Result<TranslationSource, AppError> {
    #[allow(clippy::type_complexity)]
    let (
        project_id,
        resource_href,
        source_language,
        target_language,
        instructions,
        genre_skill,
        language_skill,
    ): (
        String,
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT ch.project_id, ch.resource_href, p.source_language, p.target_language, p.translation_instructions,
                    p.translation_skill_id, p.language_skill_id
             FROM chapters ch JOIN projects p ON p.id = ch.project_id
             WHERE ch.id = ?1",
            rusqlite::params![chapter_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|_| {
            AppError::new(
                "chapter_not_found",
                "This chapter no longer exists in local storage.",
            )
        })?;
    let mut stmt = conn.prepare(
        "SELECT source_plain_text FROM blocks
         WHERE chapter_id = ?1 ORDER BY block_index",
    )?;
    let rows = stmt.query_map(rusqlite::params![chapter_id], |row| row.get::<_, String>(0))?;
    let mut paragraphs = Vec::new();
    for row in rows {
        let text: String = row?;
        if !text.trim().is_empty() {
            paragraphs.push(text);
        }
    }
    Ok(TranslationSource {
        project_id,
        resource_href,
        source_language,
        target_language,
        instructions,
        genre_skill_id: genre_skill,
        language_skill_id: language_skill,
        paragraphs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations;
    use crate::storage::projects::{NewBlock, NewChapter, NewProject};

    fn setup() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open db");
        migrations::apply(&mut conn).expect("migrate");
        crate::storage::projects::create_project(
            &mut conn,
            &NewProject {
                original_source_path: None,
                id: "p1".into(),
                source_sha256: "abc".into(),
                source_snapshot_path: "projects/p1/source.epub".into(),
                title: "T".into(),
                author: None,
                source_language: Some("en".into()),
                target_language: "my".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
            &[(
                NewChapter {
                    id: "c1".into(),
                    spine_index: 0,
                    manifest_item_id: "m1".into(),
                    resource_href: "c1.xhtml".into(),
                    title: "Chapter".into(),
                },
                vec![
                    NewBlock {
                        id: "b1".into(),
                        block_index: 0,
                        structural_locator: "0".into(),
                        element_name: "p".into(),
                        source_xhtml_fragment: "One".into(),
                        source_plain_text: "One".into(),
                        is_translatable: true,
                        supported: true,
                    },
                    NewBlock {
                        id: "b2".into(),
                        block_index: 1,
                        structural_locator: "1".into(),
                        element_name: "p".into(),
                        source_xhtml_fragment: "  ".into(),
                        source_plain_text: "  ".into(),
                        is_translatable: true,
                        supported: true,
                    },
                    NewBlock {
                        id: "b3".into(),
                        block_index: 2,
                        structural_locator: "2".into(),
                        element_name: "p".into(),
                        source_plain_text: "Two".into(),
                        source_xhtml_fragment: "Two".into(),
                        is_translatable: true,
                        supported: true,
                    },
                ],
            )],
        )
        .expect("create project");
        conn
    }

    #[test]
    fn source_loads_paragraphs_in_order_and_skips_blanks() {
        let conn = setup();
        let source = translation_source(&conn, "c1").expect("source");
        assert_eq!(source.project_id, "p1");
        assert_eq!(source.resource_href, "c1.xhtml");
        assert_eq!(source.source_language.as_deref(), Some("en"));
        assert_eq!(source.target_language, "my");
        assert_eq!(
            source.paragraphs,
            vec!["One".to_string(), "Two".to_string()]
        );
    }

    #[test]
    fn source_carries_the_books_selected_skills() {
        let conn = setup();
        let source = translation_source(&conn, "c1").unwrap();
        assert_eq!(
            (source.genre_skill_id, source.language_skill_id),
            (None, None)
        );
        conn.execute(
            "UPDATE projects SET translation_skill_id='sagar-wuxia', language_skill_id='sagar-target-my'",
            [],
        )
        .unwrap();
        let source = translation_source(&conn, "c1").unwrap();
        assert_eq!(source.genre_skill_id.as_deref(), Some("sagar-wuxia"));
        assert_eq!(source.language_skill_id.as_deref(), Some("sagar-target-my"));
    }

    #[test]
    fn unknown_chapters_are_rejected() {
        let conn = setup();
        let error = translation_source(&conn, "ghost").expect_err("ghost rejected");
        assert_eq!(error.code, "chapter_not_found");
    }
}
