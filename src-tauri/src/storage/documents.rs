//! Whole-chapter drafts, independent of source segmentation. Legacy drafts stay intact.
use crate::domain::{ChapterDocument, ChapterReview, ExportReview, SaveResult};
use crate::error::AppError;
use rusqlite::{Connection, OptionalExtension};

pub fn load(conn: &Connection, chapter_id: &str) -> Result<ChapterDocument, AppError> {
    let existing = conn
        .query_row(
            "SELECT html,status,revision,authored FROM chapter_documents WHERE chapter_id=?1",
            [chapter_id],
            |r| {
                Ok(ChapterDocument {
                    html: r.get(0)?,
                    status: r.get(1)?,
                    revision: r.get(2)?,
                    authored: r.get(3)?,
                })
            },
        )
        .optional()?;
    if let Some(document) = existing {
        return Ok(document);
    }
    let html = legacy_html(conn, chapter_id)?;
    let status = if html.is_some() { "draft" } else { "pending" };
    crate::storage::with_busy_retry(|| {
        conn.execute(
            "INSERT OR IGNORE INTO chapter_documents(chapter_id,html,status) VALUES(?1,?2,?3)",
            rusqlite::params![chapter_id, html, status],
        )?;
        Ok(())
    })?;
    conn.query_row(
        "SELECT html,status,revision,authored FROM chapter_documents WHERE chapter_id=?1",
        [chapter_id],
        |r| {
            Ok(ChapterDocument {
                html: r.get(0)?,
                status: r.get(1)?,
                revision: r.get(2)?,
                authored: r.get(3)?,
            })
        },
    )
    .map_err(Into::into)
}

/// Read-only view of a chapter's saved translation using the same precedence
/// as [`load`], without materializing a document row for legacy drafts.
pub fn peek(conn: &Connection, chapter_id: &str) -> Result<(Option<String>, String), AppError> {
    let existing = conn
        .query_row(
            "SELECT html,status FROM chapter_documents WHERE chapter_id=?1",
            [chapter_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some(found) = existing {
        return Ok(found);
    }
    let html = legacy_html(conn, chapter_id)?;
    let status = if html.is_some() { "draft" } else { "pending" };
    Ok((html, status.to_string()))
}

/// Brings forward saved block translations only; never seeds with source text.
fn legacy_html(conn: &Connection, chapter_id: &str) -> Result<Option<String>, AppError> {
    let mut stmt = conn.prepare("SELECT b.element_name,t.target_xhtml_fragment FROM blocks b JOIN translations t ON t.block_id=b.id WHERE b.chapter_id=?1 AND t.target_xhtml_fragment IS NOT NULL ORDER BY b.block_index")?;
    let rows = stmt.query_map([chapter_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut html = String::new();
    for row in rows {
        let (tag, fragment) = row?;
        let tag = if matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            tag.as_str()
        } else {
            "p"
        };
        html.push_str(&format!("<{tag}>{fragment}</{tag}>"));
    }
    Ok(if html.is_empty() { None } else { Some(html) })
}

pub fn authored(conn: &Connection, chapter_id: &str) -> Result<Option<ChapterDocument>, AppError> {
    let doc = load(conn, chapter_id)?;
    Ok(if doc.authored { Some(doc) } else { None })
}

pub fn save(
    conn: &mut Connection,
    chapter_id: &str,
    html: Option<String>,
    status: &str,
    revision: i64,
) -> Result<SaveResult, AppError> {
    crate::storage::with_busy_retry(|| {
        let tx = crate::storage::begin_write(&mut *conn)?;
        let result = save_in_transaction(&tx, chapter_id, html.clone(), status, revision)?;
        tx.commit()?;
        Ok(result)
    })
}

/// Caller owns the transaction so an advanced append and its receipt commit
/// together. Normal chapter saves use the same validation/revision rules.
pub(crate) fn save_in_transaction(
    conn: &Connection,
    chapter_id: &str,
    html: Option<String>,
    status: &str,
    revision: i64,
) -> Result<SaveResult, AppError> {
    if !matches!(status, "draft" | "reviewed" | "skipped") {
        return Err(AppError::new("invalid_status", "Invalid chapter status."));
    }
    // Chapter drafts keep unsupported, invalid or malformed HTML exactly as
    // written so a successful translation is never lost to editor formatting
    // limits. Active content is still rejected by the permissive validator.
    let (html, plain) = crate::epub::rich_text::accept_document(html.as_deref().unwrap_or(""))?;
    let has_content = !plain.trim().is_empty();
    if status == "reviewed" && !has_content {
        return Err(AppError::new(
            "review_requires_content",
            "Write a translation first, or choose Keep original chapter.",
        ));
    }
    let tx = conn;
    let current = load(tx, chapter_id)?;
    if current.revision != revision {
        return Err(AppError::new(
            "revision_conflict",
            "A newer chapter draft exists. Your local draft is kept; reload to resolve the conflict.",
        ));
    }
    let status = if status == "skipped" {
        "skipped"
    } else if has_content {
        status
    } else {
        "pending"
    };
    let content = if has_content { Some(html) } else { None };
    let now = chrono::Utc::now().to_rfc3339();
    let count=tx.execute("UPDATE chapter_documents SET html=?1,status=?2,revision=revision+1,authored=1,updated_at=?3 WHERE chapter_id=?4 AND revision=?5",rusqlite::params![content,status,now,chapter_id,revision])?;
    if count != 1 {
        return Err(AppError::new(
            "revision_conflict",
            "A newer chapter draft exists.",
        ));
    }
    let chapter_status = match status {
        "reviewed" | "skipped" => "completed",
        "draft" => "in_progress",
        _ => "not_started",
    };
    tx.execute(
        "UPDATE chapters SET status=?1 WHERE id=?2",
        rusqlite::params![chapter_status, chapter_id],
    )?;
    tx.execute(
        "UPDATE projects SET updated_at=?1 WHERE id=(SELECT project_id FROM chapters WHERE id=?2)",
        rusqlite::params![now, chapter_id],
    )?;
    Ok(SaveResult {
        block_id: chapter_id.into(),
        revision: revision + 1,
        status: status.into(),
        saved_at: now,
        chapter_status: chapter_status.into(),
    })
}

pub fn review(conn: &Connection, project_id: &str) -> Result<ExportReview, AppError> {
    let mut report = super::translations::export_review(conn, project_id)?;
    report.chapters.clear();
    report.reviewed_total = 0;
    report.skipped_total = 0;
    report.draft_total = 0;
    report.pending_total = 0;
    report.unsupported_total = 0;
    for chapter in super::projects::list_chapters(conn, project_id)? {
        let doc = load(conn, &chapter.id)?;
        let reviewed = i64::from(doc.status == "reviewed");
        let skipped = i64::from(doc.status == "skipped");
        let draft = i64::from(doc.status == "draft");
        let pending = i64::from(doc.status == "pending");
        report.reviewed_total += reviewed;
        report.skipped_total += skipped;
        report.draft_total += draft;
        report.pending_total += pending;
        report.chapters.push(ChapterReview {
            chapter_id: chapter.id,
            spine_index: chapter.spine_index,
            title: chapter.title,
            total_blocks: 1,
            reviewed,
            skipped,
            draft,
            pending,
            unsupported: 0,
        });
    }
    report.complete =
        !report.chapters.is_empty() && report.pending_total == 0 && report.draft_total == 0;
    Ok(report)
}
