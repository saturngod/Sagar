//! Content-server queries and atomic fill-only translation imports.
use crate::{
    error::AppError,
    storage::{self, documents},
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: String,
    pub source_sha256: String,
    pub title: String,
    pub author: Option<String>,
    pub cover_url: String,
    pub source_language: Option<String>,
    pub target_language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub href: String,
    pub title: String,
    pub spine_index: i64,
    pub document: crate::domain::ChapterDocument,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Incoming {
    pub source_sha256: String,
    pub target_language: String,
    pub href: String,
    pub html: String,
    pub expected_revision: i64,
}

pub fn books(conn: &Connection) -> Result<Vec<Book>, AppError> {
    let mut stmt = conn.prepare("SELECT id,source_sha256,title,author,source_language,target_language FROM projects ORDER BY updated_at DESC,id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Book {
            id: r.get(0)?,
            source_sha256: r.get(1)?,
            title: r.get(2)?,
            author: r.get(3)?,
            cover_url: cover_url(&r.get::<_, String>(0)?),
            source_language: r.get(4)?,
            target_language: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Literal substring search over metadata using Unicode lowercase, preserving library order.
/// This intentionally avoids SQLite's ASCII-only default case-insensitive matching.
pub fn search_books(conn: &Connection, query: Option<&str>) -> Result<Vec<Book>, AppError> {
    let query = query.unwrap_or_default().trim().to_lowercase();
    let mut result = books(conn)?;
    if !query.is_empty() {
        result.retain(|book| {
            book.title.to_lowercase().contains(&query)
                || book
                    .author
                    .as_ref()
                    .is_some_and(|author| author.to_lowercase().contains(&query))
        });
    }
    Ok(result)
}

pub fn book(conn: &Connection, id: &str) -> Result<Book, AppError> {
    books(conn)?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| AppError::new("not_found", "Book not found."))
}

pub fn chapters(conn: &Connection, id: &str) -> Result<Vec<Chapter>, AppError> {
    book(conn, id)?;
    let mut stmt = conn.prepare("SELECT id,resource_href,title,spine_index FROM chapters WHERE project_id=?1 ORDER BY spine_index,id")?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, href, title, spine_index) = row?;
        let document = documents::load(conn, &id)?;
        result.push(Chapter {
            id,
            href,
            title,
            spine_index,
            document,
        });
    }
    Ok(result)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedChapters {
    pub chapter_ids: Vec<String>,
}

/// Lightweight inventory matching the chapter document loader's precedence.
/// Reads only IDs; does not load HTML payloads or materialize legacy documents.
pub fn translated_chapters(
    conn: &Connection,
    project: &str,
) -> Result<TranslatedChapters, AppError> {
    book(conn, project)?;
    let mut stmt = conn.prepare(
        "SELECT ch.id FROM chapters ch
         LEFT JOIN chapter_documents d ON d.chapter_id = ch.id
         WHERE ch.project_id = ?1 AND
           CASE WHEN d.chapter_id IS NOT NULL
             THEN length(trim(COALESCE(d.html, ''))) > 0
             ELSE EXISTS (
               SELECT 1 FROM blocks b JOIN translations t ON t.block_id = b.id
               WHERE b.chapter_id = ch.id AND t.target_xhtml_fragment IS NOT NULL
             )
           END
         ORDER BY ch.spine_index, ch.id",
    )?;
    let rows = stmt.query_map([project], |row| row.get::<_, String>(0))?;
    Ok(TranslatedChapters {
        chapter_ids: rows.collect::<Result<Vec<_>, _>>()?,
    })
}

pub fn import(
    conn: &mut Connection,
    project: &str,
    chapter: &str,
    input: Incoming,
) -> Result<serde_json::Value, AppError> {
    let tx = storage::begin_write(conn)?;
    let book = book(&tx, project)?;
    if book.source_sha256 != input.source_sha256 || book.target_language != input.target_language {
        return Err(AppError::new(
            "identity_mismatch",
            "Original EPUB and translation language must match.",
        ));
    }
    let href: Option<String> = tx
        .query_row(
            "SELECT resource_href FROM chapters WHERE project_id=?1 AND id=?2",
            [project, chapter],
            |r| r.get(0),
        )
        .optional()?;
    let href = href.ok_or_else(|| AppError::new("not_found", "Chapter not found."))?;
    if href != input.href {
        return Err(AppError::new(
            "identity_mismatch",
            "Chapter resource must match.",
        ));
    }
    let current = documents::load(&tx, chapter)?;
    // Any saved HTML or explicit keep-original choice is protected, even drafts.
    let legacy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM translations t JOIN blocks b ON b.id=t.block_id WHERE b.chapter_id=?1 AND (t.target_xhtml_fragment IS NOT NULL OR t.status='skipped'))",[chapter],|r|r.get(0))?;
    if current.html.as_ref().is_some_and(|s| !s.trim().is_empty())
        || current.status == "skipped"
        || legacy
    {
        tx.commit()?;
        return Ok(serde_json::json!({"outcome":"skippedExisting"}));
    }
    let (_, plain) = crate::epub::rich_text::accept_document(&input.html)?;
    if plain.trim().is_empty() {
        return Err(AppError::new(
            "empty_translation",
            "Translation must contain text.",
        ));
    }
    let saved = documents::save_in_transaction(
        &tx,
        chapter,
        Some(input.html),
        "draft",
        input.expected_revision,
    )?;
    tx.commit()?;
    Ok(serde_json::json!({"outcome":"imported","revision":saved.revision,"status":"draft"}))
}

/// Origin-relative route; escape IDs as a single URL path segment.
fn cover_url(id: &str) -> String {
    let mut segment = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            segment.push(b as char);
        } else {
            segment.push_str(&format!("%{b:02X}"));
        }
    }
    format!("/api/v1/books/{segment}/cover")
}
