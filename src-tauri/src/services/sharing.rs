//! SagarNet OAuth and opt-in sharing. Source files and credentials stay native.

use std::path::Path;
use std::time::{Duration, Instant};

use axum::{Router, extract::Query, response::IntoResponse, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::epub::{archive::SafeArchive, container, opf, rich_text};
use crate::error::AppError;
use crate::security::ArchiveLimits;
use crate::services::{projects, workspace};

const SCOPES: &str = "sharing:read sharing:write";
const CALLBACK_PATH: &str = "/oauth/callback";
/// Fixed loopback port. Register this exact callback in SagarNet.
const CALLBACK_PORT: u16 = 53142;
const HTML_FORMAT: u64 = 2;

#[derive(Default)]
pub struct SharingSession {
    account: Mutex<Option<Session>>,
    pending_sign_in: std::sync::Mutex<Option<CancellationToken>>,
}

impl SharingSession {
    pub fn cancel_sign_in(&self) {
        if let Ok(mut pending) = self.pending_sign_in.lock() {
            if let Some(cancellation) = pending.take() {
                cancellation.cancel();
            }
        }
    }
}

struct Session {
    account_key: String,
    access_token: String,
    expires: Instant,
}

/// SagarNet connection details. They come from `src-tauri/sagarnet.json`,
/// and the `SAGARNET_URL` / `SAGARNET_CLIENT_ID` build-time environment
/// variables override them. Users never type these values.
pub struct Settings {
    server: String,
    client_id: String,
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    server: String,
    #[serde(default)]
    dev_server: String,
    #[serde(default)]
    client_id: String,
}

impl Settings {
    pub fn configured() -> Self {
        Self::from_config(
            include_str!("../../sagarnet.json"),
            option_env!("SAGARNET_URL"),
            option_env!("SAGARNET_CLIENT_ID"),
            cfg!(debug_assertions),
        )
    }

    fn from_config(
        file: &str,
        server: Option<&str>,
        client_id: Option<&str>,
        development: bool,
    ) -> Self {
        let file: ConfigFile = serde_json::from_str(file).unwrap_or_default();
        let pick = |over: Option<&str>, value: String| {
            over.map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| value.trim().to_owned())
        };
        let mut server = pick(server, file.server);
        if server.is_empty() && development {
            server = file.dev_server.trim().to_owned();
        }
        Self {
            server,
            client_id: pick(client_id, file.client_id),
        }
    }
}

#[derive(Serialize)]
pub struct Availability {
    available: bool,
    host: Option<String>,
}

/// Tells the UI whether this build can reach SagarNet, and which host it uses.
pub fn availability() -> Availability {
    let settings = Settings::configured();
    match settings.origin() {
        Ok(origin) if !settings.client_id.is_empty() => Availability {
            available: true,
            host: origin.host_str().map(|host| match origin.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            }),
        },
        _ => Availability {
            available: false,
            host: None,
        },
    }
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    Account,
    Languages,
    Disconnect,
    Match {
        project: String,
    },
    Search {
        query: String,
        page: u32,
    },
    Book {
        book: u64,
    },
    Chapters {
        book: u64,
        edition: u64,
        page: u32,
    },
    Versions {
        chapter: u64,
        target_language: String,
        page: u32,
    },
    Translation {
        translation: u64,
    },
    Mine {
        chapter: u64,
        target_language: String,
    },
    Link {
        book: u64,
    },
    PublishBook {
        book: u64,
    },
    Create {
        project: String,
        title: String,
        author: String,
        description: String,
        public: bool,
    },
    LocalPreview {
        project: String,
        local_chapter: String,
    },
    Save {
        project: String,
        local_chapter: String,
        chapter: u64,
        public: bool,
        existing: Option<Revision>,
        local_revision: i64,
        target_language: String,
    },
    Delete {
        chapter: u64,
        existing: Revision,
    },
    Cover {
        book: u64,
    },
}

#[derive(Deserialize, Serialize)]
pub struct Revision {
    translation_id: u64,
    revision: u64,
}

