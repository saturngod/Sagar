//! Per-job authenticated loopback OpenCode server and incremental SSE text.
use crate::error::AppError;
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const MAX_TEXT: usize = 1_000_000;
const MAX_FRAME: usize = 4_000_000;
fn failed() -> AppError {
    AppError::new(
        "translation_failed",
        "OpenCode Server could not complete the response. Check your OpenCode provider and selected model, then retry.",
    )
}
/// Classify provider failures without echoing response bodies, credentials,
/// paths or prompt text into user-visible diagnostics.
///
/// OpenCode V2 reports failures as `{ "type", "message", "status" }`; the
/// response body is never surfaced because it can contain provider detail.
fn provider_error(error: &Value) -> AppError {
    let status = error["status"].as_u64();
    let kind = error["type"].as_str().unwrap_or_default();
    let free_tier = error["message"]
        .as_str()
        .is_some_and(|message| message.to_ascii_lowercase().contains("free tier"));
    let message = match (status, kind) {
        (Some(403), _) if free_tier => {
            "OpenCode's free-model access check rejected this session (HTTP 403). Retry in OpenCode to check provider access."
        }
        (Some(401), _) | (_, "provider.auth") => {
            "OpenCode provider authentication failed. Sign in to that provider in OpenCode, then retry."
        }
        (Some(403), _) => {
            "The OpenCode provider rejected the request (HTTP 403). Check provider access in OpenCode, then retry."
        }
        (Some(429), _) => {
            "The OpenCode provider's usage limit was reached (HTTP 429). Wait before retrying."
        }
        (Some(500..=599), _) => {
            "The OpenCode provider is temporarily unavailable. Retry after the service recovers."
        }
        (_, "provider.overage") => {
            "OpenCode reached the model's output limit. The incomplete response was not applied; retry keeps complete sections."
        }
        _ => {
            "OpenCode reported a provider error. Check the provider connection in OpenCode, then retry."
        }
    };
    AppError::new("opencode_provider_error", message)
}

