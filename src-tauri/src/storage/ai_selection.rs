//! Durable advanced translation repository. No provider credentials are saved.
use crate::{epub::selection::SourceUnit, error::AppError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub agent: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub source_language: Option<String>,
    pub target_language: String,
    pub book_instructions: String,
    pub current_instructions: String,
    /// Translation-skill guidance captured when the run was prepared.
    #[serde(default)]
    pub skill_guidance: String,
    pub fingerprint: String,
    pub project_id: String,
    pub resource_href: String,
}
#[derive(Clone, Debug)]
pub struct Run {
    pub id: String,
    pub chapter_id: String,
    pub config: RunConfig,
    pub state: String,
    pub message: Option<String>,
    pub budget: usize,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub unit: SourceUnit,
    pub state: String,
    pub proposal: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Part {
    pub id: String,
    pub item_id: String,
    pub order: i64,
    pub node: usize,
    pub start: usize,
    pub end: usize,
    pub source: String,
    pub output: Option<String>,
    pub state: String,
}

pub fn json<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|_| AppError::new("storage_error", "The translation state could not be saved."))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|_| {
        AppError::new(
            "storage_error",
            "Saved translation state could not be read.",
        )
    })
}

pub fn source_fingerprint(conn: &Connection, project: &str) -> Result<String, AppError> {
    Ok(conn.query_row(
        "SELECT source_sha256 FROM projects WHERE id=?1",
        [project],
        |r| r.get(0),
    )?)
}

pub fn failed_output(conn: &Connection, item: &str) -> Result<Option<String>, AppError> {
    Ok(conn.query_row("SELECT failed_output FROM ai_selection_parts WHERE item_id=?1 AND failed_output IS NOT NULL ORDER BY part_order LIMIT 1",[item],|r|r.get(0)).optional()?)
}