#[derive(Serialize)]
struct Fingerprint {
    title: String,
    author: Option<String>,
    fingerprint_version: u8,
    source_fingerprint: String,
    xhtml_spine_count: usize,
    chapters: Vec<ManifestChapter>,
}

#[derive(Serialize)]
struct ManifestChapter {
    spine_position: i64,
    label: String,
    source_fingerprint: String,
}

fn failure(message: &'static str) -> AppError {
    AppError::new("sharing_error", message)
}

fn credential_error(_: keyring::Error) -> AppError {
    failure("Sagar could not access your operating system’s credential store.")
}

impl Settings {
    fn origin(&self) -> Result<Url, AppError> {
        let unavailable = || failure("SagarNet sharing is not set up in this version of Sagar.");
        if self.server.is_empty() {
            return Err(unavailable());
        }
        let url = Url::parse(&self.server).map_err(|_| unavailable())?;
        let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || (!self.client_id.is_empty() && Uuid::parse_str(&self.client_id).is_err())
        {
            return Err(unavailable());
        }
        Ok(url)
    }

    fn account_key(&self, origin: &Url) -> String {
        format!("{}:{}", origin.as_str(), self.client_id)
    }
}

fn client() -> Result<Client, AppError> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| failure("Could not start the SagarNet connection."))
}

fn endpoint(origin: &Url, path: &str) -> Result<Url, AppError> {
    origin
        .join(path)
        .map_err(|_| failure("Invalid SagarNet endpoint."))
}

async fn checked(request: RequestBuilder) -> Result<Response, AppError> {
    let response = request
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            failure("Could not reach SagarNet. Check the server address and connection.")
        })?;
    match response.status() {
        status if status.is_success() => Ok(response),
        StatusCode::UNAUTHORIZED => Err(AppError::new(
            "sharing_unauthorized",
            "Sign in to SagarNet again.",
        )),
        StatusCode::FORBIDDEN => Err(failure(
            "Your account does not have permission for this action.",
        )),
        StatusCode::NOT_FOUND => Err(failure("This item is unavailable on SagarNet.")),
        StatusCode::CONFLICT => Err(AppError::new(
            "sharing_conflict",
            "The shared item changed or cannot be published. Review it before trying again.",
        )),
        StatusCode::UNPROCESSABLE_ENTITY => Err(failure(
            "SagarNet rejected the submitted book or translation. Check the data and size limits.",
        )),
        StatusCode::TOO_MANY_REQUESTS => {
            Err(failure("Too many requests. Wait a moment and try again."))
        }
        _ => Err(failure("SagarNet could not complete this request.")),
    }
}

async fn token_request(
    client: &Client,
    origin: &Url,
    form: &[(&str, &str)],
) -> Result<Tokens, AppError> {
    let response = client
        .post(endpoint(origin, "/oauth/token")?)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|_| failure("Could not reach SagarNet to sign in."))?;
    if !response.status().is_success() {
        return if response.status().is_client_error()
            && response.status() != StatusCode::TOO_MANY_REQUESTS
        {
            Err(AppError::new(
                "sharing_unauthorized",
                "This authorization expired or was revoked. Sign in again.",
            ))
        } else {
            Err(failure(
                "SagarNet could not complete sign-in. Try again in a moment.",
            ))
        };
    }

    response
        .json()
        .await
        .map_err(|_| failure("SagarNet returned an invalid token response."))
}

async fn credential(key: String) -> Result<Option<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        match keyring::Entry::new("SagarNet", &key)
            .map_err(credential_error)?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(credential_error(error)),
        }
    })
    .await
    .map_err(|_| failure("Could not read the credential store."))?
}

async fn write_credential(key: String, token: Option<String>) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let entry = keyring::Entry::new("SagarNet", &key).map_err(credential_error)?;
        if let Some(token) = token {
            return entry.set_password(&token).map_err(credential_error);
        }
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(credential_error(error)),
        }
    })
    .await
    .map_err(|_| failure("Could not update the credential store."))?
}

