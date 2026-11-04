//! Read-only browser reader served at the content server root.
//!
//! Separate from the native `/api/v1` routes: these accept same-origin browser
//! requests, expose GET routes only, and never write to the library. Book HTML is
//! sanitized server-side with the desktop preview allowlist, and the page CSP
//! blocks scripts and network access from book content.
use super::{Api, ApiError, authorized, blocking, unauthorized};
use crate::{
    epub::{self, archive::SafeArchive},
    error::AppError,
    security::ArchiveLimits,
    storage::{content_server as repository, documents},
};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rusqlite::OptionalExtension;
use serde::Serialize;

const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub(super) fn routes(api: Api) -> Router {
    Router::new()
        .route("/", get(shell))
        .route("/assets/reader.js", get(script))
        .route("/assets/reader.css", get(styles))
        .route("/reader/api/books", get(books))
        .route("/reader/api/books/{book}", get(book))
        .route("/reader/api/books/{book}/cover", get(super::cover))
        .route("/reader/api/books/{book}/chapters/{chapter}", get(chapter))
        .route("/reader/api/books/{book}/resources/{*path}", get(resource))
        .layer(middleware::from_fn_with_state(api.clone(), guard))
        .with_state(api)
}

async fn shell() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("web_reader/index.html"),
    )
}
async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("web_reader/reader.js"),
    )
}
async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("web_reader/reader.css"),
    )
}

/// Accept IP literals, localhost and local-network names only. A public DNS
/// name pointing at a LAN address (DNS rebinding) must not read the library.
fn trusted_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        return rest.split_once(']').is_some();
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    }
    .trim_end_matches('.')
    .to_ascii_lowercase();
    !name.is_empty()
        && (name.parse::<std::net::IpAddr>().is_ok()
            || !name.contains('.')
            || [".local", ".lan", ".home.arpa", ".internal"]
                .iter()
                .any(|suffix| name.ends_with(suffix)))
}

fn cross_site(headers: &HeaderMap, host: &str) -> bool {
    // Check every value so a duplicated header cannot hide a cross-site one.
    let expected_origin = format!("http://{host}");
    headers
        .get_all("sec-fetch-site")
        .iter()
        .any(|s| s != "same-origin" && s != "none")
        || headers
            .get_all(header::ORIGIN)
            .iter()
            .any(|origin| origin.as_bytes() != expected_origin.as_bytes())
        || headers.get_all(header::HOST).iter().count() > 1
}

