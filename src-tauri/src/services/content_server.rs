//! Explicitly started LAN content API. Credentials live only in memory.
use crate::{error::AppError, storage::content_server as repository};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Config {
    pub port: u16,
    pub username: String,
    pub password: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub port: Option<u16>,
    pub authentication_required: bool,
    pub urls: Vec<String>,
}
struct Running {
    task: tauri::async_runtime::JoinHandle<()>,
    status: Status,
}
#[derive(Default)]
pub struct ContentServer(Mutex<Option<Running>>);
impl Drop for ContentServer {
    fn drop(&mut self) {
        if let Ok(slot) = self.0.get_mut() {
            if let Some(run) = slot.take() {
                run.task.abort();
            }
        }
    }
}
#[derive(Clone)]
struct Api {
    dir: PathBuf,
    authorization: Option<Arc<String>>,
    requests: Arc<tokio::sync::Semaphore>,
}

impl ContentServer {
    pub fn status(&self) -> Result<Status, AppError> {
        let slot = self.0.lock().map_err(|_| internal())?;
        Ok(slot.as_ref().map(|r| r.status.clone()).unwrap_or(Status {
            running: false,
            port: None,
            authentication_required: false,
            urls: Vec::new(),
        }))
    }
    pub fn start(&self, dir: PathBuf, config: Config) -> Result<Status, AppError> {
        if config.username.is_empty() != config.password.is_empty()
            || config.username.contains(':')
            || config.username.len() > 256
            || config.password.len() > 1024
            || config.username.chars().any(char::is_control)
            || config.password.chars().any(char::is_control)
        {
            return Err(AppError::new(
                "invalid_credentials",
                "Provide both username and password, or leave both empty. Username cannot contain a colon or control characters.",
            ));
        }
        let mut slot = self.0.lock().map_err(|_| internal())?;
        if slot.is_some() {
            return Err(AppError::new(
                "already_running",
                "Stop the content server before changing its configuration.",
            ));
        }
        super::workspace::open_db(&dir)?;
        let listener =
            std::net::TcpListener::bind(SocketAddr::new(IpAddr::from([0, 0, 0, 0]), config.port))
                .map_err(|_| {
                AppError::new(
                    "server_bind_failed",
                    "Cannot start content server on this port.",
                )
            })?;
        listener.set_nonblocking(true).map_err(|_| internal())?;
        let port = listener.local_addr().map_err(|_| internal())?.port();
        let authorization = if config.username.is_empty() {
            None
        } else {
            Some(Arc::new(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", config.username, config.password))
            )))
        };
        let status = Status {
            running: true,
            port: Some(port),
            authentication_required: authorization.is_some(),
            urls: server_urls(port),
        };
        let app = router(Api {
            dir,
            authorization,
            requests: Arc::new(tokio::sync::Semaphore::new(8)),
        });
        // Enter the Tauri runtime before constructing Tokio's listener.
        let task = tauri::async_runtime::spawn(async move {
            if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
                let _ = axum::serve(listener, app).await;
            }
        });
        *slot = Some(Running {
            task,
            status: status.clone(),
        });
        Ok(status)
    }
    pub fn stop(&self) -> Result<Status, AppError> {
        if let Some(run) = self.0.lock().map_err(|_| internal())?.take() {
            run.task.abort();
        }
        self.status()
    }
}
fn server_urls(port: u16) -> Vec<String> {
    let mut urls: Vec<_> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, address)| match address {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast() => {
                Some(format!("http://{ip}:{port}"))
            }
            _ => None,
        })
        .collect();
    urls.sort();
    urls.dedup();
    urls
}

