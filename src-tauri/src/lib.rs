pub mod commands;
pub mod domain;
pub mod epub;
pub mod error;
pub mod security;
pub mod services;
pub mod storage;

use tauri::Manager;
use tauri::http::header::{CACHE_CONTROL, CONTENT_TYPE};

use crate::error::AppError;

/// Serves images/fonts/styles from a project's immutable snapshot to the
/// read-only preview pane via `epubres://<project-id>/<zip path>`. Only
/// resources inside app-owned snapshots can ever be served.
fn serve_epub_resource(
    app: &tauri::AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let respond = |status: u16, content_type: &'static str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header(CONTENT_TYPE, content_type)
            .header(CACHE_CONTROL, "no-store")
            .body(body)
            .unwrap_or_else(|_| {
                tauri::http::Response::builder()
                    .status(500)
                    .body(Vec::new())
                    .expect("static response")
            })
    };

    let path = epub::percent_decode_path(request.uri().path().trim_start_matches('/'));
    let mut segments = path.splitn(2, '/');
    let (Some(project_id), Some(resource)) = (segments.next(), segments.next()) else {
        return respond(404, "text/plain", b"Not found".to_vec());
    };
    if resource.is_empty() || crate::security::is_safe_entry_name(resource).is_err() {
        return respond(404, "text/plain", b"Not found".to_vec());
    }

    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(_) => return respond(500, "text/plain", b"Storage unavailable".to_vec()),
    };
    let result = (|| -> Result<(String, Vec<u8>), AppError> {
        let conn = services::workspace::open_db(&data_dir)?;
        let relative: String = conn.query_row(
            "SELECT source_snapshot_path FROM projects WHERE id = ?1",
            rusqlite::params![project_id],
            |row| row.get(0),
        )?;
        let mut snapshot_abs = data_dir.clone();
        for segment in relative.split('/') {
            crate::security::is_safe_entry_name(segment)?;
            snapshot_abs.push(segment);
        }
        let mut archive =
            epub::archive::SafeArchive::open(&snapshot_abs, &security::ArchiveLimits::default())?;
        let bytes = archive.read_entry(resource)?;
        Ok((media_type_for(resource), bytes))
    })();

    match result {
        Ok((content_type, bytes)) => {
            // SVG can carry scripts; refuse suspicious ones.
            if content_type == "image/svg+xml" {
                return respond(200, "image/svg+xml", sanitize_svg(&bytes));
            }
            respond(200, box_leak(content_type), bytes)
        }
        Err(_) => respond(404, "text/plain", b"Not found".to_vec()),
    }
}

fn sanitize_svg(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let lowered_scan =
        |haystack: &str, needle: &str| haystack.to_ascii_lowercase().contains(needle);
    if lowered_scan(&text, "<script")
        || lowered_scan(&text, "onload")
        || lowered_scan(&text, "onerror")
    {
        // Refuse suspicious SVGs rather than serve them.
        return Vec::new();
    }
    bytes.to_vec()
}

/// &'static str needed by the header builder.
fn box_leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn media_type_for(resource: &str) -> String {
    let extension = resource
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "css" => "text/css",
        "html" | "xhtml" => "application/xhtml+xml",
        "ncx" => "application/x-dtbncx+xml",
        "opf" => "text/xml",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(services::sharing::SharingSession::default())
        .manage(services::content_server::ContentServer::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("epubres", |ctx, request| {
            serve_epub_resource(ctx.app_handle(), request)
        })
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::sharing_sign_in,
            commands::sharing_cancel_sign_in,
            commands::sharing_request,
            commands::sharing_availability,
            commands::start_content_server,
            commands::stop_content_server,
            commands::content_server_status,
            commands::list_system_fonts,
            commands::import_epub,
            commands::list_projects,
            commands::get_project,
            commands::list_chapters,
            commands::get_project_cover,
            commands::delete_project,
            commands::get_chapter,
            commands::save_translation,
            commands::save_rich_translation,
            commands::save_chapter_document,
            commands::validate_chapter_html,
            commands::get_last_chapter,
            commands::update_project_metadata,
            commands::get_book_settings,
            commands::save_book_settings,
            commands::review_export,
            commands::export_epub,
            commands::list_ai_providers,
            commands::check_ai_cli,
            commands::save_ai_custom_provider,
            commands::delete_ai_custom_provider,
            commands::test_ai_custom_provider,
            commands::list_translation_models,
            commands::get_translation_selection,
            commands::set_translation_selection,
            commands::start_ai_translation,
            commands::start_ai_html_fix,
            commands::cancel_ai_translation,
            commands::get_ai_selection_state,
            commands::delete_ai_selection_run,
            commands::prepare_ai_selection_run,
            commands::control_ai_selection_run,
            commands::append_ai_selection_results,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
