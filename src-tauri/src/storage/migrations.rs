//! Ordered, embedded SQLite migrations.
//!
//! Each entry bumps `PRAGMA user_version` to its version inside a
//! transaction. Migrations must never mutate data destructively without an
//! explicit migration step; never drop a user's translation database.

use crate::error::AppError;
use rusqlite::Connection;

/// (version, SQL script). Versions must be strictly increasing.
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        "CREATE TABLE IF NOT EXISTS app_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    ),
    // Translation project schema from dev/plan.md §5 (proposed SQLite v1).
    (
        2,
        "CREATE TABLE projects (
            id                   TEXT PRIMARY KEY,
            source_sha256        TEXT NOT NULL,
            source_snapshot_path TEXT NOT NULL,
            title                TEXT NOT NULL,
            author               TEXT,
            source_language      TEXT,
            target_language      TEXT NOT NULL DEFAULT 'my',
            translated_title     TEXT,
            created_at           TEXT NOT NULL,
            updated_at           TEXT NOT NULL,
            last_chapter_id      TEXT,
            schema_version       INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE chapters (
            id               TEXT PRIMARY KEY,
            project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            spine_index      INTEGER NOT NULL,
            manifest_item_id TEXT NOT NULL,
            resource_href    TEXT NOT NULL,
            title            TEXT NOT NULL,
            status           TEXT NOT NULL DEFAULT 'not_started',
            UNIQUE (project_id, spine_index)
        );

        CREATE TABLE blocks (
            id                    TEXT PRIMARY KEY,
            chapter_id            TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
            block_index           INTEGER NOT NULL,
            structural_locator    TEXT NOT NULL,
            element_name          TEXT NOT NULL,
            source_xhtml_fragment TEXT NOT NULL,
            source_plain_text     TEXT NOT NULL,
            is_translatable       INTEGER NOT NULL DEFAULT 1,
            support_status        TEXT NOT NULL DEFAULT 'supported',
            UNIQUE (chapter_id, block_index)
        );

        CREATE TABLE translations (
            block_id              TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE,
            target_xhtml_fragment TEXT,
            target_plain_text     TEXT,
            status                TEXT NOT NULL DEFAULT 'pending',
            revision              INTEGER NOT NULL DEFAULT 0,
            updated_at            TEXT,
            reviewed_at           TEXT
        );

        CREATE INDEX idx_chapters_project ON chapters(project_id, spine_index);
        CREATE INDEX idx_blocks_chapter ON blocks(chapter_id, block_index);

        CREATE VIEW project_progress AS
        SELECT ch.project_id AS project_id,
               COUNT(b.id) AS total,
               SUM(CASE WHEN t.status IN ('reviewed', 'skipped')
                        THEN 1 ELSE 0 END) AS reviewed,
               SUM(CASE WHEN b.support_status != 'supported'
                        THEN 1 ELSE 0 END) AS unsupported
        FROM chapters ch
        JOIN blocks b ON b.chapter_id = ch.id
        LEFT JOIN translations t ON t.block_id = b.id
        GROUP BY ch.project_id;",
    ),
    (
        3,
        "CREATE TABLE chapter_documents (
        chapter_id TEXT PRIMARY KEY REFERENCES chapters(id) ON DELETE CASCADE,
        html TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        revision INTEGER NOT NULL DEFAULT 0,
        authored INTEGER NOT NULL DEFAULT 0,
        updated_at TEXT
    );
    DROP VIEW project_progress;
    CREATE VIEW project_progress AS
    SELECT ch.project_id, COUNT(ch.id) AS total,
        COALESCE(SUM(d.status IN ('reviewed','skipped')),0) AS reviewed,
        0 AS unsupported
    FROM chapters ch LEFT JOIN chapter_documents d ON d.chapter_id=ch.id GROUP BY ch.project_id;",
    ),
    // AI provider settings (settings-only slice): user-added OpenAI-compatible
    // endpoints. Secrets stay in app-owned SQLite and are never serialized
    // back; summaries only report key presence.
    (
        4,
        "CREATE TABLE ai_custom_providers (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            base_url    TEXT NOT NULL,
            api_key     TEXT NOT NULL,
            model       TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );",
    ),
    // Active translation agent + model choice for future translation work.
    // Single row (id = 1); selecting never starts translation.
    (
        5,
        "CREATE TABLE ai_translation_selection (
            id          INTEGER PRIMARY KEY CHECK (id = 1),
            agent       TEXT NOT NULL,
            model       TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );",
    ),
    (
        6,
        "ALTER TABLE ai_translation_selection ADD COLUMN reasoning_effort TEXT;",
    ),
    (
        7,
        "ALTER TABLE projects ADD COLUMN export_author TEXT;
         ALTER TABLE projects ADD COLUMN export_publisher TEXT;
         ALTER TABLE projects ADD COLUMN export_description TEXT;
         ALTER TABLE projects ADD COLUMN translation_instructions TEXT NOT NULL DEFAULT '';",
    ),
    (
        8,
        "ALTER TABLE projects ADD COLUMN original_source_path TEXT;",
    ),
    (9, "ALTER TABLE projects ADD COLUMN custom_cover TEXT;"),
    (10, "CREATE TABLE ai_selection_units (
        id TEXT PRIMARY KEY, chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
        source_order INTEGER NOT NULL, locator TEXT NOT NULL, fingerprint TEXT NOT NULL,
        data TEXT NOT NULL, UNIQUE(chapter_id,locator));
      CREATE TABLE ai_selection_runs (
        id TEXT PRIMARY KEY, chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
        config TEXT NOT NULL, state TEXT NOT NULL, message TEXT, budget INTEGER NOT NULL DEFAULT 4096,
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
      CREATE TABLE ai_selection_items (
        id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES ai_selection_runs(id) ON DELETE CASCADE,
        unit_id TEXT NOT NULL REFERENCES ai_selection_units(id) ON DELETE CASCADE,
        state TEXT NOT NULL, proposal TEXT, UNIQUE(run_id,unit_id));
      CREATE TABLE ai_selection_parts (
        id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES ai_selection_items(id) ON DELETE CASCADE,
        part_order INTEGER NOT NULL, node_index INTEGER NOT NULL, start_byte INTEGER NOT NULL,
        end_byte INTEGER NOT NULL, source TEXT NOT NULL, output TEXT, state TEXT NOT NULL DEFAULT 'pending',
        attempts INTEGER NOT NULL DEFAULT 0, failed_output TEXT);
      CREATE TABLE ai_selection_appends (
        request_id TEXT PRIMARY KEY, chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
        proposal_ids TEXT NOT NULL, revision INTEGER NOT NULL, created_at TEXT NOT NULL);
      CREATE INDEX ai_selection_runs_chapter ON ai_selection_runs(chapter_id,created_at);
      CREATE INDEX ai_selection_parts_item ON ai_selection_parts(item_id,part_order);"),
    (11, "ALTER TABLE ai_selection_runs ADD COLUMN deleted_at TEXT;"),
    (12, "ALTER TABLE ai_custom_providers ADD COLUMN protocol TEXT NOT NULL DEFAULT 'openai' CHECK(protocol IN ('openai', 'gemini', 'anthropic'));"),
    (
        13,
        "ALTER TABLE projects ADD COLUMN translation_skill_id TEXT;
         ALTER TABLE projects ADD COLUMN language_skill_id TEXT;",
    ),
];