fn check(cancel: &AtomicBool, started: Instant, timeout: Duration) -> Result<(), AppError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::new(
            "translation_cancelled",
            "The run was cancelled.",
        ));
    }
    if started.elapsed() >= timeout {
        return Err(AppError::new(
            "translation_timeout",
            "The OpenCode response timed out. Retry keeps complete translated sections.",
        ));
    }
    Ok(())
}
fn runtime_config(existing: Option<&str>) -> Result<String, AppError> {
    let mut config = match existing {
        Some(text) => serde_json::from_str::<Value>(text).map_err(|_| {
            AppError::new(
                "opencode_config_invalid",
                "OpenCode's inline configuration must be a JSON object.",
            )
        })?,
        None => json!({}),
    };
    config
        .as_object_mut()
        .ok_or_else(failed)?
        .insert("share".into(), json!("disabled"));
    Ok(config.to_string())
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Server {
    process: Process,
    client: Client,
    base: String,
    password: String,
    session: Option<String>,
}
impl Server {
    fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        // OpenCode V2 only accepts the built-in `opencode` basic-auth user.
        self.client
            .request(method, format!("{}{path}", self.base))
            .basic_auth("opencode", Some(&self.password))
    }
    fn start(
        binary: &Path,
        cwd: &Path,
        cancel: &AtomicBool,
        started: Instant,
        timeout: Duration,
    ) -> Result<Self, AppError> {
        let password = uuid::Uuid::new_v4().to_string();
        let config = runtime_config(std::env::var("OPENCODE_CONFIG_CONTENT").ok().as_deref())?;
        let mut command = Command::new(binary);
        command
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .current_dir(cwd)
            .env("OPENCODE_CONFIG_CONTENT", config)
            .env("OPENCODE_SERVER_PASSWORD", &password)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = super::ai::cli_child_path_env(binary) {
            command.env("PATH", path);
        }
        let mut process = Process(command.spawn().map_err(|_| failed())?);
        let mut stdout = process.0.stdout.take().ok_or_else(failed)?;
        let stderr = process.0.stderr.take();
        std::thread::spawn(move || {
            if let Some(mut pipe) = stderr {
                let mut b = [0u8; 8192];
                while matches!(pipe.read(&mut b), Ok(n) if n > 0) {}
            }
        });
        let (tx, rx) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let mut b = [0u8; 8192];
            loop {
                match stdout.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(b[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut output = Vec::new();
        let base = loop {
            check(cancel, started, timeout)?;
            if started.elapsed() > Duration::from_secs(30) {
                return Err(AppError::new(
                    "translation_failed",
                    "OpenCode Server did not become ready. Check the CLI installation and retry.",
                ));
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    output.extend_from_slice(&chunk);
                    if output.len() > 32768 {
                        return Err(failed());
                    }
                    if let Some(base) = listening_url(&String::from_utf8_lossy(&output)) {
                        break base;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    // Prefer an expired deadline over a generic failure when
                    // the child pipe closes around the same time.
                    check(cancel, started, timeout)?;
                    return Err(failed());
                }
            }
        };
        // Continue draining stdout; a verbose child must not block on its pipe.
        std::thread::spawn(move || while rx.recv().is_ok() {});
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| failed())?;
        Ok(Self {
            process,
            client,
            base,
            password,
            session: None,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            // Remove only this adapter's temporary session, never user sessions.
            let _ = self
                .request(
                    reqwest::Method::POST,
                    &format!("/api/session/{session}/interrupt"),
                )
                .timeout(Duration::from_millis(500))
                .send();
            let _ = self
                .request(reqwest::Method::DELETE, &format!("/api/session/{session}"))
                .timeout(Duration::from_millis(500))
                .send();
        }
        let _ = self.process.0.kill();
        let _ = self.process.0.wait();
    }
}
fn listening_url(output: &str) -> Option<String> {
    let suffix = output.split("http://127.0.0.1:").nth(1)?;
    let port: String = suffix.chars().take_while(char::is_ascii_digit).collect();
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    // Require a delimiter so a split port number is never used prematurely.
    if suffix.len() == port.to_string().len() {
        return None;
    }
    Some(format!("http://127.0.0.1:{port}"))
}
fn read_json(response: reqwest::blocking::Response) -> Result<Value, AppError> {
    let response = response.error_for_status().map_err(|_| failed())?;
    let mut bytes = Vec::new();
    response
        .take((MAX_FRAME + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| failed())?;
    if bytes.len() > MAX_FRAME {
        return Err(failed());
    }
    serde_json::from_slice(&bytes).map_err(|_| failed())
}

#[derive(Default)]
struct Sse {
    pending: Vec<u8>,
    data: Vec<u8>,
}
impl Sse {
    fn push(
        &mut self,
        bytes: &[u8],
        mut on_event: impl FnMut(Value) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                if !self.data.is_empty() {
                    let value = serde_json::from_slice(&self.data).map_err(|_| failed())?;
                    self.data.clear();
                    on_event(value)?;
                }
            } else if let Some(data) = line.strip_prefix(b"data:") {
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
            }
            if self.data.len() > MAX_FRAME {
                return Err(failed());
            }
        }
        if self.pending.len() > MAX_FRAME {
            return Err(failed());
        }
        Ok(())
    }
}
#[derive(Default)]
struct Block {
    text: String,
    sent: usize,
}
#[derive(Default)]
struct Events {
    session: String,
    message: Option<String>,
    blocks: HashMap<u64, Block>,
    order: Vec<u64>,
    complete: bool,
    failed: bool,
    done: bool,
}
impl Events {
    /// Only the assistant turn started by `session.step.started` may become a
    /// proposal. Text belonging to the echoed user prompt, to another message
    /// or to reasoning is never streamed.
    fn owns_message(&self, data: &Value) -> bool {
        match (self.message.as_deref(), data["assistantMessageID"].as_str()) {
            (Some(message), Some(owner)) => message == owner,
            _ => false,
        }
    }
    /// Returns the text block for an ordinal, registering it on first use.
    fn block(&mut self, ordinal: u64) -> &mut Block {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.blocks.entry(ordinal) {
            entry.insert(Block::default());
            self.order.push(ordinal);
        }
        self.blocks.get_mut(&ordinal).expect("block just inserted")
    }
    fn emit(&mut self, on_text: &mut impl FnMut(&[u8])) -> Result<(), AppError> {
        if self.blocks.len() > 2000
            || self.order.len() > 2000
            || self.blocks.values().map(|b| b.text.len()).sum::<usize>() > MAX_TEXT
        {
            return Err(AppError::new(
                "proposal_too_large",
                "The OpenCode proposal was too large to use safely.",
            ));
        }
        for ordinal in self.order.clone() {
            if let Some(block) = self.blocks.get_mut(&ordinal) {
                if block.text.len() > block.sent {
                    on_text(&block.text.as_bytes()[block.sent..]);
                }
                block.sent = block.text.len();
            }
        }
        Ok(())
    }
    /// OpenCode V2 streams a session as `session.*` events whose payload sits
    /// under `data`. Text arrives as `session.text.delta` and is finalised by
    /// `session.text.ended`; `session.step.ended` reports the assistant turn's
    /// finish reason and `session.execution.succeeded` closes the run.
    fn handle(&mut self, value: Value, on_text: &mut impl FnMut(&[u8])) -> Result<(), AppError> {
        let kind = value["type"].as_str().ok_or_else(failed)?;
        let data = value.get("data").ok_or_else(failed)?;
        if data["sessionID"].as_str() != Some(self.session.as_str()) {
            return Ok(());
        }
        match kind {
            "session.step.started" => {
                self.message = data["assistantMessageID"].as_str().map(str::to_string);
            }
            "session.text.started" => {
                let ordinal = data["ordinal"].as_u64().ok_or_else(failed)?;
                if self.owns_message(data) {
                    self.block(ordinal);
                }
            }
            "session.text.delta" => {
                let ordinal = data["ordinal"].as_u64().ok_or_else(failed)?;
                let delta = data["delta"].as_str().ok_or_else(failed)?;
                if !self.owns_message(data) {
                    return Ok(());
                }
                self.block(ordinal).text.push_str(delta);
                self.emit(on_text)?;
            }
            "session.text.ended" => {
                let ordinal = data["ordinal"].as_u64().ok_or_else(failed)?;
                let text = data["text"].as_str().unwrap_or_default();
                if !self.owns_message(data) {
                    return Ok(());
                }
                let block = self.block(ordinal);
                // The final snapshot is authoritative when it extends what was
                // already streamed; a shorter snapshot is ignored so the user
                // never sees text retracted mid-run.
                if text.starts_with(&block.text) {
                    block.text = text.to_string();
                }
                self.emit(on_text)?;
            }
            "session.step.ended" => match data["finish"].as_str().unwrap_or_default() {
                "stop" => self.complete = true,
                "error" | "abort" => self.failed = true,
                _ => {}
            },
            "session.execution.succeeded" | "session.execution.failed" => {
                self.done = true;
            }
            "session.tool.input.started" | "session.tool.called" => {
                return Err(AppError::new(
                    "opencode_tool_requested",
                    "OpenCode requested an interactive tool during translation. No tool was approved and your draft is unchanged.",
                ));
            }
            _ => {}
        }
        if self.blocks.len() > 2000
            || self.order.len() > 2000
            || self.blocks.values().map(|b| b.text.len()).sum::<usize>() > MAX_TEXT
        {
            return Err(AppError::new(
                "proposal_too_large",
                "The OpenCode proposal was too large to use safely.",
            ));
        }
        Ok(())
    }
    fn response(&self) -> Result<Vec<u8>, AppError> {
        if !self.done || !self.complete {
            return Err(failed());
        }
        let text: String = self
            .order
            .iter()
            .filter_map(|ordinal| self.blocks.get(ordinal))
            .map(|block| block.text.as_str())
            .collect();
        if text.trim().is_empty() {
            return Err(failed());
        }
        Ok(text.into_bytes())
    }
}
enum Wire {
    Ready,
    Chunk(Vec<u8>),
    Failed,
}

#[cfg(test)]
pub(super) fn run(
    binary: &Path,
    args: &[String],
    cwd: &Path,
    cancel: &AtomicBool,
    timeout: Duration,
    on_text: impl FnMut(&[u8]),
) -> Result<Vec<u8>, AppError> {
    run_with_instructions(binary, args, "", cwd, cancel, timeout, on_text)
}

// Instruction entries are limited to 8 KiB of serialized JSON. Bound chunks by
// characters so JSON escaping and multibyte scripts fit without truncating text.
fn instruction_chunks(instructions: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for character in instructions.chars() {
        chunk.push(character);
        if chunk.chars().count() == 1000 {
            chunks.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_with_instructions(
    binary: &Path,
    args: &[String],
    instructions: &str,
    cwd: &Path,
    cancel: &AtomicBool,
    timeout: Duration,
    mut on_text: impl FnMut(&[u8]),
) -> Result<Vec<u8>, AppError> {
    let selected = args
        .windows(2)
        .find(|v| v[0] == "--model")
        .map(|v| v[1].as_str())
        .ok_or_else(failed)?;
    let (provider, model) = selected
        .split_once('/')
        .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        .ok_or_else(failed)?;
    let prompt = args.last().ok_or_else(failed)?;
    let variant = args
        .windows(2)
        .find(|v| v[0] == "--variant")
        .map(|v| v[1].as_str());
    let started = Instant::now();
    let mut server = Server::start(binary, cwd, cancel, started, timeout)?;
    // OpenCode V2 serves the effective configuration as a list of documents.
    // The inline configuration Sagar injects is the entry without a path, and
    // it has the highest precedence, so it decides the effective share mode.
    let config = read_json(
        server
            .request(reqwest::Method::GET, "/api/config")
            .send()
            .map_err(|_| failed())?,
    )?;
    let share = config.as_array().and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry["path"].is_null() && entry["info"].is_object())
            .and_then(|entry| entry["info"]["share"].as_str())
            .map(str::to_string)
    });
    if share.as_deref() != Some("disabled") {
        return Err(AppError::new(
            "opencode_sharing_enabled",
            "OpenCode's managed settings prevent disabling session sharing. Translation requires sharing to be disabled.",
        ));
    }
    // Keep OpenCode's normal tool schema: deny removes tools and triggers its
    // free-tier gate. Ask retains the schema, but our event handler aborts any
    // actual tool request without granting it.
    let mut model = json!({"id": model, "providerID": provider});
    if let Some(variant) = variant {
        model["variant"] = json!(variant);
    }
    let session = read_json(server.request(reqwest::Method::POST, "/api/session").json(&json!({"title":"Sagar temporary translation","model":model,"permissions":[{"action":"*","resource":"*","effect":"ask"}]})).send().map_err(|_| failed())?)?;
    let id = session["data"]["id"]
        .as_str()
        .filter(|id| {
            id.starts_with("ses")
                && id.len() < 128
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        })
        .ok_or_else(failed)?
        .to_string();
    server.session = Some(id.clone());
    for (index, chunk) in instruction_chunks(instructions).iter().enumerate() {
        check(cancel, started, timeout)?;
        let response = server
            .request(
                reqwest::Method::PUT,
                &format!("/api/experimental/session/{id}/instructions/entries/sagar-{index:06}"),
            )
            .json(&json!({"value":{"instructions":chunk}}))
            .send()
            .map_err(|_| failed())?;
        if !response.status().is_success() {
            return Err(AppError::new(
                "instructions_unsupported",
                "OpenCode could not accept translation instructions. Update OpenCode or choose another provider.",
            ));
        }
    }
    // The SSE stream lifetime is bounded by the polling loops below via
    // `check`. Give the underlying request headroom so reqwest's own timeout
    // cannot fire at the same instant as the translation deadline and mask a
    // timeout as a generic failure.
    let request = server.request(reqwest::Method::GET, "/api/event").timeout(
        timeout
            .checked_add(Duration::from_secs(10))
            .unwrap_or(timeout),
    );
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let Ok(response) = request.send().and_then(|r| r.error_for_status()) else {
            let _ = tx.send(Wire::Failed);
            return;
        };
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            let _ = tx.send(Wire::Failed);
            return;
        }
        if tx.send(Wire::Ready).is_err() {
            return;
        }
        let mut response = response;
        let mut buf = [0u8; 8192];
        loop {
            match response.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(Wire::Chunk(buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = tx.send(Wire::Failed);
    });
    loop {
        check(cancel, started, timeout)?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Wire::Ready) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => {
                // The event thread reports every transport problem as Failed,
                // including its own read timeout. Prefer the translation
                // deadline when it has expired so timeouts stay distinct from
                // generic failures.
                check(cancel, started, timeout)?;
                return Err(failed());
            }
        }
    }
    // The model and variant travel with the session, so the prompt only
    // carries the text to translate.
    server
        .request(reqwest::Method::POST, &format!("/api/session/{id}/prompt"))
        .json(&json!({"text": prompt}))
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| failed())?;
    let mut events = Events {
        session: id,
        ..Default::default()
    };
    let mut sse = Sse::default();
    while !events.done {
        check(cancel, started, timeout)?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Wire::Chunk(chunk)) => sse.push(&chunk, |v| {
                // Connection/heartbeat events are intentionally payload-free.
                if matches!(
                    v["type"].as_str(),
                    Some("server.connected" | "server.heartbeat")
                ) {
                    return Ok(());
                }
                events.handle(v, &mut on_text)
            })?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => {
                // Same deadline precedence as above: a closed/idle event
                // stream at the deadline is a timeout, not a generic failure.
                check(cancel, started, timeout)?;
                return Err(failed());
            }
        }
    }
    if events.failed {
        return Err(provider_failure(&server, &events));
    }
    events.response()
}