fn internal() -> AppError {
    AppError::new("server_error", "Content server operation failed.")
}
struct ApiError(AppError);
impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "not_found" => StatusCode::NOT_FOUND,
            "identity_mismatch" | "revision_conflict" => StatusCode::CONFLICT,
            "storage_busy" => StatusCode::SERVICE_UNAVAILABLE,
            "storage_error" | "io_error" | "server_error" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        // Never expose internal DB errors, paths, or book fragments over HTTP.
        let message = if status.is_server_error() {
            "Library operation failed. Retry later."
        } else {
            "Request could not be applied. Check book, chapter, language, revision and HTML."
        };
        (
            status,
            Json(serde_json::json!({"code":self.0.code,"message":message})),
        )
            .into_response()
    }
}
fn router(api: Api) -> Router {
    Router::new()
        .route("/api/v1",get(||async {Json(serde_json::json!({"name":"Sagar","apiVersion":1,"translationPolicy":"fillMissing","htmlFormat":"chapter-body-fragment"}))}))
        .route("/api/v1/books",get(books))
        .route("/api/v1/books/{book}/epub",get(epub))
        .route("/api/v1/books/{book}/cover",get(cover))
        .route("/api/v1/books/{book}/chapters",get(chapters))
        .route("/api/v1/books/{book}/chapters/translated",get(translated_chapters))
        .route("/api/v1/books/{book}/chapters/{chapter}/translation",put(import))
        .layer(DefaultBodyLimit::max(8*1024*1024))
        .layer(middleware::from_fn_with_state(api.clone(),guard))
        .with_state(api)
}
async fn guard(State(api): State<Api>, request: Request, next: Next) -> Response {
    // Native clients only: browser origins are rejected even without credentials.
    let permit = api.requests.clone().try_acquire_owned();
    let mut response = if permit.is_err() {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"code":"server_busy"})),
        )
            .into_response()
    } else if request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site")
    {
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"code":"browser_not_allowed"})),
        )
            .into_response()
    } else if api.authorization.as_ref().is_some_and(|expected| {
        request
            .headers()
            .get(header::AUTHORIZATION)
            .map(|h| h.as_bytes())
            != Some(expected.as_bytes())
    }) {
        let mut r = (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"code":"unauthorized"})),
        )
            .into_response();
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Basic realm=\"Sagar\", charset=\"UTF-8\""),
        );
        r
    } else {
        match tokio::time::timeout(std::time::Duration::from_secs(30), next.run(request)).await {
            Ok(response) => response,
            Err(_) => (
                StatusCode::REQUEST_TIMEOUT,
                Json(serde_json::json!({"code":"request_timeout"})),
            )
                .into_response(),
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        header::HeaderValue::from_static("nosniff"),
    );
    response
}
async fn blocking<T: Send + 'static>(
    op: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|_| ApiError(internal()))?
        .map_err(Into::into)
}
async fn books(State(api): State<Api>) -> Result<Json<Vec<repository::Book>>, ApiError> {
    blocking(move || repository::books(&super::workspace::open_db(&api.dir)?))
        .await
        .map(Json)
}
async fn chapters(
    State(api): State<Api>,
    Path(book): Path<String>,
) -> Result<Json<Vec<repository::Chapter>>, ApiError> {
    blocking(move || repository::chapters(&super::workspace::open_db(&api.dir)?, &book))
        .await
        .map(Json)
}
async fn translated_chapters(
    State(api): State<Api>,
    Path(book): Path<String>,
) -> Result<Json<repository::TranslatedChapters>, ApiError> {
    blocking(move || repository::translated_chapters(&super::workspace::open_db(&api.dir)?, &book))
        .await
        .map(Json)
}

