//! Thin Tauri command adapters. Business logic lives in `services`; commands
//! only translate between the WebView and service calls.

use tauri::{AppHandle, Emitter, Manager};

use crate::domain::{
    AiProviderList, AiTranslationEvent, AiTranslationStart, ChapterContent, ChapterSummary,
    CliCheckResult, CoverImage, CustomProviderSummary, CustomTestResult, ExportOutcome,
    ExportReview, ImportResult, ProjectSummary, SaveResult, TranslationModelList,
    TranslationSelection,
};
use crate::error::AppError;
use crate::services::{self, HealthReport};
use crate::storage::translations::SaveInput;

#[tauri::command]
pub async fn get_ai_selection_state(
    app: AppHandle,
    chapter_id: String,
) -> Result<services::ai_selection::SelectionState, AppError> {
    let dir = app_data_dir(&app)?;
    run_blocking(move || services::ai_selection::load(&dir, &chapter_id)).await
}

#[tauri::command]
pub async fn prepare_ai_selection_run(
    app: AppHandle,
    input: services::ai_selection::PrepareInput,
) -> Result<String, AppError> {
    let dir = app_data_dir(&app)?;
    run_blocking(move || services::ai_selection::prepare(&dir, input)).await
}

#[tauri::command]
pub async fn control_ai_selection_run(
    app: AppHandle,
    run_id: String,
    action: String,
    consent: bool,
) -> Result<String, AppError> {
    let dir = app_data_dir(&app)?;
    let emitter = app.clone();
    run_blocking(move || {
        services::ai_selection::control(&dir, &run_id, &action, consent, move |event| {
            let _ = emitter.emit("ai-selection-event", event);
        })
    })
    .await
}

#[tauri::command]
pub async fn append_ai_selection_results(
    app: AppHandle,
    input: services::ai_selection::AppendInput,
) -> Result<services::ai_selection::AppendResult, AppError> {
    let dir = app_data_dir(&app)?;
    run_blocking(move || services::ai_selection::append(&dir, input)).await
}

#[tauri::command]
pub fn health_check(app: AppHandle) -> Result<HealthReport, AppError> {
    services::health_check(&app)
}

fn app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|error| AppError::new("path_error", error.to_string()))
}

/// Long-running file work stays off the UI thread.
async fn run_blocking<T, F>(task: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| AppError::new("task_failed", error.to_string()))?
}

#[tauri::command]
pub async fn import_epub(
    app: AppHandle,
    source_path: String,
    allow_duplicate: bool,
    app_language: Option<String>,
) -> Result<ImportResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::import::import_epub_with_app_language(
            &data_dir,
            std::path::Path::new(&source_path),
            allow_duplicate,
            app_language.as_deref().unwrap_or("en"),
        )
    })
    .await
}

#[tauri::command]
pub async fn list_projects(app: AppHandle) -> Result<Vec<ProjectSummary>, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::projects::list_projects(&data_dir)).await
}

#[tauri::command]
pub async fn get_project(app: AppHandle, project_id: String) -> Result<ProjectSummary, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::projects::get_project(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn list_chapters(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<ChapterSummary>, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::projects::list_chapters(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn delete_project(app: AppHandle, project_id: String) -> Result<(), AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::projects::delete_project(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn get_project_cover(
    app: AppHandle,
    project_id: String,
) -> Result<Option<CoverImage>, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::projects::get_cover(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn get_chapter(
    app: AppHandle,
    project_id: String,
    chapter_id: String,
) -> Result<ChapterContent, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::workspace::get_chapter(&data_dir, &project_id, &chapter_id))
        .await
}

#[tauri::command]
pub async fn save_translation(
    app: AppHandle,
    block_id: String,
    target_text: Option<String>,
    status: String,
    expected_revision: i64,
) -> Result<SaveResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::workspace::save_translation(
            &data_dir,
            SaveInput {
                block_id,
                target_text,
                status,
                expected_revision,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn get_last_chapter(
    app: AppHandle,
    project_id: String,
) -> Result<Option<String>, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::workspace::get_last_chapter(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn update_project_metadata(
    app: AppHandle,
    project_id: String,
    translated_title: Option<String>,
    target_language: Option<String>,
    source_language: Option<String>,
) -> Result<(), AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::workspace::update_project_metadata(
            &data_dir,
            &project_id,
            translated_title,
            target_language,
            source_language,
        )
    })
    .await
}

#[tauri::command]
pub async fn review_export(app: AppHandle, project_id: String) -> Result<ExportReview, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::export::review_export(&data_dir, &project_id)).await
}

#[tauri::command]
pub async fn export_epub(
    app: AppHandle,
    project_id: String,
    destination: String,
    mode: String,
    title: Option<String>,
    language: Option<String>,
    include_original: Option<bool>,
) -> Result<ExportOutcome, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::export::export_epub(
            &data_dir,
            services::export::ExportRequest {
                project_id,
                destination: std::path::PathBuf::from(destination),
                mode,
                title,
                language,
                include_original: include_original.unwrap_or(true),
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn save_rich_translation(
    app: AppHandle,
    block_id: String,
    target_html: Option<String>,
    status: String,
    expected_revision: i64,
) -> Result<SaveResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::workspace::save_rich_translation(
            &data_dir,
            block_id,
            target_html,
            status,
            expected_revision,
        )
    })
    .await
}