async fn store(
    session: &mut Option<Session>,
    key: String,
    tokens: Tokens,
) -> Result<String, AppError> {
    write_credential(key.clone(), Some(tokens.refresh_token)).await?;
    let expires =
        Instant::now() + Duration::from_secs(tokens.expires_in.min(86400).saturating_sub(20));
    *session = Some(Session {
        account_key: key,
        access_token: tokens.access_token.clone(),
        expires,
    });
    Ok(tokens.access_token)
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub async fn sign_in(app: &AppHandle, state: &SharingSession) -> Result<Value, AppError> {
    let settings = Settings::configured();
    let origin = settings.origin()?;
    if settings.client_id.is_empty() {
        return Err(failure(
            "SagarNet sharing is not set up in this version of Sagar.",
        ));
    }
    let path = CALLBACK_PATH;
    if !path.starts_with('/')
        || path.len() < 2
        || path.len() > 128
        || path[1..].split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(failure(
            "Enter the loopback callback path registered in SagarNet.",
        ));
    }
    let client = client()?;
    let mut session = state
        .account
        .try_lock()
        .map_err(|_| failure("Another SagarNet operation is in progress."))?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, CALLBACK_PORT))
        .await
        .map_err(|_| {
            failure(
                "Sagar’s sign-in port 53142 is in use. Close the program using it and try again.",
            )
        })?;
    let port = listener
        .local_addr()
        .map_err(|_| failure("Could not read the local sign-in port."))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}{path}");
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let oauth_state = Uuid::new_v4().to_string();
    let mut authorize = endpoint(&origin, "/oauth/authorize")?;
    authorize.query_pairs_mut().extend_pairs([
        ("client_id", settings.client_id.as_str()),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("state", &oauth_state),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge(&verifier)),
    ]);

    let (send, receive) = oneshot::channel();
    let send = std::sync::Arc::new(Mutex::new(Some(send)));
    let router = Router::new().route(
        path,
        get(move |Query(callback): Query<Callback>| {
            let send = send.clone();
            let expected = oauth_state.clone();
            async move {
                if callback.state != expected {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        "Invalid sign-in state.",
                    )
                        .into_response();
                }
                if let Some(sender) = send.lock().await.take() {
                    let _ = sender.send(callback.code);
                }
                (
                    [
                        (axum::http::header::CACHE_CONTROL, "no-store"),
                        (axum::http::header::REFERRER_POLICY, "no-referrer"),
                    ],
                    "Return to Sagar to continue. You can close this tab.",
                )
                    .into_response()
            }
        }),
    );
    let cancellation = CancellationToken::new();
    *state
        .pending_sign_in
        .lock()
        .map_err(|_| failure("Could not start sign-in."))? = Some(cancellation.clone());
    app.opener()
        .open_url(authorize.as_str(), None::<&str>)
        .map_err(|_| failure("Could not open the system browser."))?;
    let (stop, shutdown) = oneshot::channel();
    let mut server = std::pin::pin!(
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown.await;
            })
            .into_future()
    );
    let result = tokio::select! {
        _ = cancellation.cancelled() => None,
        outcome = tokio::time::timeout(Duration::from_secs(180), receive) =>
            outcome.ok().and_then(Result::ok).flatten(),
        _ = &mut server => return Err(failure("The local sign-in callback failed.")),
    };
    let _ = stop.send(());
    let _ = tokio::time::timeout(Duration::from_secs(1), &mut server).await;
    let code = result.ok_or_else(|| failure("Sign-in was cancelled or timed out."))?;
    let tokens = token_request(
        &client,
        &origin,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &settings.client_id),
            ("redirect_uri", &redirect),
            ("code", &code),
            ("code_verifier", &verifier),
        ],
    )
    .await?;
    let account = checked(
        client
            .get(endpoint(&origin, "/api/v1/me")?)
            .bearer_auth(&tokens.access_token),
    )
    .await?;
    store(&mut session, settings.account_key(&origin), tokens).await?;
    account
        .json()
        .await
        .map_err(|_| failure("Could not read your SagarNet account."))
}