async fn import(
    State(api): State<Api>,
    Path((book, chapter)): Path<(String, String)>,
    Json(input): Json<repository::Incoming>,
) -> Result<Json<serde_json::Value>, ApiError> {
    blocking(move || {
        repository::import(
            &mut super::workspace::open_db(&api.dir)?,
            &book,
            &chapter,
            input,
        )
    })
    .await
    .map(Json)
}
/// Reuse the library's custom-cover precedence, archive bounds and OPF lookup.
async fn cover(State(api): State<Api>, Path(book): Path<String>) -> Result<Response, ApiError> {
    let image = blocking(move || {
        repository::book(&super::workspace::open_db(&api.dir)?, &book)?;
        super::projects::get_cover(&api.dir, &book)?
            .ok_or_else(|| AppError::new("not_found", "Book has no available cover."))
    })
    .await?;
    if !matches!(
        image.media_type.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/svg+xml"
    ) {
        return Err(AppError::new("not_found", "Book has no supported cover.").into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image.base64)
        .map_err(|_| ApiError(internal()))?;
    let mut response = ([(header::CONTENT_TYPE, image.media_type)], bytes).into_response();
    // SVGs are untrusted publication content, even when opened directly.
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static("sandbox; default-src 'none'; style-src 'unsafe-inline'"),
    );
    Ok(response)
}