pub fn append_receipt(conn: &Connection, id: &str) -> Result<Option<(String, String)>, AppError> {
    Ok(conn
        .query_row(
            "SELECT chapter_id,proposal_ids FROM ai_selection_appends WHERE request_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

pub fn record_append(
    conn: &Connection,
    id: &str,
    chapter: &str,
    proposals: &str,
    revision: i64,
) -> Result<(), AppError> {
    conn.execute("INSERT INTO ai_selection_appends(request_id,chapter_id,proposal_ids,revision,created_at) VALUES(?1,?2,?3,?4,?5)",params![id,chapter,proposals,revision,chrono::Utc::now().to_rfc3339()])?;
    Ok(())
}

pub fn units(conn: &Connection, chapter: &str) -> Result<Vec<SourceUnit>, AppError> {
    let mut stmt = conn
        .prepare("SELECT data FROM ai_selection_units WHERE chapter_id=?1 ORDER BY source_order")?;
    stmt.query_map([chapter], |r| r.get::<_, String>(0))?
        .map(|r| decode(&r?))
        .collect()
}
pub fn put_unit(
    conn: &Connection,
    chapter: &str,
    fingerprint: &str,
    unit: &SourceUnit,
) -> Result<(), AppError> {
    conn.execute("INSERT OR IGNORE INTO ai_selection_units(id,chapter_id,source_order,locator,fingerprint,data) VALUES(?1,?2,?3,?4,?5,?6)",params![unit.id,chapter,unit.order,unit.locator,fingerprint,json(unit)?])?;
    Ok(())
}
/// Re-homes a cached unit whose structural locator changed (for example when
/// a validator upgrade merges wrapper spans into their parent div). Keeping
/// the id means saved run items and proposals stay attached, so the unit is
/// never silently orphaned.
pub fn replace_unit(
    conn: &Connection,
    chapter: &str,
    fingerprint: &str,
    unit: &SourceUnit,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_units SET source_order=?1, locator=?2, fingerprint=?3, data=?4 WHERE id=?5 AND chapter_id=?6",
        params![unit.order, unit.locator, fingerprint, json(unit)?, unit.id, chapter],
    )?;
    Ok(())
}

/// Removes a cached unit only when no saved run item references it. Units that
/// already carry translation work are kept even if their locator disappeared.
pub fn delete_unit_if_unreferenced(conn: &Connection, unit_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM ai_selection_units WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM ai_selection_items WHERE unit_id=?1)",
        [unit_id],
    )?;
    Ok(())
}
pub fn block_id(
    conn: &Connection,
    chapter: &str,
    locator: &str,
) -> Result<Option<String>, AppError> {
    Ok(conn
        .query_row(
            "SELECT id FROM blocks WHERE chapter_id=?1 AND structural_locator=?2",
            params![chapter, locator],
            |r| r.get(0),
        )
        .optional()?)
}
pub fn run(conn: &Connection, id: &str) -> Result<Run, AppError> {
    let row = conn
        .query_row(
            "SELECT id,chapter_id,config,state,message,budget FROM ai_selection_runs WHERE id=?1 AND deleted_at IS NULL",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, String>(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::new(
                "run_not_found",
                "This saved translation run no longer exists.",
            )
        })?;
    Ok(Run {
        id: row.0,
        chapter_id: row.1,
        config: decode(&row.2)?,
        state: row.3,
        message: row.4,
        budget: row.5,
    })
}
pub fn runs(conn: &Connection, chapter: &str) -> Result<Vec<Run>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ai_selection_runs WHERE chapter_id=?1 AND deleted_at IS NULL ORDER BY created_at DESC,id",
    )?;
    stmt.query_map([chapter], |r| r.get::<_, String>(0))?
        .map(|r| run(conn, &r?))
        .collect()
}
pub fn create_run(
    conn: &mut Connection,
    chapter: &str,
    config: &RunConfig,
    selected: &[SourceUnit],
) -> Result<String, AppError> {
    let tx = crate::storage::begin_write(conn)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute("INSERT INTO ai_selection_runs(id,chapter_id,config,state,created_at,updated_at) VALUES(?1,?2,?3,'paused',?4,?4)",params![id,chapter,json(config)?,now])?;
    for unit in selected {
        tx.execute(
            "INSERT INTO ai_selection_items(id,run_id,unit_id,state) VALUES(?1,?2,?3,'pending')",
            params![uuid::Uuid::new_v4().to_string(), id, unit.id],
        )?;
    }
    tx.commit()?;
    Ok(id)
}
pub fn set_run(
    conn: &Connection,
    id: &str,
    state: &str,
    message: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_runs SET state=?1,message=?2,updated_at=?3 WHERE id=?4",
        params![state, message, chrono::Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}
pub fn items(conn: &Connection, run: &str) -> Result<Vec<Item>, AppError> {
    let mut stmt=conn.prepare("SELECT i.id,u.data,i.state,i.proposal FROM ai_selection_items i JOIN ai_selection_units u ON u.id=i.unit_id WHERE i.run_id=?1 ORDER BY u.source_order")?;
    stmt.query_map([run], |r| {
        Ok((r.get(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?))
    })?
    .map(|r| {
        let (id, data, state, proposal) = r?;
        Ok(Item {
            id,
            unit: decode(&data)?,
            state,
            proposal,
        })
    })
    .collect()
}
pub fn set_item(
    conn: &Connection,
    id: &str,
    state: &str,
    proposal: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_items SET state=?1,proposal=?2 WHERE id=?3",
        params![state, proposal, id],
    )?;
    Ok(())
}
pub fn parts(conn: &Connection, item: &str) -> Result<Vec<Part>, AppError> {
    let mut stmt=conn.prepare("SELECT id,item_id,part_order,node_index,start_byte,end_byte,source,output,state FROM ai_selection_parts WHERE item_id=?1 ORDER BY part_order,node_index,start_byte")?;
    Ok(stmt
        .query_map([item], |r| {
            Ok(Part {
                id: r.get(0)?,
                item_id: r.get(1)?,
                order: r.get(2)?,
                node: r.get(3)?,
                start: r.get(4)?,
                end: r.get(5)?,
                source: r.get(6)?,
                output: r.get(7)?,
                state: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}
pub fn insert_parts(
    conn: &mut Connection,
    item: &str,
    parts: &[crate::epub::selection::TextPart],
) -> Result<(), AppError> {
    let tx = crate::storage::begin_write(conn)?;
    for (order, p) in parts.iter().enumerate() {
        tx.execute("INSERT INTO ai_selection_parts(id,item_id,part_order,node_index,start_byte,end_byte,source) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![uuid::Uuid::new_v4().to_string(),item,order,p.node,p.start,p.end,p.text])?;
    }
    tx.commit()?;
    Ok(())
}
pub fn save_part(conn: &Connection, id: &str, output: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_parts SET output=?1,state='ready',failed_output=NULL WHERE id=?2",
        params![output, id],
    )?;
    Ok(())
}
pub fn attempt(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_parts SET attempts=attempts+1 WHERE id=?1",
        [id],
    )?;
    Ok(())
}
pub fn fail_part(conn: &Connection, id: &str, text: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE ai_selection_parts SET state='failed',failed_output=?1 WHERE id=?2",
        params![text, id],
    )?;
    Ok(())
}
pub fn reduce_budget(conn: &mut Connection, run_id: &str) -> Result<(), AppError> {
    let current = run(conn, run_id)?;
    if current.budget <= 512 {
        return Err(AppError::new(
            "minimum_batch_size",
            "The smallest batch size has been reached. Retry, skip this item, or choose another model.",
        ));
    }
    let tx = crate::storage::begin_write(conn)?;
    tx.execute(
        "UPDATE ai_selection_runs SET budget=?1 WHERE id=?2",
        params![(current.budget / 2).max(512), run_id],
    )?;
    // Only unfinished ranges are rebuilt; completed translated ranges survive.
    let unfinished: Vec<_> = items(&tx, run_id)?
        .into_iter()
        .filter(|i| {
            !matches!(
                i.state.as_str(),
                "ready" | "skipped" | "added" | "superseded"
            )
        })
        .collect();
    for item in unfinished {
        let saved = parts(&tx, &item.id)?;
        for part in saved.iter().filter(|p| p.state != "ready") {
            tx.execute("DELETE FROM ai_selection_parts WHERE id=?1", [&part.id])?;
            let chunks = crate::epub::selection::parts(
                &format!(
                    "<p>{}</p>",
                    crate::epub::preview::escape_html_text(&part.source)
                ),
                (current.budget / 2).max(512),
            )?;
            for chunk in chunks {
                tx.execute("INSERT INTO ai_selection_parts(id,item_id,part_order,node_index,start_byte,end_byte,source) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![uuid::Uuid::new_v4().to_string(),item.id,part.order,part.node,part.start+chunk.start,part.start+chunk.end,chunk.text])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}
pub fn added_units(conn: &Connection, chapter: &str) -> Result<Vec<String>, AppError> {
    let mut stmt =
        conn.prepare("SELECT proposal_ids FROM ai_selection_appends WHERE chapter_id=?1")?;
    let mut result = vec![];
    for row in stmt.query_map([chapter], |r| r.get::<_, String>(0))? {
        let ids: Vec<String> = decode(&row?)?;
        for id in ids {
            if let Some(unit) = conn
                .query_row(
                    "SELECT unit_id FROM ai_selection_items WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?
            {
                result.push(unit);
            }
        }
    }
    Ok(result)
}

/// Retain only identity mappings needed by existing append receipts.
pub fn delete_run(conn: &mut Connection, chapter: &str, id: &str) -> Result<(), AppError> {
    let tx = crate::storage::begin_write(conn)?;
    let saved = run(&tx, id)?;
    if saved.chapter_id != chapter {
        return Err(AppError::new(
            "run_not_found",
            "This run does not belong to this chapter.",
        ));
    }
    tx.execute("DELETE FROM ai_selection_parts WHERE item_id IN (SELECT id FROM ai_selection_items WHERE run_id=?1)", [id])?;
    tx.execute(
        "UPDATE ai_selection_items SET proposal=NULL, state='skipped' WHERE run_id=?1",
        [id],
    )?;
    tx.execute(
        "UPDATE ai_selection_runs SET deleted_at=?2, config='{}', message=NULL WHERE id=?1",
        params![id, chrono::Utc::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Caller owns the transaction. Keep receipt identities, discard consumed content.
pub fn consume_item(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM ai_selection_parts WHERE item_id=?1 AND EXISTS (SELECT 1 FROM ai_selection_items WHERE id=?1 AND state='ready')", [id])?;
    conn.execute(
        "UPDATE ai_selection_items SET proposal=NULL,state='added' WHERE id=?1 AND state='ready'",
        [id],
    )?;
    Ok(())
}

pub fn consume_ready_versions(conn: &Connection, unit: &str) -> Result<(), AppError> {
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM ai_selection_items WHERE unit_id=?1 AND state='ready'")?
        .query_map([unit], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        consume_item(conn, &id)?;
    }
    Ok(())
}

/// Clean accepted outputs retained by older app versions, never newly translated proposals.
pub fn cleanup_receipted_proposals(conn: &Connection, chapter: &str) -> Result<(), AppError> {
    let rows: Vec<String> = conn
        .prepare("SELECT proposal_ids FROM ai_selection_appends WHERE chapter_id=?1")?
        .query_map([chapter], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for row in rows {
        for id in decode::<Vec<String>>(&row)? {
            consume_item(conn, &id)?;
        }
    }
    Ok(())
}

fn retain_newest_ready(conn: &Connection, unit: &str) -> Result<(), AppError> {
    let ids: Vec<String> = conn.prepare("SELECT i.id FROM ai_selection_items i JOIN ai_selection_runs r ON r.id=i.run_id WHERE i.unit_id=?1 AND i.state='ready' AND r.deleted_at IS NULL ORDER BY r.created_at DESC,r.id")?
        .query_map([unit], |row| row.get(0))?.collect::<Result<_,_>>()?;
    for id in ids.into_iter().skip(1) {
        conn.execute("DELETE FROM ai_selection_parts WHERE item_id=?1", [&id])?;
        conn.execute(
            "UPDATE ai_selection_items SET proposal=NULL,state='superseded' WHERE id=?1",
            [&id],
        )?;
    }
    Ok(())
}

/// Only a complete, validated success can replace an earlier proposal.
pub fn publish_proposal(conn: &mut Connection, id: &str, html: &str) -> Result<(), AppError> {
    let tx = crate::storage::begin_write(conn)?;
    set_item(&tx, id, "ready", Some(html))?;
    let unit: String = tx.query_row(
        "SELECT unit_id FROM ai_selection_items WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    retain_newest_ready(&tx, &unit)?;
    tx.commit()?;
    Ok(())
}

pub fn cleanup_replaced_proposals(conn: &Connection, chapter: &str) -> Result<(), AppError> {
    let units: Vec<String> = conn
        .prepare("SELECT id FROM ai_selection_units WHERE chapter_id=?1")?
        .query_map([chapter], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for unit in units {
        retain_newest_ready(conn, &unit)?;
    }
    Ok(())
}
