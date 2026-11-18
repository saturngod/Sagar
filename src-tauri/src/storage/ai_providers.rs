//! Custom AI provider persistence. Secrets are stored in app-owned SQLite
//! and never returned to callers: reads produce masked summaries only. No
//! book content is involved; rows hold endpoint configuration.

use rusqlite::{Connection, OptionalExtension};

use crate::domain::{CustomProviderSummary, TranslationSelection};
use crate::error::AppError;

pub struct NewCustomProvider {
    pub protocol: String,
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub created_at: String,
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> Result<CustomProviderSummary, rusqlite::Error> {
    let api_key: String = row.get(3)?;
    Ok(CustomProviderSummary {
        protocol: row.get(7)?,
        id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        has_api_key: !api_key.trim().is_empty(),
        model: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// Masked summaries in creation order (rowid), oldest first.
pub fn list_custom(conn: &Connection) -> Result<Vec<CustomProviderSummary>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, api_key, model, created_at, updated_at, protocol
         FROM ai_custom_providers ORDER BY rowid",
    )?;
    let rows = stmt.query_map([], summary_from_row)?;
    let mut providers = Vec::new();
    for row in rows {
        providers.push(row?);
    }
    Ok(providers)
}

pub fn create_custom(conn: &Connection, provider: &NewCustomProvider) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO ai_custom_providers
            (id, name, base_url, api_key, model, created_at, updated_at, protocol)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
        rusqlite::params![
            provider.id,
            provider.name,
            provider.base_url,
            provider.api_key,
            provider.model,
            provider.created_at,
            provider.protocol,
        ],
    )?;
    Ok(())
}

/// Secret material for a connection test. Kept inside Rust; never serialized.
pub struct CustomSecret {
    pub protocol: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub fn get_secret(conn: &Connection, id: &str) -> Result<CustomSecret, AppError> {
    conn.query_row(
        "SELECT base_url, api_key, model, protocol FROM ai_custom_providers WHERE id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(CustomSecret {
                protocol: row.get(3)?,
                base_url: row.get(0)?,
                api_key: row.get(1)?,
                model: row.get(2)?,
            })
        },
    )
    .map_err(|_| {
        AppError::new(
            "provider_not_found",
            "This provider no longer exists in local settings.",
        )
    })
}

pub fn update_custom(
    conn: &Connection,
    id: &str,
    name: &str,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    updated_at: &str,
    protocol: &str,
) -> Result<CustomProviderSummary, AppError> {
    if api_key.is_some() {
        let changed = conn.execute(
            "UPDATE ai_custom_providers
             SET name = ?1, base_url = ?2, api_key = ?3, model = ?4, updated_at = ?5, protocol = ?7
             WHERE id = ?6",
            rusqlite::params![name, base_url, api_key, model, updated_at, id, protocol],
        )?;
        if changed == 0 {
            return Err(AppError::new(
                "provider_not_found",
                "This provider no longer exists in local settings.",
            ));
        }
    } else {
        let changed = conn.execute(
            "UPDATE ai_custom_providers
             SET name = ?1, base_url = ?2, model = ?3, updated_at = ?4, protocol = ?6
             WHERE id = ?5",
            rusqlite::params![name, base_url, model, updated_at, id, protocol],
        )?;
        if changed == 0 {
            return Err(AppError::new(
                "provider_not_found",
                "This provider no longer exists in local settings.",
            ));
        }
    }
    conn.query_row(
        "SELECT id, name, base_url, api_key, model, created_at, updated_at, protocol
         FROM ai_custom_providers WHERE id = ?1",
        rusqlite::params![id],
        summary_from_row,
    )
    .map_err(|_| {
        AppError::new(
            "provider_not_found",
            "This provider no longer exists in local settings.",
        )
    })
}