async fn access(
    session: &mut Option<Session>,
    client: &Client,
    settings: &Settings,
    origin: &Url,
) -> Result<Option<String>, AppError> {
    let key = settings.account_key(origin);
    if let Some(current) = session
        .as_ref()
        .filter(|current| current.account_key == key && current.expires > Instant::now())
    {
        return Ok(Some(current.access_token.clone()));
    }
    let Some(refresh) = credential(key.clone()).await? else {
        return Ok(None);
    };
    match token_request(
        client,
        origin,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &settings.client_id),
            ("refresh_token", &refresh),
        ],
    )
    .await
    {
        Ok(tokens) => store(session, key, tokens).await.map(Some),
        Err(error) if error.code == "sharing_unauthorized" => {
            write_credential(key, None).await?;
            *session = None;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn validated_sharing_html(fragment: &str) -> Result<(String, String), AppError> {
    let (html, plain) = rich_text::validate_document(fragment)?;
    let wrapped = format!("<w>{html}</w>");
    let document = roxmltree::Document::parse(&wrapped)
        .map_err(|_| failure("The chapter could not be read as shareable HTML."))?;
    if document
        .descendants()
        .filter(|node| node.is_element())
        .any(|node| {
            node.attributes()
                .any(|attribute| matches!(attribute.name(), "src" | "srcset"))
                || node.has_tag_name(("http://www.w3.org/2000/svg", "image"))
        })
    {
        return Err(failure(
            "SagarNet chapter HTML cannot include images or embedded book resources.",
        ));
    }
    Ok((html, plain))
}

fn translation_html(
    data_dir: &Path,
    project: &str,
    local_chapter: &str,
) -> Result<Option<(String, i64)>, AppError> {
    let conn = workspace::open_db(data_dir)?;
    let belongs: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ?1 AND project_id = ?2)",
        [local_chapter, project],
        |row| row.get(0),
    )?;
    if !belongs {
        return Err(failure(
            "This chapter does not belong to the selected book.",
        ));
    }
    let draft = crate::storage::documents::load(&conn, local_chapter)?;
    let (html, plain) = validated_sharing_html(draft.html.as_deref().unwrap_or(""))?;
    Ok((!plain.trim().is_empty()).then_some((html, draft.revision)))
}

fn sharing_language(value: &str) -> Result<String, AppError> {
    let mut parts = value.split('-');
    if value.len() <= 63
        && parts.next().is_some_and(|base| {
            (2..=8).contains(&base.len()) && base.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
        && parts.all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(failure(
            "Enter a SagarNet language tag such as my or my-Mymr-MM.",
        ))
    }
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_metadata(hash: &mut Sha256, value: &str) {
    let normalized = normalize(value);
    hash.update((normalized.len() as u64).to_be_bytes());
    hash.update(normalized.as_bytes());
}

fn fingerprint(data_dir: &Path, project_id: &str) -> Result<Fingerprint, AppError> {
    let project = projects::get_project(data_dir, project_id)?;
    let mut archive = SafeArchive::open(
        &workspace::snapshot_path(data_dir, project_id)?,
        &ArchiveLimits::default(),
    )?;
    let opf_path = container::find_opf_path(&mut archive)?;
    let source = opf::parse_opf(&archive.read_entry(&opf_path)?, &opf_path)?;
    let local = projects::list_chapters(data_dir, project_id)?;
    if local.is_empty() || local.len() > 5000 {
        return Err(failure(
            "This book has an unsupported number of XHTML chapters.",
        ));
    }
    let mut hash = Sha256::new();
    hash.update(b"sagar-source-v1\0");
    hash_metadata(&mut hash, source.metadata.title.as_deref().unwrap_or(""));
    hash_metadata(&mut hash, source.metadata.author.as_deref().unwrap_or(""));
    let mut chapters = Vec::with_capacity(local.len());
    for chapter in local {
        let bytes = archive.read_entry(&chapter.resource_href)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| failure("A source chapter is not valid UTF-8."))?;
        let digest = Sha256::digest(text.replace("\r\n", "\n").replace('\r', "\n"));
        hash.update(&digest[..]);
        chapters.push(ManifestChapter {
            spine_position: chapter.spine_index,
            label: chapter.title.chars().take(255).collect(),
            source_fingerprint: hex_digest(&digest),
        });
    }
    Ok(Fingerprint {
        title: project.title,
        author: project.author,
        fingerprint_version: 1,
        source_fingerprint: hex_digest(&hash.finalize()),
        xhtml_spine_count: chapters.len(),
        chapters,
    })
}

const COVER_LIMIT: usize = 2 * 1024 * 1024;

/// Identifies a cover image by its leading bytes, so a mislabeled response is never shown.
fn cover_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Resolves a cover redirect target. Only HTTPS targets are followed, except
/// for a same-origin target (which keeps local HTTP development servers working).
fn cover_redirect(from: &Url, response: &Response) -> Option<Url> {
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)?
        .to_str()
        .ok()?;
    let target = from.join(location).ok()?;
    (target.scheme() == "https" || target.origin() == from.origin()).then_some(target)
}