/// Highest defined schema version. Currently asserted on by tests; production
/// callers (import, export) start using it from Milestone 1.
#[cfg(test)]
pub fn latest_version() -> i64 {
    MIGRATIONS
        .iter()
        .map(|(version, _)| *version)
        .max()
        .unwrap_or(0)
}

/// Applies pending migrations and returns the resulting schema version.
/// Safe to call repeatedly: already-applied scripts are skipped.
pub fn apply(conn: &mut Connection) -> Result<i64, AppError> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (target, script) in MIGRATIONS {
        let target = *target;
        if target <= version {
            continue;
        }
        let tx = crate::storage::begin_write(&mut *conn)?;
        tx.execute_batch(script)?;
        tx.pragma_update(None, "user_version", target)?;
        tx.commit()?;
        version = target;
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_with_unique_versions() {
        let mut previous = 0;
        for (version, _) in MIGRATIONS {
            assert!(
                *version > previous,
                "migration versions must strictly increase"
            );
            previous = *version;
        }
    }

    #[test]
    fn selection_upgrade_preserves_saved_chapter_and_source_identity() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        for (version, script) in MIGRATIONS.iter().filter(|(version, _)| *version <= 9) {
            conn.execute_batch(script).expect("prior migration");
            conn.pragma_update(None, "user_version", version)
                .expect("version");
        }
        conn.execute_batch("INSERT INTO projects(id,source_sha256,source_snapshot_path,title,created_at,updated_at) VALUES('p','immutable-hash','projects/p/source.epub','T','t','t'); INSERT INTO chapters(id,project_id,spine_index,manifest_item_id,resource_href,title) VALUES('c','p',0,'c','c.xhtml','C'); INSERT INTO chapter_documents(chapter_id,html,status,revision,authored) VALUES('c','<p>မြန်မာစာ</p>','reviewed',5,1);").expect("saved v9 project");
        apply(&mut conn).expect("upgrade");
        apply(&mut conn).expect("idempotent");
        let row:(String,String,i64,String)=conn.query_row("SELECT p.source_sha256,d.html,d.revision,d.status FROM projects p JOIN chapters c ON c.project_id=p.id JOIN chapter_documents d ON d.chapter_id=c.id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).expect("preserved");
        assert_eq!(
            row,
            (
                "immutable-hash".into(),
                "<p>မြန်မာစာ</p>".into(),
                5,
                "reviewed".into()
            )
        );
    }
}