#[tauri::command]
pub async fn save_chapter_document(
    app: AppHandle,
    chapter_id: String,
    html: Option<String>,
    status: String,
    expected_revision: i64,
) -> Result<SaveResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::workspace::save_chapter_document(
            &data_dir,
            &chapter_id,
            html,
            &status,
            expected_revision,
        )
    })
    .await
}

#[tauri::command]
pub async fn validate_chapter_html(html: String) -> Result<String, AppError> {
    run_blocking(move || crate::epub::rich_text::validate_document(&html).map(|(html, _)| html))
        .await
}

/// AI provider settings (settings-only slice): no book content is involved.
#[tauri::command]
pub async fn list_ai_providers(app: AppHandle) -> Result<AiProviderList, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::list_ai_providers(&data_dir)).await
}

#[tauri::command]
pub async fn check_ai_cli(app: AppHandle, provider_id: String) -> Result<CliCheckResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::check_ai_cli(&data_dir, &provider_id)).await
}

#[tauri::command]
pub async fn save_ai_custom_provider(
    app: AppHandle,
    protocol: Option<String>,
    id: Option<String>,
    name: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
) -> Result<CustomProviderSummary, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::ai::save_ai_custom_provider(
            &data_dir,
            services::ai::SaveCustomInput {
                protocol: protocol.unwrap_or_else(|| "openai".into()),
                id,
                name,
                base_url,
                api_key,
                model,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn delete_ai_custom_provider(app: AppHandle, id: String) -> Result<(), AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::delete_ai_custom_provider(&data_dir, &id)).await
}

#[tauri::command]
pub async fn test_ai_custom_provider(
    app: AppHandle,
    id: String,
) -> Result<CustomTestResult, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::test_ai_custom_provider(&data_dir, &id)).await
}

/// Translation model catalog + persisted selection (selection only; no
/// translation runs and no book content is involved).
#[tauri::command]
pub async fn list_translation_models(
    app: AppHandle,
    agent: String,
) -> Result<TranslationModelList, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::list_translation_models(&data_dir, &agent)).await
}

#[tauri::command]
pub async fn get_translation_selection(
    app: AppHandle,
) -> Result<Option<TranslationSelection>, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::ai::get_translation_selection(&data_dir)).await
}

#[tauri::command]
pub async fn set_translation_selection(
    app: AppHandle,
    agent: String,
    model: String,
    reasoning_effort: Option<String>,
) -> Result<TranslationSelection, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || {
        services::ai::set_translation_selection(
            &data_dir,
            &agent,
            &model,
            reasoning_effort.as_deref(),
        )
    })
    .await
}