/// Downloads a book cover for display. Covers are decorative, so any failure
/// returns `null` and the dialog shows a placeholder.
async fn cover(state: &SharingSession, settings: &Settings, book: u64) -> Result<Value, AppError> {
    let origin = settings.origin()?;
    let client = client()?;
    // Reuse a current token without refreshing it, and release the lock before
    // downloading so a cover never blocks other SagarNet requests.
    let token = {
        let session = state.account.lock().await;
        let key = settings.account_key(&origin);
        session
            .as_ref()
            .filter(|current| current.account_key == key && current.expires > Instant::now())
            .map(|current| current.access_token.clone())
    };
    let accept = "image/jpeg, image/png, image/webp";
    let url = endpoint(&origin, &format!("/api/v1/books/{book}/cover"))?;
    let mut request = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, accept);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let Ok(mut response) = request.send().await else {
        return Ok(Value::Null);
    };
    // SagarNet may answer with a redirect to a signed storage URL. Follow one
    // HTTPS hop without the SagarNet token, since it belongs to another host.
    if response.status().is_redirection() {
        let Some(target) = cover_redirect(&url, &response) else {
            return Ok(Value::Null);
        };
        let Ok(next) = client
            .get(target)
            .header(reqwest::header::ACCEPT, accept)
            .send()
            .await
        else {
            return Ok(Value::Null);
        };
        response = next;
    }
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > COVER_LIMIT as u64)
    {
        return Ok(Value::Null);
    }
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if bytes.len() + chunk.len() > COVER_LIMIT {
            return Ok(Value::Null);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(cover_media_type(&bytes).map_or(Value::Null, |media_type| {
        json!({
            "media_type": media_type,
            "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        })
    }))
}

async fn local_work<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| failure("Could not prepare the local chapter."))?
}