#[cfg(test)]
mod book_settings_migration_tests {
    use super::*;
    #[test]
    fn v7_preserves_existing_metadata_and_translation_documents() {
        let mut conn = Connection::open_in_memory().expect("db");
        conn.execute_batch(
            "CREATE TABLE projects(id TEXT PRIMARY KEY,title TEXT,translated_title TEXT);
          INSERT INTO projects VALUES('p','Original','Translated');
          CREATE TABLE chapter_documents(chapter_id TEXT,html TEXT,revision INTEGER);
          INSERT INTO chapter_documents VALUES('c','<p>Saved draft</p>',4);
          CREATE TABLE ai_custom_providers (id TEXT PRIMARY KEY); PRAGMA user_version=6;",
        )
        .expect("v6");
        apply(&mut conn).expect("migrate");
        apply(&mut conn).expect("idempotent");
        let (title, instructions): (String, String) = conn
            .query_row(
                "SELECT translated_title,translation_instructions FROM projects",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("project");
        assert_eq!(title, "Translated");
        assert_eq!(instructions, "");
        let revision: i64 = conn
            .query_row("SELECT revision FROM chapter_documents", [], |r| r.get(0))
            .expect("draft");
        assert_eq!(revision, 4);
    }
}

#[cfg(test)]
mod source_path_upgrade_tests {
    use super::*;

    #[test]
    fn upgrade_preserves_existing_projects_and_leaves_unknown_paths_null() {
        let mut conn = Connection::open_in_memory().unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 7) {
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute("INSERT INTO projects(id, source_sha256, source_snapshot_path, title, created_at, updated_at) VALUES('old','hash','projects/old/source.epub','Existing book','now','now')", []).unwrap();
        assert_eq!(apply(&mut conn).unwrap(), latest_version());
        let (title, path): (String, Option<String>) = conn
            .query_row(
                "SELECT title, original_source_path FROM projects WHERE id='old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Existing book");
        assert_eq!(path, None);
    }
}

#[cfg(test)]
mod translation_skill_upgrade_tests {
    use super::*;

    #[test]
    fn v13_keeps_existing_books_and_starts_without_skills() {
        let mut conn = Connection::open_in_memory().unwrap();
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 12) {
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute("INSERT INTO projects(id, source_sha256, source_snapshot_path, title, created_at, updated_at, translation_instructions) VALUES('old','hash','projects/old/source.epub','Existing book','now','now','Keep names')", []).unwrap();
        assert_eq!(apply(&mut conn).unwrap(), latest_version());
        let row: (String, String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT title, translation_instructions, translation_skill_id, language_skill_id FROM projects WHERE id='old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("Existing book".into(), "Keep names".into(), None, None)
        );
    }
}
