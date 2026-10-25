//! Application services: use cases orchestrated for the UI, kept free of
//! both Tauri plumbing and JSX concerns.

pub mod ai;
pub mod ai_selection;
pub mod ai_translate;
pub mod book_settings;
mod codex_stream;
mod custom_stream;
pub mod export;
pub mod fonts;
pub mod import;
mod opencode_stream;
pub mod projects;
mod qoder;
pub mod workspace;

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::storage;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageHealth {
    pub ready: bool,
    pub schema_version: i64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub app_name: String,
    pub app_version: String,
    pub storage: StorageHealth,
}

/// Verifies the local project storage is reachable and migrated. Used by the
/// Milestone 0 system check as the typed end-to-end command path.
pub fn health_check(app: &AppHandle) -> Result<HealthReport, AppError> {
    let app_data_dir = app.path().app_data_dir()?;
    let info = storage::initialize(&app_data_dir)?;
    Ok(HealthReport {
        app_name: app.package_info().name.clone(),
        app_version: app.package_info().version.to_string(),
        storage: StorageHealth {
            ready: true,
            schema_version: info.schema_version,
        },
    })
}