/// AI translation proposals (assist slice). The run executes the user's chosen
/// local CLI with the chapter text after explicit consent; the saved draft is
/// never modified — the frontend inserts the proposal only on confirmation.
#[tauri::command]
pub async fn start_ai_translation(
    app: AppHandle,
    chapter_id: String,
    agent: String,
    model: String,
    consent: bool,
    previous_output: Option<String>,
    current_instructions: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AiTranslationStart, AppError> {
    let data_dir = app_data_dir(&app)?;
    let emitter = app.clone();
    run_blocking(move || {
        services::ai_translate::start_ai_translation(
            &data_dir,
            services::ai_translate::StartTranslationInput {
                chapter_id,
                agent,
                model,
                consent,
                previous_output,
                current_instructions,
                reasoning_effort,
            },
            move |event: AiTranslationEvent| {
                let _ = emitter.emit("ai-translation-event", event);
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn cancel_ai_translation(job_id: String) -> Result<(), AppError> {
    services::ai_translate::cancel_ai_translation(&job_id)
}

/// AI HTML repair. The run sends the user's current draft HTML to the chosen
/// local CLI after explicit consent; the saved draft is never modified — the
/// frontend fills the fixed HTML only on confirmation.
#[tauri::command]
pub async fn start_ai_html_fix(
    app: AppHandle,
    chapter_id: String,
    html: String,
    agent: String,
    model: String,
    consent: bool,
    reasoning_effort: Option<String>,
) -> Result<crate::domain::AiHtmlFixStart, AppError> {
    let data_dir = app_data_dir(&app)?;
    let emitter = app.clone();
    run_blocking(move || {
        services::ai_translate::start_ai_html_fix(
            &data_dir,
            services::ai_translate::StartHtmlFixInput {
                chapter_id,
                html,
                agent,
                model,
                consent,
                reasoning_effort,
            },
            move |event: crate::domain::AiHtmlFixEvent| {
                let _ = emitter.emit("ai-html-fix-event", event);
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn get_book_settings(
    app: AppHandle,
    project_id: String,
) -> Result<services::book_settings::BookSettings, AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::book_settings::get(&data_dir, &project_id)).await
}
#[tauri::command]
pub async fn save_book_settings(
    app: AppHandle,
    project_id: String,
    settings: services::book_settings::BookSettings,
) -> Result<(), AppError> {
    let data_dir = app_data_dir(&app)?;
    run_blocking(move || services::book_settings::save(&data_dir, &project_id, settings)).await
}

#[tauri::command]
pub async fn delete_ai_selection_run(
    app: AppHandle,
    chapter_id: String,
    run_id: String,
) -> Result<(), AppError> {
    let dir = app_data_dir(&app)?;
    run_blocking(move || services::ai_selection::delete_run(&dir, &chapter_id, &run_id)).await
}

/// Font discovery runs off the UI thread and returns family names only.
#[tauri::command]
pub async fn list_system_fonts() -> Result<Vec<String>, AppError> {
    run_blocking(|| Ok(services::fonts::list_system_fonts())).await
}

#[tauri::command]
pub fn start_content_server(
    app: AppHandle,
    server: tauri::State<'_, services::content_server::ContentServer>,
    config: services::content_server::Config,
) -> Result<services::content_server::Status, AppError> {
    server.start(app_data_dir(&app)?, config)
}
#[tauri::command]
pub fn stop_content_server(
    server: tauri::State<'_, services::content_server::ContentServer>,
) -> Result<services::content_server::Status, AppError> {
    server.stop()
}
#[tauri::command]
pub fn content_server_status(
    server: tauri::State<'_, services::content_server::ContentServer>,
) -> Result<services::content_server::Status, AppError> {
    server.status()
}

#[tauri::command]
pub async fn sharing_sign_in(
    app: AppHandle,
    state: tauri::State<'_, services::sharing::SharingSession>,
) -> Result<serde_json::Value, AppError> {
    services::sharing::sign_in(&app, &state).await
}

#[tauri::command]
pub async fn sharing_request(
    app: AppHandle,
    state: tauri::State<'_, services::sharing::SharingSession>,
    operation: services::sharing::Operation,
) -> Result<serde_json::Value, AppError> {
    services::sharing::request(app_data_dir(&app)?, &state, operation).await
}

#[tauri::command]
pub fn sharing_availability() -> services::sharing::Availability {
    services::sharing::availability()
}

#[tauri::command]
pub fn sharing_cancel_sign_in(state: tauri::State<'_, services::sharing::SharingSession>) {
    state.cancel_sign_in();
}