async fn epub(State(api): State<Api>, Path(book): Path<String>) -> Result<Response, ApiError> {
    let path = blocking(move || {
        repository::book(&super::workspace::open_db(&api.dir)?, &book)?;
        super::workspace::snapshot_path(&api.dir, &book)
    })
    .await?;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| ApiError(internal()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/epub+zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"book.epub\"",
            ),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        documents,
        projects::{NewChapter, NewProject, create_project},
    };
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = super::super::workspace::open_db(dir.path()).unwrap();
        create_project(
            &mut conn,
            &NewProject {
                id: "book".into(),
                source_sha256: "a".repeat(64),
                original_source_path: None,
                source_snapshot_path: "projects/book/source.epub".into(),
                title: "Book A".into(),
                author: Some("XXX".into()),
                source_language: Some("en".into()),
                target_language: "my".into(),
                created_at: "now".into(),
            },
            &[(
                NewChapter {
                    id: "chapter".into(),
                    spine_index: 0,
                    manifest_item_id: "c".into(),
                    resource_href: "Text/c.xhtml".into(),
                    title: "Chapter".into(),
                },
                vec![],
            )],
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("projects/book")).unwrap();
        std::fs::write(
            dir.path().join("projects/book/source.epub"),
            b"immutable-test-snapshot",
        )
        .unwrap();
        dir
    }
    fn payload() -> serde_json::Value {
        serde_json::json!({"sourceSha256":"a".repeat(64),"targetLanguage":"my","href":"Text/c.xhtml","html":"<p>မြန်မာစာ <em>စာ</em></p>","expectedRevision":0})
    }
    #[test]
    fn http_auth_download_fill_only_and_lifecycle() {
        let dir = fixture();
        let server = ContentServer::default();
        assert!(!server.status().unwrap().running);
        assert!(
            server
                .start(
                    dir.path().into(),
                    Config {
                        username: "u".into(),
                        ..Config::default()
                    }
                )
                .is_err()
        );
        let status = server
            .start(
                dir.path().into(),
                Config {
                    username: "user".into(),
                    password: "pass".into(),
                    ..Config::default()
                },
            )
            .unwrap();
        assert!(server.start(dir.path().into(), Config::default()).is_err());
        for url in &status.urls {
            let url = reqwest::Url::parse(url).unwrap();
            assert_eq!(url.scheme(), "http");
            assert_eq!(url.port(), status.port);
            let address: std::net::Ipv4Addr = url.host_str().unwrap().parse().unwrap();
            assert!(!address.is_loopback() && !address.is_unspecified());
        }
        let base = format!("http://127.0.0.1:{}/api/v1", status.port.unwrap());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        for suffix in [
            "",
            "/books",
            "/books/book/epub",
            "/books/book/cover",
            "/books/book/chapters",
            "/books/book/chapters/translated",
        ] {
            assert_eq!(
                client
                    .get(format!("{base}{suffix}"))
                    .send()
                    .unwrap()
                    .status(),
                401
            );
        }
        let request = |method, path: &str| {
            client
                .request(method, format!("{base}{path}"))
                .basic_auth("user", Some("pass"))
        };
        assert_eq!(
            request(reqwest::Method::GET, "/books")
                .header("Origin", "http://evil.invalid")
                .send()
                .unwrap()
                .status(),
            403
        );
        let books: serde_json::Value = request(reqwest::Method::GET, "/books")
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(books[0]["sourceSha256"], "a".repeat(64));
        assert!(books[0].get("sourceSnapshotPath").is_none());
        assert_eq!(
            request(reqwest::Method::GET, "/books/book/epub")
                .send()
                .unwrap()
                .bytes()
                .unwrap(),
            &b"immutable-test-snapshot"[..]
        );
        assert_eq!(
            request(reqwest::Method::GET, "/books/missing/chapters")
                .send()
                .unwrap()
                .status(),
            404
        );
        let path = "/books/book/chapters/chapter/translation";
        assert_eq!(
            client
                .put(format!("{base}{path}"))
                .json(&payload())
                .send()
                .unwrap()
                .status(),
            401
        );
        let mut oversized = payload();
        oversized["html"] = "x".repeat(8 * 1024 * 1024).into();
        assert_eq!(
            request(reqwest::Method::PUT, path)
                .json(&oversized)
                .send()
                .unwrap()
                .status(),
            413
        );

        let mut wrong = payload();
        wrong["targetLanguage"] = "fr".into();
        assert_eq!(
            request(reqwest::Method::PUT, path)
                .json(&wrong)
                .send()
                .unwrap()
                .status(),
            409
        );
        let mut stale = payload();
        stale["expectedRevision"] = 7.into();
        assert_eq!(
            request(reqwest::Method::PUT, path)
                .json(&stale)
                .send()
                .unwrap()
                .status(),
            409
        );
        let mut active = payload();
        active["html"] = "<script>alert(1)</script>".into();
        assert_eq!(
            request(reqwest::Method::PUT, path)
                .json(&active)
                .send()
                .unwrap()
                .status(),
            422
        );
        let mut empty = payload();
        empty["html"] = "<p></p>".into();
        assert_eq!(
            request(reqwest::Method::PUT, path)
                .json(&empty)
                .send()
                .unwrap()
                .status(),
            422
        );
        let result: serde_json::Value = request(reqwest::Method::PUT, path)
            .json(&payload())
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(result["outcome"], "imported");
        let result: serde_json::Value = request(reqwest::Method::PUT, path)
            .json(&payload())
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(result["outcome"], "skippedExisting");
        let chapters: serde_json::Value = request(reqwest::Method::GET, "/books/book/chapters")
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert!(
            chapters[0]["document"]["html"]
                .as_str()
                .unwrap()
                .contains("မြန်မာစာ")
        );
        assert_eq!(chapters[0]["document"]["revision"], 1);
        assert_eq!(chapters[0]["document"]["status"], "draft");
        assert!(!server.stop().unwrap().running);
        assert!(!server.stop().unwrap().running);
        let status = server.start(dir.path().into(), Config::default()).unwrap();
        assert!(!status.authentication_required);
        assert_eq!(
            client
                .get(format!(
                    "http://127.0.0.1:{}/api/v1/books",
                    status.port.unwrap()
                ))
                .send()
                .unwrap()
                .status(),
            200
        );
        server.stop().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("projects/book/source.epub")).unwrap(),
            b"immutable-test-snapshot"
        );
    }
    #[test]
    fn translated_inventory_is_ordered_lightweight_and_matches_document_precedence() {
        let dir = fixture();
        let mut conn = super::super::workspace::open_db(dir.path()).unwrap();
        for (id, index) in [
            ("last", 8),
            ("reviewed", 2),
            ("legacy", 3),
            ("skipped", 4),
            ("cleared", 5),
            ("pending", 6),
        ] {
            conn.execute("INSERT INTO chapters(id,project_id,spine_index,manifest_item_id,resource_href,title) VALUES(?1,'book',?2,?1,?1,?1)",rusqlite::params![id,index]).unwrap();
        }
        documents::save(
            &mut conn,
            "chapter",
            Some("<p>မြန်မာစာ</p>".into()),
            "draft",
            0,
        )
        .unwrap();
        documents::save(&mut conn, "last", Some("<p>Last</p>".into()), "draft", 0).unwrap();
        documents::save(
            &mut conn,
            "reviewed",
            Some("<p>Reviewed</p>".into()),
            "reviewed",
            0,
        )
        .unwrap();
        documents::save(&mut conn, "skipped", None, "skipped", 0).unwrap();
        documents::save(&mut conn, "cleared", None, "draft", 0).unwrap();
        for chapter in ["legacy", "cleared"] {
            conn.execute("INSERT INTO blocks(id,chapter_id,block_index,structural_locator,element_name,source_xhtml_fragment,source_plain_text) VALUES(?1,?1,0,'0','p','Source','Source')",[chapter]).unwrap();
            conn.execute("INSERT INTO translations(block_id,target_xhtml_fragment,target_plain_text,status) VALUES(?1,'Translated','Translated','draft')",[chapter]).unwrap();
        }
        conn.execute("INSERT INTO projects(id,source_sha256,source_snapshot_path,title,created_at,updated_at) VALUES('other','other','projects/other/source.epub','Other','now','now')",[]).unwrap();
        conn.execute("INSERT INTO chapters(id,project_id,spine_index,manifest_item_id,resource_href,title) VALUES('other-chapter','other',0,'c','c.xhtml','Other')",[]).unwrap();
        documents::save(
            &mut conn,
            "other-chapter",
            Some("<p>Other translation</p>".into()),
            "draft",
            0,
        )
        .unwrap();
        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapter_documents", [], |r| r.get(0))
            .unwrap();
        let expected = vec!["chapter", "reviewed", "legacy", "last"];
        assert_eq!(
            repository::translated_chapters(&conn, "book")
                .unwrap()
                .chapter_ids,
            expected
        );
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapter_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after, "inventory must not create document rows");
        let server = ContentServer::default();
        let status = server.start(dir.path().into(), Config::default()).unwrap();
        let base = format!("http://127.0.0.1:{}/api/v1/books", status.port.unwrap());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let response = client
            .get(format!("{base}/book/chapters/translated"))
            .send()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let value: serde_json::Value = response.json().unwrap();
        assert_eq!(value, serde_json::json!({"chapterIds":expected}));
        assert_eq!(
            client
                .get(format!("{base}/missing/chapters/translated"))
                .send()
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            client
                .get(format!("{base}/book/chapters/transalted"))
                .send()
                .unwrap()
                .status(),
            404
        );
        // GET chapters materializes legacy documents. The inventory stays consistent.
        let full = repository::chapters(&conn, "book").unwrap();
        let with_html: Vec<_> = full
            .iter()
            .filter(|c| {
                c.document
                    .html
                    .as_ref()
                    .is_some_and(|h| !h.trim().is_empty())
            })
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(with_html, expected);
        assert_eq!(
            repository::translated_chapters(&conn, "book")
                .unwrap()
                .chapter_ids,
            expected
        );
        conn.execute("UPDATE chapter_documents SET html=NULL", [])
            .unwrap();
        assert!(
            repository::translated_chapters(&conn, "book")
                .unwrap()
                .chapter_ids
                .is_empty()
        );
        let value: serde_json::Value = client
            .get(format!("{base}/book/chapters/translated"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(value, serde_json::json!({"chapterIds":[]}));
        server.stop().unwrap();
    }

    #[test]
    fn covers_include_original_custom_and_missing_with_metadata() {
        use std::io::Write;
        let dir = fixture();
        let snapshot = dir.path().join("projects/book/source.epub");
        let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jvJkAAAAASUVORK5CYII=").unwrap();
        let write_snapshot = |cover: Option<(&str, &[u8])>| {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&snapshot).unwrap());
            let container = br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
            let item = cover.map(|(mime,_)|format!("<item id=\"cover\" href=\"cover-image\" media-type=\"{mime}\" properties=\"cover-image\"/>")).unwrap_or_default();
            let opf = format!(
                r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">test</dc:identifier><dc:title>Book A</dc:title><dc:language>en</dc:language></metadata><manifest>{item}<item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#
            );
            for (name,bytes) in [("mimetype",b"application/epub+zip".as_slice()),("META-INF/container.xml",container.as_slice()),("EPUB/package.opf",opf.as_bytes()),("EPUB/c.xhtml",b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Chapter</title></head><body><p>Text</p></body></html>".as_slice())] {
                zip.start_file(name,zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();zip.write_all(bytes).unwrap();
            }
            if let Some((_, bytes)) = cover {
                zip.start_file("EPUB/cover-image", zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        };
        write_snapshot(Some(("image/png", &png)));
        let server = ContentServer::default();
        let status = server.start(dir.path().into(), Config::default()).unwrap();
        let base = format!("http://127.0.0.1:{}", status.port.unwrap());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let books: serde_json::Value = client
            .get(format!("{base}/api/v1/books"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(books[0]["title"], "Book A");
        assert_eq!(books[0]["author"], "XXX");
        assert_eq!(books[0]["coverUrl"], "/api/v1/books/book/cover");
        let url = format!("{base}{}", books[0]["coverUrl"].as_str().unwrap());
        let before = std::fs::read(&snapshot).unwrap();
        let response = client.get(&url).send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.bytes().unwrap().as_ref(), png.as_slice());
        assert_eq!(std::fs::read(&snapshot).unwrap(), before);
        write_snapshot(None);
        assert_eq!(client.get(&url).send().unwrap().status(), 404);
        let conn = super::super::workspace::open_db(dir.path()).unwrap();
        // A custom cover takes precedence even when an original is present.
        write_snapshot(Some((
            "image/svg+xml",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )));
        let custom = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        conn.execute(
            "UPDATE projects SET custom_cover=?1 WHERE id='book'",
            [custom],
        )
        .unwrap();
        assert_eq!(
            client.get(&url).send().unwrap().bytes().unwrap().as_ref(),
            png.as_slice()
        );
        conn.execute("UPDATE projects SET custom_cover=NULL WHERE id='book'", [])
            .unwrap();
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        write_snapshot(Some(("image/svg+xml", svg)));
        let response = client.get(&url).send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert!(
            response.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains("sandbox; default-src 'none'")
        );
        write_snapshot(Some(("text/html", b"<script>alert(1)</script>")));
        assert_eq!(client.get(&url).send().unwrap().status(), 404);
        assert_eq!(
            client
                .get(format!("{base}/api/v1/books/missing/cover"))
                .send()
                .unwrap()
                .status(),
            404
        );
        server.stop().unwrap();
    }

    #[test]
    fn keep_original_and_concurrent_imports_are_protected() {
        let dir = fixture();
        let mut conn = super::super::workspace::open_db(dir.path()).unwrap();
        documents::save(&mut conn, "chapter", None, "skipped", 0).unwrap();
        let input = serde_json::from_value(payload()).unwrap();
        assert_eq!(
            repository::import(&mut conn, "book", "chapter", input).unwrap()["outcome"],
            "skippedExisting"
        );
        documents::save(&mut conn, "chapter", None, "draft", 1).unwrap();
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let dir = dir.path().to_owned();
                std::thread::spawn(move || {
                    let mut conn = super::super::workspace::open_db(&dir).unwrap();
                    let mut value = payload();
                    value["expectedRevision"] = 2.into();
                    repository::import(
                        &mut conn,
                        "book",
                        "chapter",
                        serde_json::from_value(value).unwrap(),
                    )
                    .unwrap()["outcome"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
            })
            .collect();
        let mut outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        outcomes.sort();
        assert_eq!(outcomes, ["imported", "skippedExisting"]);
    }
}