/// Reads the persisted translation selection, if the user chose one.
pub fn get_translation_selection(
    conn: &Connection,
) -> Result<Option<TranslationSelection>, AppError> {
    let row = conn
        .query_row(
            "SELECT agent, model, updated_at, reasoning_effort FROM ai_translation_selection WHERE id = 1",
            [],
            |row| {
                Ok(TranslationSelection {
                    agent: row.get(0)?,
                    model: row.get(1)?,
                    updated_at: row.get(2)?,
                    reasoning_effort: row.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Persists the translation selection, replacing any previous choice.
pub fn set_translation_selection(
    conn: &Connection,
    agent: &str,
    model: &str,
    reasoning_effort: Option<&str>,
    updated_at: &str,
) -> Result<TranslationSelection, AppError> {
    conn.execute(
        "INSERT INTO ai_translation_selection (id, agent, model, updated_at, reasoning_effort)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET agent = ?1, model = ?2, updated_at = ?3, reasoning_effort = ?4",
        rusqlite::params![agent, model, updated_at, reasoning_effort],
    )?;
    Ok(TranslationSelection {
        reasoning_effort: reasoning_effort.map(str::to_string),
        agent: agent.to_string(),
        model: model.to_string(),
        updated_at: updated_at.to_string(),
    })
}

/// Returns false when the provider does not exist.
pub fn delete_custom(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let deleted = conn.execute(
        "DELETE FROM ai_custom_providers WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(deleted > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations;

    fn setup() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open db");
        migrations::apply(&mut conn).expect("migrate");
        conn
    }

    fn sample(id: &str) -> NewCustomProvider {
        NewCustomProvider {
            protocol: "openai".into(),
            id: id.into(),
            name: "Local AI".into(),
            base_url: "http://localhost:1234/v1".into(),
            api_key: "secret".into(),
            model: "local-model".into(),
            created_at: "2026-10-13T00:00:00Z".into(),
        }
    }

    #[test]
    fn protocol_upgrade_defaults_existing_providers_without_changing_secrets() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE ai_custom_providers(id TEXT PRIMARY KEY,name TEXT,base_url TEXT,api_key TEXT,model TEXT,created_at TEXT,updated_at TEXT);
            INSERT INTO ai_custom_providers VALUES ('p','Existing','https://example.invalid/v1','private-key','m','before','before');
            CREATE TABLE projects(id TEXT PRIMARY KEY);
            PRAGMA user_version=11;").unwrap();
        migrations::apply(&mut conn).unwrap();
        assert_eq!(list_custom(&conn).unwrap()[0].protocol, "openai");
        for protocol in ["gemini", "anthropic", "openai"] {
            let summary = update_custom(
                &conn,
                "p",
                "Existing",
                "https://example.invalid/v1",
                None,
                "m",
                "after",
                protocol,
            )
            .unwrap();
            assert_eq!(summary.protocol, protocol);
            let secret = get_secret(&conn, "p").unwrap();
            assert_eq!(secret.protocol, protocol);
            assert_eq!(secret.api_key, "private-key");
            assert!(
                !serde_json::to_string(&summary)
                    .unwrap()
                    .contains("private-key")
            );
        }
    }

    #[test]
    fn migration_preserves_existing_model_and_effort_round_trips() {
        let mut conn = Connection::open_in_memory().expect("db");
        conn.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY);
            CREATE TABLE ai_translation_selection (id INTEGER PRIMARY KEY, agent TEXT NOT NULL, model TEXT NOT NULL, updated_at TEXT NOT NULL);
            INSERT INTO ai_translation_selection VALUES (1, 'codex', 'existing-model', 'before');
            CREATE TABLE ai_custom_providers (id TEXT PRIMARY KEY); PRAGMA user_version = 5;").expect("v5");
        migrations::apply(&mut conn).expect("upgrade");
        let selection = get_translation_selection(&conn)
            .expect("read")
            .expect("saved");
        assert_eq!(selection.model, "existing-model");
        assert_eq!(selection.reasoning_effort, None);
        set_translation_selection(&conn, "codex", "existing-model", Some("xhigh"), "after")
            .expect("save");
        assert_eq!(
            get_translation_selection(&conn)
                .expect("read")
                .expect("saved")
                .reasoning_effort,
            Some("xhigh".into())
        );
        set_translation_selection(&conn, "codex", "existing-model", None, "default")
            .expect("reset");
        assert_eq!(
            get_translation_selection(&conn)
                .expect("read")
                .expect("saved")
                .reasoning_effort,
            None
        );
    }

    #[test]
    fn summaries_mask_the_api_key() {
        let conn = setup();
        create_custom(&conn, &sample("p1")).expect("create");
        let list = list_custom(&conn).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Local AI");
        assert!(list[0].has_api_key);
        // The raw key must not be recoverable from the summary path.
        let serialized = serde_json::to_string(&list[0]).expect("serialize");
        assert!(!serialized.contains("secret"), "{serialized}");
    }

    #[test]
    fn update_without_a_key_keeps_the_secret() {
        let conn = setup();
        create_custom(&conn, &sample("p1")).expect("create");
        update_custom(
            &conn,
            "p1",
            "Renamed",
            "http://localhost:1234/v1",
            None,
            "other-model",
            "2026-10-13T01:00:00Z",
            "openai",
        )
        .expect("update");
        let secret = get_secret(&conn, "p1").expect("secret");
        assert_eq!(secret.api_key, "secret");
        assert_eq!(secret.model, "other-model");
    }

    #[test]
    fn delete_reports_existence() {
        let conn = setup();
        assert!(!delete_custom(&conn, "ghost").expect("delete ghost"));
        create_custom(&conn, &sample("p1")).expect("create");
        assert!(delete_custom(&conn, "p1").expect("delete"));
        assert!(get_secret(&conn, "p1").is_err());
    }

    #[test]
    fn translation_selection_replaces_the_single_row() {
        let conn = setup();
        assert!(
            get_translation_selection(&conn)
                .expect("read empty")
                .is_none()
        );
        set_translation_selection(
            &conn,
            "codex",
            "gpt-5.6-sol",
            Some("high"),
            "2026-10-13T00:00:00Z",
        )
        .expect("save");
        set_translation_selection(
            &conn,
            "opencode",
            "anthropic/claude-sonnet-4-5",
            None,
            "2026-10-13T01:00:00Z",
        )
        .expect("overwrite");
        let selection = get_translation_selection(&conn)
            .expect("read")
            .expect("selection saved");
        assert_eq!(selection.agent, "opencode");
        assert_eq!(selection.model, "anthropic/claude-sonnet-4-5");
    }
}