async fn guard(State(api): State<Api>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let reject =
        |status, code: &str| (status, Json(serde_json::json!({ "code": code }))).into_response();
    let mut response = if !trusted_host(&host) {
        reject(StatusCode::FORBIDDEN, "host_not_allowed")
    } else if cross_site(request.headers(), &host) {
        reject(StatusCode::FORBIDDEN, "cross_site_not_allowed")
    } else if !authorized(&api, request.headers()) {
        unauthorized()
    } else {
        // A chapter page loads several images at once, so wait briefly for a slot.
        let wait = std::time::Duration::from_secs(10);
        match tokio::time::timeout(wait, api.web_requests.clone().acquire_owned()).await {
            Ok(Ok(_permit)) => {
                match tokio::time::timeout(std::time::Duration::from_secs(30), next.run(request))
                    .await
                {
                    Ok(response) => response,
                    Err(_) => reject(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
                }
            }
            _ => reject(StatusCode::SERVICE_UNAVAILABLE, "server_busy"),
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(PAGE_CSP));
    response
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebBook {
    id: String,
    title: String,
    author: Option<String>,
    source_language: Option<String>,
    target_language: String,
    cover_url: String,
    chapter_count: i64,
    translated_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebChapter {
    id: String,
    title: String,
    href: String,
    translated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebBookDetail {
    book: WebBook,
    chapters: Vec<WebChapter>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChapterView {
    id: String,
    title: String,
    href: String,
    status: String,
    original_html: String,
    translated_html: Option<String>,
}

/// Encodes an ID as a single URL path segment.
fn segment(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn web_book(conn: &rusqlite::Connection, book: repository::Book) -> Result<WebBook, AppError> {
    let chapter_count = conn.query_row(
        "SELECT COUNT(*) FROM chapters WHERE project_id=?1",
        [&book.id],
        |r| r.get(0),
    )?;
    let translated_count = repository::translated_chapters(conn, &book.id)?
        .chapter_ids
        .len();
    Ok(WebBook {
        cover_url: format!("/reader/api/books/{}/cover", segment(&book.id)),
        id: book.id,
        title: book.title,
        author: book.author,
        source_language: book.source_language,
        target_language: book.target_language,
        chapter_count,
        translated_count,
    })
}

#[derive(Default, serde::Deserialize)]
struct BookQuery {
    q: Option<String>,
}

async fn books(
    State(api): State<Api>,
    Query(query): Query<BookQuery>,
) -> Result<Json<Vec<WebBook>>, ApiError> {
    blocking(move || {
        let conn = super::super::workspace::open_db(&api.dir)?;
        repository::search_books(&conn, query.q.as_deref())?
            .into_iter()
            .map(|book| web_book(&conn, book))
            .collect()
    })
    .await
    .map(Json)
}

async fn book(
    State(api): State<Api>,
    Path(id): Path<String>,
) -> Result<Json<WebBookDetail>, ApiError> {
    blocking(move || {
        let conn = super::super::workspace::open_db(&api.dir)?;
        let book = web_book(&conn, repository::book(&conn, &id)?)?;
        let translated: std::collections::HashSet<String> =
            repository::translated_chapters(&conn, &id)?
                .chapter_ids
                .into_iter()
                .collect();
        let mut stmt = conn.prepare(
            "SELECT id,title,resource_href FROM chapters WHERE project_id=?1 ORDER BY spine_index,id",
        )?;
        let chapters = stmt
            .query_map([&id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (id, title, href) = row?;
                Ok(WebChapter {
                    translated: translated.contains(&id),
                    id,
                    title,
                    href,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        Ok(WebBookDetail { book, chapters })
    })
    .await
    .map(Json)
}

fn chapter_view(dir: &std::path::Path, book: &str, chapter: &str) -> Result<ChapterView, AppError> {
    let conn = super::super::workspace::open_db(dir)?;
    repository::book(&conn, book)?;
    let (href, title): (String, String) = conn
        .query_row(
            "SELECT resource_href,title FROM chapters WHERE project_id=?1 AND id=?2",
            [book, chapter],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::new("not_found", "Chapter not found."))?;
    let (html, status) = documents::peek(&conn, chapter)?;
    drop(conn);
    let prefix = format!("/reader/api/books/{}/resources", segment(book));
    let mut archive = SafeArchive::open(
        &super::super::workspace::snapshot_path(dir, book)?,
        &ArchiveLimits::default(),
    )?;
    let source = String::from_utf8(archive.read_entry(&href)?)
        .map_err(|_| AppError::new("invalid_epub", "The chapter is not UTF-8 XHTML."))?;
    let doc = epub::xml::parse(&source)
        .map_err(|_| AppError::new("invalid_epub", "The chapter XHTML could not be read."))?;
    let original_html = doc
        .descendants()
        .find(|n| n.has_tag_name(("http://www.w3.org/1999/xhtml", "body")))
        .map(|body| epub::preview::sanitize_fragment(&source[body.range()], "body", &href, &prefix))
        .unwrap_or_default();
    let translated_html = html
        .filter(|h| !h.trim().is_empty())
        .map(|h| epub::preview::sanitize_fragment(&h, "body", &href, &prefix));
    Ok(ChapterView {
        id: chapter.to_string(),
        title,
        href,
        status,
        original_html,
        translated_html,
    })
}

async fn chapter(
    State(api): State<Api>,
    Path((book, chapter)): Path<(String, String)>,
) -> Result<Json<ChapterView>, ApiError> {
    blocking(move || chapter_view(&api.dir, &book, &chapter))
        .await
        .map(Json)
}

/// Serves passive images only. Stylesheets, fonts and documents stay private.
async fn resource(
    State(api): State<Api>,
    Path((book, path)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let not_found = || AppError::new("not_found", "Resource not found.");
    let (mime, bytes) = blocking(move || {
        repository::book(&super::super::workspace::open_db(&api.dir)?, &book)?;
        crate::security::is_safe_entry_name(&path).map_err(|_| not_found())?;
        let mut archive = SafeArchive::open(
            &super::super::workspace::snapshot_path(&api.dir, &book)?,
            &ArchiveLimits::default(),
        )?;
        let bytes = archive.read_entry(&path).map_err(|_| not_found())?;
        let mime = epub::rich_text::image_mime_from_bytes(&bytes)
            .or_else(|| {
                path.to_ascii_lowercase()
                    .ends_with(".svg")
                    .then_some("image/svg+xml")
            })
            .ok_or_else(not_found)?;
        Ok((mime, bytes))
    })
    .await?;
    let mut response = ([(header::CONTENT_TYPE, mime)], bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; style-src 'unsafe-inline'"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_allowlist_rejects_public_names() {
        for ok in [
            "192.168.1.20:8080",
            "127.0.0.1",
            "[::1]:8080",
            "localhost:8080",
            "desktop:8080",
            "mac.local:8080",
            "pc.lan",
        ] {
            assert!(trusted_host(ok), "{ok}");
        }
        for bad in ["", "evil.example.com:8080", "attacker.com", ":8080"] {
            assert!(!trusted_host(bad), "{bad}");
        }
    }
}