pub async fn request(
    data_dir: std::path::PathBuf,
    state: &SharingSession,
    operation: Operation,
) -> Result<Value, AppError> {
    let settings = Settings::configured();
    if let Operation::LocalPreview {
        project,
        local_chapter,
    } = operation
    {
        let preview =
            local_work(move || translation_html(&data_dir, &project, &local_chapter)).await?;
        return Ok(preview.map_or(
            Value::Null,
            |(content, revision)| json!({ "content": content, "revision": revision }),
        ));
    }
    if let Operation::Cover { book } = operation {
        return cover(state, &settings, book).await;
    }
    let origin = settings.origin()?;
    let client = client()?;
    let mut session = state
        .account
        .try_lock()
        .map_err(|_| failure("Another SagarNet operation is in progress."))?;
    let token = if settings.client_id.is_empty() || matches!(operation, Operation::Languages) {
        None
    } else {
        access(&mut session, &client, &settings, &origin).await?
    };
    let send = |builder: RequestBuilder, token: Option<&str>| match token {
        Some(token) => builder.bearer_auth(token),
        None => builder,
    };
    let api = |path: &str| endpoint(&origin, &format!("/api/v1/{path}"));

    if matches!(operation, Operation::Account | Operation::Disconnect) && token.is_none() {
        return Ok(Value::Null);
    }
    if token.is_none()
        && matches!(
            operation,
            Operation::Mine { .. }
                | Operation::Link { .. }
                | Operation::PublishBook { .. }
                | Operation::Create { .. }
                | Operation::Save { .. }
                | Operation::Delete { .. }
        )
    {
        return Err(AppError::new(
            "sharing_unauthorized",
            "Sign in to SagarNet before changing shares.",
        ));
    }
    let checking_account = matches!(operation, Operation::Account);
    let reading_translation = matches!(operation, Operation::Translation { .. });
    let builder = match operation {
        Operation::Account => client.get(api("me")?),
        Operation::Languages => client.get(api("languages")?),
        Operation::Disconnect => {
            let result = checked(send(client.delete(api("me/token")?), token.as_deref())).await;
            if let Err(error) = result {
                if error.code != "sharing_unauthorized" {
                    return Err(error);
                }
            }
            write_credential(settings.account_key(&origin), None).await?;
            *session = None;
            return Ok(Value::Null);
        }
        Operation::Search { query, page } => client
            .get(api("books")?)
            .query(&[("q", query), ("page", page.to_string())]),
        Operation::Book { book } => client.get(api(&format!("books/{book}"))?),
        Operation::Chapters {
            book,
            edition,
            page,
        } => client
            .get(api(&format!("books/{book}/editions/{edition}/chapters"))?)
            .query(&[("page", page)]),
        Operation::Versions {
            chapter,
            page,
            target_language,
        } => client
            .get(api(&format!("chapters/{chapter}/translations"))?)
            .query(&[
                ("page", page.to_string()),
                ("target_language", sharing_language(&target_language)?),
            ]),
        Operation::Translation { translation } => {
            client.get(api(&format!("translations/{translation}"))?)
        }
        Operation::Mine {
            chapter,
            target_language,
        } => client
            .get(api(&format!("chapters/{chapter}/my-translation"))?)
            .query(&[("target_language", sharing_language(&target_language)?)]),
        Operation::Link { book } => client.put(api(&format!("books/{book}/link"))?),
        Operation::PublishBook { book } => client
            .put(api(&format!("books/{book}/visibility"))?)
            .json(&json!({ "visibility": "public" })),
        Operation::Match { project } => {
            let manifest = local_work(move || fingerprint(&data_dir, &project)).await?;
            client.post(api("books/match")?).json(&json!({
                "title": manifest.title, "author": manifest.author, "fingerprint_version": manifest.fingerprint_version,
                "source_fingerprint": manifest.source_fingerprint, "xhtml_spine_count": manifest.xhtml_spine_count
            }))
        }
        Operation::Create {
            project,
            title,
            author,
            description,
            public,
        } => {
            let (manifest, cover) = local_work(move || {
                Ok((
                    fingerprint(&data_dir, &project)?,
                    projects::get_cover(&data_dir, &project)?,
                ))
            })
            .await?;
            if title.trim().is_empty()
                || title.chars().count() > 255
                || author.chars().count() > 255
                || description.chars().count() > 10_000
            {
                return Err(failure(
                    "Enter a book title and check the catalog metadata length limits.",
                ));
            }
            let visibility = if public { "public" } else { "private" };
            let mut form = reqwest::multipart::Form::new()
                .text("title", title.trim().to_string())
                .text("author", author.trim().to_string())
                .text("description", description)
                .text("visibility", visibility)
                .text(
                    "fingerprint_version",
                    manifest.fingerprint_version.to_string(),
                )
                .text("source_fingerprint", manifest.source_fingerprint)
                .text("xhtml_spine_count", manifest.xhtml_spine_count.to_string())
                .text(
                    "chapters",
                    serde_json::to_string(&manifest.chapters)
                        .map_err(|_| failure("Could not encode the chapter list."))?,
                );
            if let Some(cover) = cover.filter(|cover| {
                matches!(
                    cover.media_type.as_str(),
                    "image/jpeg" | "image/png" | "image/webp"
                )
            }) {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(cover.base64)
                    .map_err(|_| failure("Could not read the cover image."))?;
                if bytes.len() <= 2 * 1024 * 1024 {
                    form = form.part(
                        "cover",
                        reqwest::multipart::Part::bytes(bytes)
                            .file_name("cover")
                            .mime_str(&cover.media_type)
                            .map_err(|_| failure("The cover image type is unsupported."))?,
                    );
                }
            }
            client.post(api("books")?).multipart(form)
        }
        Operation::LocalPreview { .. } | Operation::Cover { .. } => {
            return Err(failure("The preview could not be opened."));
        }
        Operation::Save {
            project,
            local_chapter,
            chapter,
            public,
            existing,
            local_revision,
            target_language,
        } => {
            let language = sharing_language(&target_language)?;
            let expected_language = language.clone();
            let content = local_work(move || {
                let (content, revision) = translation_html(&data_dir, &project, &local_chapter)?
                    .ok_or_else(|| failure("Write and save a translation before submitting this chapter."))?;
                if revision != local_revision {
                    return Err(failure(
                        "The local translation changed. Preview it again before sharing.",
                    ));
                }
                if !projects::get_project(&data_dir, &project)?.target_language.eq_ignore_ascii_case(&expected_language) {
                    return Err(failure("The book’s target language changed. Reopen Submit translations."));
                }
                if content.chars().count() > 250_000 || content.contains('\0') {
                    return Err(failure("A translation must contain at most 250,000 characters and no null characters."));
                }
                Ok(content)
            })
            .await?;
            let capabilities: Value = checked(client.get(api("languages")?))
                .await?
                .json()
                .await
                .map_err(|_| failure("Could not check SagarNet’s chapter formats."))?;
            if !capabilities["translation_content_format_versions"]
                .as_array()
                .is_some_and(|versions| versions.contains(&json!(HTML_FORMAT)))
            {
                return Err(failure(
                    "This SagarNet server needs an update before it can accept chapter HTML.",
                ));
            }
            let mut payload = json!({
                "content": content, "target_language": language, "content_format_version": HTML_FORMAT,
                "visibility": if public { "public" } else { "private" }
            });
            if let Some(existing) = existing {
                payload["translation_id"] = json!(existing.translation_id);
                payload["revision"] = json!(existing.revision);
            }
            client
                .put(api(&format!("chapters/{chapter}/my-translation"))?)
                .json(&payload)
        }
        Operation::Delete { chapter, existing } => client
            .delete(api(&format!("chapters/{chapter}/my-translation"))?)
            .json(&existing),
    };

    let retry = builder.try_clone();
    let response = match checked(send(builder, token.as_deref())).await {
        Err(error) if error.code == "sharing_unauthorized" && token.is_some() => {
            *session = None;
            let renewed = access(&mut session, &client, &settings, &origin).await?;
            match (retry, renewed.as_deref()) {
                (Some(request), Some(token)) => match checked(send(request, Some(token))).await {
                    Err(error) if error.code == "sharing_unauthorized" => {
                        write_credential(settings.account_key(&origin), None).await?;
                        *session = None;
                        return if checking_account {
                            Ok(Value::Null)
                        } else {
                            Err(error)
                        };
                    }
                    result => result?,
                },
                (_, None) if checking_account => return Ok(Value::Null),
                (None, Some(_)) => {
                    return Err(failure(
                        "Your session was renewed. Try creating the book again.",
                    ));
                }
                _ => return Err(error),
            }
        }
        result => result?,
    };
    if response.status() == StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    let mut body: Value = response
        .json()
        .await
        .map_err(|_| failure("SagarNet returned an invalid response."))?;
    if reading_translation {
        match body["content_format_version"].as_u64() {
            Some(1) => {}
            Some(HTML_FORMAT) => {
                let html = body["content"]
                    .as_str()
                    .ok_or_else(|| failure("SagarNet returned an invalid translation."))?;
                body["content"] = json!(validated_sharing_html(html)?.0);
            }
            _ => {
                return Err(failure(
                    "This translation uses an unsupported content format.",
                ));
            }
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    #[test]
    fn bundled_config_parses_and_has_a_valid_client_id() {
        let file: ConfigFile =
            serde_json::from_str(include_str!("../../sagarnet.json")).expect("valid config");
        assert!(Uuid::parse_str(&file.client_id).is_ok());
    }

    #[test]
    fn config_prefers_build_overrides_and_uses_dev_server_only_in_development() {
        let file = r#"{"server":"","dev_server":"http://127.0.0.1:8000","client_id":"a"}"#;
        let dev = Settings::from_config(file, None, None, true);
        assert_eq!(dev.server, "http://127.0.0.1:8000");
        assert_eq!(dev.client_id, "a");
        let release = Settings::from_config(file, None, None, false);
        assert!(release.server.is_empty());
        assert!(release.origin().is_err());
        let overridden =
            Settings::from_config(file, Some(" https://sagar.example/ "), Some("b"), false);
        assert_eq!(overridden.server, "https://sagar.example/");
        assert_eq!(overridden.client_id, "b");
    }

    fn imported_fingerprint(
        root: &Path,
        name: &str,
        second_text: &str,
        reversed: bool,
        extra: bool,
    ) -> Fingerprint {
        let file_path = root.join(format!("{name}.epub"));
        let mut zip = ZipWriter::new(std::fs::File::create(&file_path).expect("fixture file"));
        let order = if reversed {
            r#"<itemref idref="b"/><itemref idref="a"/>"#
        } else {
            r#"<itemref idref="a"/><itemref idref="b"/>"#
        };
        let opf = format!(
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
            <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Shared Test</dc:title><dc:creator>Sagar</dc:creator></metadata>
            <manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/></manifest>
            <spine>{order}</spine>
        </package>"#
        );
        let second = format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>B</title></head><body><p>{second_text}</p></body></html>"#
        );
        let files = [
            ("mimetype", "application/epub+zip"),
            (
                "META-INF/container.xml",
                r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            ("content.opf", opf.as_str()),
            (
                "a.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A</title></head><body><p>First</p></body></html>"#,
            ),
            ("b.xhtml", second.as_str()),
        ];
        for (entry, content) in files {
            zip.start_file(
                entry,
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("fixture entry");
            zip.write_all(content.as_bytes()).expect("fixture content");
        }
        if extra {
            zip.start_file("extra.txt", SimpleFileOptions::default())
                .expect("additional entry");
            zip.write_all(b"Packaging metadata").expect("metadata");
        }
        zip.finish().expect("complete fixture");
        let data = root.join("data");
        let project = crate::services::import::import_epub(&data, &file_path, true)
            .expect("import fixture")
            .project;
        fingerprint(&data, &project.id).expect("fingerprint")
    }

    #[test]
    fn fingerprints_source_text_in_reading_order_independently_of_packaging() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let path = directory.path();
        let original = imported_fingerprint(path, "original", "Second", false, false);
        let repackaged = imported_fingerprint(path, "repacked", "Second", false, true);
        let revised = imported_fingerprint(path, "revised", "Changed", false, false);
        let reordered = imported_fingerprint(path, "reordered", "Second", true, false);

        assert_eq!(original.xhtml_spine_count, 2);
        assert_eq!(original.source_fingerprint, repackaged.source_fingerprint);
        assert_ne!(original.source_fingerprint, revised.source_fingerprint);
        assert_ne!(original.source_fingerprint, reordered.source_fingerprint);
        assert_eq!(
            original.chapters[0].source_fingerprint,
            reordered.chapters[1].source_fingerprint
        );
    }

    #[test]
    fn sharing_html_keeps_formatting_and_rejects_active_or_embedded_content() {
        let html = "<h2>အခန်း</h2><p>မြန်မာ <em>စာ</em></p>";
        assert_eq!(
            validated_sharing_html(html).expect("formatted chapter").0,
            html
        );
        assert!(validated_sharing_html(r#"<p>Text<img src="cover.png" /></p>"#).is_err());
        assert!(validated_sharing_html("<p onclick='alert(1)'>Text</p>").is_err());
    }

    #[test]
    fn accepts_only_recognized_cover_images() {
        assert_eq!(
            cover_media_type(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(
            cover_media_type(b"\x89PNG\r\n\x1a\n...."),
            Some("image/png")
        );
        assert_eq!(
            cover_media_type(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(
            cover_media_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            None
        );
        assert_eq!(cover_media_type(b""), None);
    }

    #[test]
    fn computes_the_oauth_pkce_s256_challenge() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        );
    }
}