/// Reads the failed assistant turn so the user sees why OpenCode stopped,
/// without exposing provider response bodies.
fn provider_failure(server: &Server, events: &Events) -> AppError {
    let Some(message) = events.message.as_deref() else {
        return failed();
    };
    let detail = server
        .request(
            reqwest::Method::GET,
            &format!("/api/session/{}/message/{message}", events.session),
        )
        .send()
        .ok()
        .and_then(|response| read_json(response).ok());
    match detail {
        Some(value) if value["error"].is_object() => provider_error(&value["error"]),
        _ => failed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(kind: &str, data: Value) -> Value {
        json!({"type":kind,"data":data})
    }
    fn step_started() -> Value {
        event(
            "session.step.started",
            json!({"sessionID":"ses_test","agent":"build","assistantMessageID":"msg_answer"}),
        )
    }
    fn step_ended(finish: &str) -> Value {
        event(
            "session.step.ended",
            json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","finish":finish}),
        )
    }
    fn text_started() -> Value {
        event(
            "session.text.started",
            json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","ordinal":0}),
        )
    }
    fn delta(text: &str) -> Value {
        event(
            "session.text.delta",
            json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","ordinal":0,"delta":text}),
        )
    }
    fn text_ended(text: &str) -> Value {
        event(
            "session.text.ended",
            json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","ordinal":0,"text":text}),
        )
    }
    fn executed() -> Value {
        event(
            "session.execution.succeeded",
            json!({"sessionID":"ses_test"}),
        )
    }
    #[test]
    fn instruction_entries_preserve_unicode_and_fit_the_json_limit() {
        let instructions = "Use မြန်မာ combining marks \"\n\t\u{0001}".repeat(1500);
        let chunks = instruction_chunks(&instructions);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), instructions);
        for chunk in chunks {
            assert!(
                serde_json::to_vec(&json!({"instructions":chunk}))
                    .unwrap()
                    .len()
                    < 8192
            );
        }
    }

    #[test]
    fn provider_errors_explain_status_without_echoing_sensitive_output() {
        let error = provider_error(
            &json!({"type":"provider.error","status":403,"message":"OpenCode's free tier can only be used from within OpenCode","response":{"body":"private prompt and credentials"}}),
        );
        assert_eq!(error.code, "opencode_provider_error");
        assert!(error.message.contains("free-model access"));
        assert!(error.message.contains("403"));
        assert!(!error.message.contains("private"));
        assert!(
            provider_error(
                &json!({"type":"provider.auth","status":403,"message":"Authorization failed"})
            )
            .message
            .contains("authentication")
        );
        for (status, expected) in [
            (401, "authentication"),
            (429, "usage limit"),
            (503, "temporarily unavailable"),
        ] {
            assert!(
                provider_error(&json!({"type":"provider.error","status":status}))
                    .message
                    .contains(expected)
            );
        }
    }

    #[test]
    fn runtime_override_disables_sharing_and_preserves_inline_preferences() {
        let config: Value = serde_json::from_str(
            &runtime_config(Some(
                r#"{"share":"auto","model":"p/m","provider":{"p":{"name":"local"}}}"#,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(config["share"], "disabled");
        assert_eq!(config["model"], "p/m");
        assert_eq!(config["provider"]["p"]["name"], "local");
        assert!(runtime_config(Some("not json")).is_err());
    }

    #[test]
    fn sse_handles_split_unicode_crlf_multiline_data_and_keepalive() {
        let input = format!(
            ": heartbeat\r\nevent: message\r\ndata: {}\r\n\r\n",
            delta("မြန်မာ")
        );
        let mut sse = Sse::default();
        let mut found = Vec::new();
        for chunk in input.as_bytes().chunks(2) {
            sse.push(chunk, |v| {
                found.push(v);
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(found, vec![delta("မြန်မာ")]);
        sse.push(b"data: {\ndata: \"type\":\"server.connected\"}\n\n", |_| {
            Ok(())
        })
        .unwrap();
        assert!(sse.push(b"data: not-json\n\n", |_| Ok(())).is_err());
    }
    #[test]
    fn streaming_text_deduplicates_final_snapshot_and_requires_completed_assistant() {
        let mut e = Events {
            session: "ses_test".into(),
            ..Default::default()
        };
        let mut output = Vec::new();
        for v in [
            step_started(),
            text_started(),
            delta("<p>မြန်မာ"),
            delta("</p>"),
            text_ended("<p>မြန်မာ</p>"),
        ] {
            e.handle(v, &mut |c| output.extend_from_slice(c)).unwrap();
            assert!(!e.done);
        }
        assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
        e.handle(step_ended("stop"), &mut |c| output.extend_from_slice(c))
            .unwrap();
        e.handle(executed(), &mut |_| {}).unwrap();
        assert_eq!(e.response().unwrap(), output);
    }
    #[test]
    fn source_echo_reasoning_other_sessions_and_errors_never_become_proposals() {
        let mut e = Events {
            session: "ses_test".into(),
            ..Default::default()
        };
        let mut out = Vec::new();
        e.handle(
            event(
                "session.text.started",
                json!({"sessionID":"ses_test","assistantMessageID":"msg_user","ordinal":0}),
            ),
            &mut |_| {},
        )
        .unwrap();
        e.handle(
            event(
                "session.text.delta",
                json!({"sessionID":"ses_test","assistantMessageID":"msg_user","ordinal":0,"delta":"private source"}),
            ),
            &mut |c| out.extend_from_slice(c),
        )
        .unwrap();
        e.handle(
            event(
                "session.reasoning.delta",
                json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","delta":"ignore"}),
            ),
            &mut |_| panic!("reasoning leaked"),
        )
        .unwrap();
        e.handle(
            event(
                "session.text.delta",
                json!({"sessionID":"other","assistantMessageID":"msg_answer","ordinal":0,"delta":"ignore"}),
            ),
            &mut |_| panic!("other session leaked"),
        )
        .unwrap();
        assert!(out.is_empty());
        assert!(e.response().is_err());
        // A tool request is never approved during translation.
        assert!(
            e.handle(
                event(
                    "session.tool.input.started",
                    json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","name":"shell"}),
                ),
                &mut |_| {}
            )
            .is_err()
        );
    }
    #[test]
    fn output_limit_is_checked_before_display_and_urls_stay_on_loopback() {
        let mut e = Events {
            session: "ses_test".into(),
            ..Default::default()
        };
        e.handle(step_started(), &mut |_| {}).unwrap();
        assert!(
            e.handle(delta(&"x".repeat(MAX_TEXT + 1)), &mut |_| panic!(
                "oversized output displayed"
            ))
            .is_err()
        );
        // OpenCode V2 prints "server listening on ..."; V1 printed
        // "opencode server listening on ...". Both must be recognised.
        assert_eq!(
            listening_url("opencode server listening on http://127.0.0.1:1234\n"),
            Some("http://127.0.0.1:1234".into())
        );
        assert_eq!(
            listening_url("server listening on http://127.0.0.1:1234\n"),
            Some("http://127.0.0.1:1234".into())
        );
        for bad in [
            "http://example.com:1234\n",
            "http://0.0.0.0:1234\n",
            "http://127.0.0.1:1234",
            "http://127.0.0.1:0\n",
        ] {
            assert!(listening_url(bad).is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    fn http_adapter_preserves_selection_streams_and_cleans_up_on_all_outcomes() {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
            os::unix::fs::PermissionsExt,
            sync::{Arc, Mutex},
        };
        for mode in [
            "success",
            "cancel",
            "timeout",
            "permission",
            "instructions-rejected",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let base = format!("http://{addr}");
            let dir = tempfile::tempdir().unwrap();
            let binary = dir.path().join("opencode");
            std::fs::write(
                &binary,
                format!("#!/bin/sh\necho 'server listening on {base}'\nexec sleep 10\n"),
            )
            .unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
            let recorded = calls.clone();
            let mock_server = std::thread::spawn(move || {
                let mut events = None;
                for connection in listener.incoming() {
                    let mut socket = connection.unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        socket.read_exact(&mut byte).unwrap();
                        headers.push(byte[0]);
                        assert!(headers.len() < 16384);
                    }
                    let headers = String::from_utf8(headers).unwrap();
                    assert!(headers.lines().any(|line| {
                        line.to_ascii_lowercase()
                            .starts_with("authorization: basic ")
                    }));
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).unwrap();
                    let request = headers
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let body = if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    };
                    // Test-only exit signal: a timeout run may end before its
                    // own cleanup DELETE, and the mock must always terminate.
                    if request == "GET /__teardown" {
                        break;
                    }
                    recorded.lock().unwrap().push((request.clone(), body));
                    if request == "GET /api/event" {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"server.connected\"}\n\n").unwrap();
                        events = Some(socket);
                        continue;
                    }
                    let response = if request == "POST /api/session" {
                        r#"{"data":{"id":"ses_test"}}"#
                    } else if request == "GET /api/config" {
                        r#"[{"type":"document","path":"/tmp/opencode.json","info":{}},{"type":"document","path":null,"info":{"share":"disabled"}}]"#
                    } else {
                        "true"
                    };
                    let status = if mode == "instructions-rejected"
                        && request.starts_with("PUT /api/experimental/")
                    {
                        "404 Not Found"
                    } else {
                        "200 OK"
                    };
                    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{response}",response.len()).as_bytes()).unwrap();
                    if request.ends_with("/prompt") {
                        let stream = events.as_mut().unwrap();
                        for v in [step_started(), text_started(), delta("<p>မြန်မာ")] {
                            stream
                                .write_all(format!("data: {v}\n\n").as_bytes())
                                .unwrap();
                        }
                        if mode == "permission" {
                            let request = event(
                                "session.tool.input.started",
                                json!({"sessionID":"ses_test","assistantMessageID":"msg_answer","name":"shell"}),
                            );
                            stream
                                .write_all(format!("data: {request}\n\n").as_bytes())
                                .unwrap();
                        }
                        if mode == "success" {
                            std::thread::sleep(Duration::from_millis(200));
                            for v in [
                                delta("</p>"),
                                text_ended("<p>မြန်မာ</p>"),
                                step_ended("stop"),
                                executed(),
                            ] {
                                stream
                                    .write_all(format!("data: {v}\n\n").as_bytes())
                                    .unwrap();
                            }
                        }
                    }
                    if request == "DELETE /api/session/ses_test" {
                        break;
                    }
                }
            });
            let args = super::super::ai_translate::agent_args(
                crate::domain::CliProviderId::Opencode,
                "provider/model",
                "Translate.",
                Some("high"),
            );
            let cancel = AtomicBool::new(false);
            let mut output = Vec::new();
            let mut arrivals = Vec::new();
            let started = Instant::now();
            let timeout = if mode == "timeout" {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(5)
            };
            let result = run_with_instructions(
                &binary,
                &args,
                "Use ဖန်ရှန်း for Fan Xian.",
                dir.path(),
                &cancel,
                timeout,
                |chunk| {
                    arrivals.push(started.elapsed());
                    output.extend_from_slice(chunk);
                    if mode == "cancel" {
                        cancel.store(true, Ordering::SeqCst);
                    }
                },
            );
            // The mock must exit even when the run ends without its own
            // cleanup DELETE (a timeout can expire during startup).
            // Connecting fails once the mock already exited via the DELETE.
            // A failed write means the same thing: the mock is already gone,
            // so the error must not be raised here.
            if let Ok(mut teardown) = TcpStream::connect(addr) {
                let _ = teardown.write_all(
                    b"GET /__teardown HTTP/1.1\r\nauthorization: basic dGVzdA==\r\ncontent-length: 0\r\n\r\n",
                );
            }
            // Bound the wait: a mock that never terminates must fail the test
            // loudly instead of hanging the whole suite.
            let deadline = Instant::now() + Duration::from_secs(10);
            while !mock_server.is_finished() {
                assert!(
                    Instant::now() < deadline,
                    "the mock server did not terminate"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            mock_server.join().unwrap();
            if mode == "success" {
                assert_eq!(result.unwrap(), "<p>မြန်မာ</p>".as_bytes());
                assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
                assert_eq!(arrivals.len(), 2);
                assert!(arrivals[0] < arrivals[1]);
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    if mode == "cancel" {
                        "translation_cancelled"
                    } else if mode == "permission" {
                        "opencode_tool_requested"
                    } else if mode == "instructions-rejected" {
                        "instructions_unsupported"
                    } else {
                        "translation_timeout"
                    }
                );
            }
            let calls = calls.lock().unwrap();
            // The timeout budget can expire anywhere in the run, including
            // before the session or prompt are sent. Only modes whose
            // expected outcome already required those requests — a streamed
            // answer, a cancelled delta or a tool request — may assert on
            // them; timeout mode may only require cleanup once a session
            // exists, because startup may legitimately consume the budget.
            if mode == "instructions-rejected" {
                assert!(!calls.iter().any(|(path, _)| path.ends_with("/prompt")));
                assert!(
                    calls
                        .iter()
                        .any(|(path, _)| path == "DELETE /api/session/ses_test")
                );
                continue;
            }
            if mode == "timeout" {
                if calls.iter().any(|(path, _)| path == "POST /api/session") {
                    assert!(
                        calls
                            .iter()
                            .any(|(path, _)| path == "POST /api/session/ses_test/interrupt")
                    );
                    assert!(
                        calls
                            .iter()
                            .any(|(path, _)| path == "DELETE /api/session/ses_test")
                    );
                }
                continue;
            }
            let create = &calls
                .iter()
                .find(|(path, _)| path == "POST /api/session")
                .unwrap()
                .1;
            assert_eq!(create["permissions"][0]["action"], "*");
            assert_eq!(create["permissions"][0]["effect"], "ask");
            // OpenCode V2 carries the model and variant on the session.
            assert_eq!(create["model"]["providerID"], "provider");
            assert_eq!(create["model"]["id"], "model");
            assert_eq!(create["model"]["variant"], "high");
            let prompt = &calls
                .iter()
                .find(|(path, _)| path.ends_with("/prompt"))
                .unwrap()
                .1;
            assert_eq!(prompt["text"], "Translate.");
            let instruction_at = calls
                .iter()
                .position(|(path, _)| {
                    path.starts_with("PUT /api/experimental/session/ses_test/instructions/entries/")
                })
                .unwrap();
            let prompt_at = calls
                .iter()
                .position(|(path, _)| path.ends_with("/prompt"))
                .unwrap();
            assert!(instruction_at < prompt_at);
            assert_eq!(
                calls[instruction_at].1["value"]["instructions"],
                "Use ဖန်ရှန်း for Fan Xian."
            );
            assert!(
                calls
                    .iter()
                    .any(|(path, _)| path == "POST /api/session/ses_test/interrupt")
            );
            assert!(
                calls
                    .iter()
                    .any(|(path, _)| path == "DELETE /api/session/ses_test")
            );
        }
    }

    #[test]
    #[ignore = "explicit authenticated OpenCode probe; sends synthetic text only"]
    fn installed_opencode_delivers_multiple_text_deltas() {
        let binary =
            super::super::ai::find_cli_binary(crate::domain::CliProviderId::Opencode).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let model = std::env::var("SAGAR_TEST_OPENCODE_MODEL").expect("Set SAGAR_TEST_OPENCODE_MODEL to an installed, authenticated provider/model for this explicit probe");
        let args = super::super::ai_translate::agent_args(
            crate::domain::CliProviderId::Opencode,
            &model,
            "Reply exactly <p>hello</p>. Do not use tools.",
            None,
        );
        let started = Instant::now();
        let mut times = Vec::new();
        let mut streamed = Vec::new();
        let result = run(
            &binary,
            &args,
            dir.path(),
            &cancel,
            Duration::from_secs(60),
            |chunk| {
                times.push(started.elapsed());
                streamed.extend_from_slice(chunk);
            },
        )
        .unwrap();
        assert_eq!(result, b"<p>hello</p>");
        assert_eq!(result, streamed);
        // The provider decides how finely it splits its answer, so a short
        // reply can arrive as a single chunk. The deterministic mock-server
        // test asserts that several separate chunks are delivered.
        assert!(
            !times.is_empty(),
            "Expected streamed text from the SSE stream"
        );
        eprintln!(
            "OpenCode live probe: {} deltas, first {:?}, finished {:?}",
            times.len(),
            times[0],
            started.elapsed()
        );
    }
}
