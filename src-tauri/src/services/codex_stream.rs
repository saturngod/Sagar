//! One ephemeral Codex App Server turn over local stdio. No book text is logged.
use crate::error::AppError;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const MAX_TEXT: usize = 1_000_000;
const MAX_LINE: usize = 4_000_000;
fn failed() -> AppError {
    AppError::new(
        "translation_failed",
        "Codex App Server could not complete the response. Check your Codex login and selected model, then retry.",
    )
}
fn send(pipe: &mut ChildStdin, message: &Value) -> Result<(), AppError> {
    serde_json::to_writer(&mut *pipe, message).map_err(|_| failed())?;
    pipe.write_all(b"\n")
        .and_then(|_| pipe.flush())
        .map_err(|_| failed())
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct Protocol {
    instructions: String,
    stage: u8,
    thread: Option<String>,
    turn: Option<String>,
    text: HashMap<String, String>,
    phases: HashMap<String, String>,
    final_text: Option<String>,
    bytes: usize,
    done: bool,
}
impl Protocol {
    fn belongs(&self, params: &Value) -> bool {
        params["threadId"].as_str() == self.thread.as_deref()
            && params["turnId"].as_str() == self.turn.as_deref()
            && self.turn.is_some()
    }
    fn handle(
        &mut self,
        v: Value,
        model: &str,
        prompt: &str,
        effort: Option<&str>,
        cwd: &Path,
        on_text: &mut impl FnMut(&[u8]),
    ) -> Result<Vec<Value>, AppError> {
        if !v.is_object() {
            return Err(failed());
        }
        let mut requests = Vec::new();
        if let Some(method) = v["method"].as_str() {
            // Never silently approve tools or permissions requested by book content.
            if let Some(id) = v.get("id") {
                let result = match method {
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                        json!({"decision":"decline"})
                    }
                    "item/permissions/requestApproval" => json!({"permissions":{},"scope":"turn"}),
                    "mcpServer/elicitation/request" => json!({"action":"decline","content":null}),
                    _ => {
                        return Err(AppError::new(
                            "translation_failed",
                            "Codex requested an interactive action. Complete authentication in Codex and retry; translation does not approve tools.",
                        ));
                    }
                };
                requests.push(json!({"id":id,"result":result}));
                return Ok(requests);
            }
            let p = &v["params"];
            match method {
                "turn/started" if p["threadId"].as_str() == self.thread.as_deref() => {
                    self.turn = p["turn"]["id"].as_str().map(str::to_owned);
                }
                "item/started" if self.belongs(p) && p["item"]["type"] == "agentMessage" => {
                    if let (Some(id), Some(phase)) =
                        (p["item"]["id"].as_str(), p["item"]["phase"].as_str())
                    {
                        self.phases.insert(id.into(), phase.into());
                    }
                }
                "item/agentMessage/delta" if self.belongs(p) => {
                    let id = p["itemId"].as_str().ok_or_else(failed)?;
                    let delta = p["delta"].as_str().ok_or_else(failed)?;
                    self.bytes += delta.len();
                    if self.bytes > MAX_TEXT {
                        return Err(AppError::new(
                            "proposal_too_large",
                            "The Codex proposal was too large to use safely.",
                        ));
                    }
                    self.text.entry(id.into()).or_default().push_str(delta);
                    if self
                        .phases
                        .get(id)
                        .is_none_or(|phase| phase == "final_answer")
                    {
                        on_text(delta.as_bytes());
                    }
                }
                "item/completed" if self.belongs(p) && p["item"]["type"] == "agentMessage" => {
                    let item = &p["item"];
                    let id = item["id"].as_str().ok_or_else(failed)?;
                    let text = item["text"].as_str().ok_or_else(failed)?;
                    if text.len() > MAX_TEXT {
                        return Err(AppError::new(
                            "proposal_too_large",
                            "The Codex proposal was too large to use safely.",
                        ));
                    }
                    if item["phase"]
                        .as_str()
                        .or_else(|| self.phases.get(id).map(String::as_str))
                        .is_none_or(|phase| phase == "final_answer")
                    {
                        let previous = self.text.get(id).map(String::as_str).unwrap_or("");
                        let delta = text.strip_prefix(previous).ok_or_else(failed)?;
                        if !delta.is_empty() {
                            on_text(delta.as_bytes());
                        }
                        self.text.insert(id.into(), text.into());
                        self.final_text = Some(text.into());
                    }
                }
                "turn/completed"
                    if p["threadId"].as_str() == self.thread.as_deref()
                        && p["turn"]["id"].as_str() == self.turn.as_deref() =>
                {
                    if p["turn"]["status"] != "completed"
                        || self.final_text.as_ref().is_none_or(|s| s.trim().is_empty())
                    {
                        return Err(failed());
                    }
                    self.done = true;
                }
                "error" if self.belongs(p) && p["willRetry"] != true => return Err(failed()),
                _ => {}
            }
            return Ok(requests);
        }
        if v.get("error").is_some() {
            return Err(failed());
        }
        match (self.stage, v["id"].as_u64()) {
            (0, Some(0)) => {
                if v.get("result").is_none() {
                    return Err(failed());
                }
                self.stage = 1;
                requests.push(json!({"method":"initialized","params":{}}));
                requests.push(json!({"id":1,"method":"thread/start","params":{"model":model,"cwd":cwd,"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only","developerInstructions":self.instructions}}));
            }
            (1, Some(1)) => {
                self.thread = v["result"]["thread"]["id"].as_str().map(str::to_owned);
                let thread = self.thread.as_ref().ok_or_else(failed)?;
                self.stage = 2;
                let mut params = json!({"threadId":thread,"model":model,"cwd":cwd,"input":[{"type":"text","text":prompt}]});
                if let Some(effort) = effort {
                    params["effort"] = json!(effort);
                }
                requests.push(json!({"id":2,"method":"turn/start","params":params}));
            }
            (2, Some(2)) => {
                let turn = v["result"]["turn"]["id"].as_str().ok_or_else(failed)?;
                if self.turn.as_deref().is_some_and(|id| id != turn) {
                    return Err(failed());
                }
                self.turn = Some(turn.into());
                self.stage = 3;
            }
            _ => {}
        }
        Ok(requests)
    }
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
    let model = args
        .windows(2)
        .find(|v| v[0] == "--model")
        .map(|v| v[1].as_str())
        .ok_or_else(failed)?;
    let prompt = args.last().ok_or_else(failed)?;
    let effort = args.iter().find_map(|arg| {
        arg.strip_prefix("model_reasoning_effort=\"")
            .and_then(|s| s.strip_suffix('"'))
    });
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = super::ai::cli_child_path_env(binary) {
        command.env("PATH", path);
    }
    let mut process = Process(command.spawn().map_err(|_| failed())?);
    let mut input = process.0.stdin.take().ok_or_else(failed)?;
    let mut output = process.0.stdout.take().ok_or_else(failed)?;
    let stderr = process.0.stderr.take();
    // Drain diagnostics without displaying startup noise, paths or book context.
    std::thread::spawn(move || {
        if let Some(mut pipe) = stderr {
            let mut buf = [0u8; 8192];
            while matches!(pipe.read(&mut buf), Ok(n) if n > 0) {}
        }
    });
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match output.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    send(
        &mut input,
        &json!({"id":0,"method":"initialize","params":{"clientInfo":{"name":"sagar","title":"Sagar","version":env!("CARGO_PKG_VERSION")}}}),
    )?;
    let started = Instant::now();
    let mut protocol = Protocol {
        instructions: instructions.into(),
        ..Protocol::default()
    };
    let mut pending = Vec::new();
    while !protocol.done {
        if cancel.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "translation_cancelled",
                "The run was cancelled.",
            ));
        }
        if started.elapsed() >= timeout {
            return Err(AppError::new(
                "translation_timeout",
                "The Codex response timed out. Retry keeps complete translated sections.",
            ));
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                pending.extend_from_slice(&chunk);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    if end > MAX_LINE {
                        return Err(failed());
                    }
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let value = serde_json::from_slice(&line).map_err(|_| failed())?;
                    for message in
                        protocol.handle(value, model, prompt, effort, cwd, &mut on_text)?
                    {
                        send(&mut input, &message)?;
                    }
                    if protocol.done {
                        break;
                    }
                }
                if pending.len() > MAX_LINE {
                    return Err(failed());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(failed()),
        }
    }
    drop(input);
    // A persistent server must never keep the translation job running after
    // turn completion. Reap on every path, with a bounded graceful shutdown.
    let shutdown = Instant::now();
    while shutdown.elapsed() < Duration::from_millis(500) {
        if process.0.try_wait().map_err(|_| failed())?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(protocol.final_text.ok_or_else(failed)?.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared() -> Protocol {
        Protocol {
            stage: 3,
            thread: Some("t".into()),
            turn: Some("u".into()),
            ..Default::default()
        }
    }
    fn handle(
        protocol: &mut Protocol,
        value: Value,
        output: &mut Vec<u8>,
    ) -> Result<Vec<Value>, AppError> {
        protocol.handle(
            value,
            "model",
            "prompt",
            Some("low"),
            Path::new("."),
            &mut |chunk| output.extend_from_slice(chunk),
        )
    }
    #[test]
    fn handshake_preserves_model_effort_prompt_and_ephemeral_readonly_policy() {
        let mut protocol = Protocol {
            instructions: "Use ဖန်ရှန်း for Fan Xian.".into(),
            ..Protocol::default()
        };
        let mut output = Vec::new();
        let requests = handle(&mut protocol, json!({"id":0,"result":{}}), &mut output).unwrap();
        assert_eq!(requests[0]["method"], "initialized");
        assert_eq!(requests[1]["params"]["model"], "model");
        assert_eq!(
            requests[1]["params"]["developerInstructions"],
            "Use ဖန်ရှန်း for Fan Xian."
        );
        assert_eq!(requests[1]["params"]["ephemeral"], true);
        assert_eq!(requests[1]["params"]["sandbox"], "read-only");
        assert_eq!(requests[1]["params"]["approvalPolicy"], "never");
        let requests = handle(
            &mut protocol,
            json!({"id":1,"result":{"thread":{"id":"t"}}}),
            &mut output,
        )
        .unwrap();
        assert_eq!(requests[0]["params"]["input"][0]["text"], "prompt");
        assert_eq!(requests[0]["params"]["effort"], "low");
        assert!(
            handle(
                &mut protocol,
                json!({"id":2,"result":{"turn":{"id":"u"}}}),
                &mut output
            )
            .is_ok()
        );
        assert_eq!(protocol.turn.as_deref(), Some("u"));
    }
    #[test]
    fn deltas_stream_before_completion_without_final_message_duplication() {
        let mut protocol = prepared();
        let mut output = Vec::new();
        handle(&mut protocol, json!({"method":"item/started","params":{"threadId":"t","turnId":"u","item":{"type":"agentMessage","id":"a","phase":"final_answer"}}}), &mut output).unwrap();
        for text in ["<p>", "မြန်မာ", "</p>"] {
            handle(&mut protocol, json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"a","delta":text}}), &mut output).unwrap();
            assert!(!protocol.done);
        }
        assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
        handle(&mut protocol, json!({"method":"item/completed","params":{"threadId":"t","turnId":"u","item":{"type":"agentMessage","id":"a","text":"<p>မြန်မာ</p>"}}}), &mut output).unwrap();
        assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
        handle(&mut protocol, json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","status":"completed"}}}), &mut output).unwrap();
        assert!(protocol.done);
        assert_eq!(protocol.final_text.as_deref(), Some("<p>မြန်မာ</p>"));
    }
    #[test]
    fn commentary_and_other_turns_never_enter_the_proposal() {
        let mut protocol = prepared();
        let mut output = Vec::new();
        handle(&mut protocol, json!({"method":"item/started","params":{"threadId":"t","turnId":"u","item":{"type":"agentMessage","id":"comment","phase":"commentary"}}}), &mut output).unwrap();
        handle(&mut protocol, json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"comment","delta":"Working..."}}), &mut output).unwrap();
        handle(&mut protocol, json!({"method":"item/agentMessage/delta","params":{"threadId":"other","turnId":"u","itemId":"a","delta":"ignore"}}), &mut output).unwrap();
        assert!(output.is_empty());
    }
    #[test]
    fn failed_missing_or_oversized_results_are_rejected() {
        for status in ["completed", "failed", "interrupted"] {
            assert!(handle(&mut prepared(), json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","status":status}}}), &mut Vec::new()).is_err());
        }
        assert!(
            handle(
                &mut Protocol::default(),
                json!({"id":0,"error":{"message":"auth"}}),
                &mut Vec::new()
            )
            .is_err()
        );
        let mut protocol = prepared();
        assert!(handle(&mut protocol, json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"a","delta":"x".repeat(MAX_TEXT+1)}}), &mut Vec::new()).is_err());
    }
    #[test]
    fn server_tool_approvals_are_declined() {
        let requests = handle(
            &mut prepared(),
            json!({"id":"approval","method":"item/commandExecution/requestApproval","params":{}}),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(requests[0]["result"]["decision"], "decline");
    }
    #[cfg(unix)]
    fn stub(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }
    #[cfg(unix)]
    #[test]
    fn process_streams_deltas_reaps_persistent_server_and_handles_split_json() {
        let (dir, binary) = stub(
            r#"
read -r init
echo '{"id":0,"result":{}}'
read -r initialized
read -r thread
printf '%s' "$thread" > thread.json
echo '{"id":1,"result":{"thread":{"id":"t"}}}'
read -r turn
printf '%s' "$turn" > turn.json
echo '{"id":2,"result":{"turn":{"id":"u"}}}'
printf '%s' '{"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"a","delta":"<p>မြန်မာ'
sleep 0.1
printf '%s\n' '"}}'
sleep 0.2
echo '{"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"a","delta":"</p>"}}'
echo '{"method":"item/completed","params":{"threadId":"t","turnId":"u","item":{"type":"agentMessage","id":"a","text":"<p>မြန်မာ</p>"}}}'
echo '{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","status":"completed"}}}'
while read -r ignored; do :; done
"#,
        );
        let started = Instant::now();
        let mut times = Vec::new();
        let mut output = Vec::new();
        let request = super::super::ai_translate::build_book_translation_request(
            Some("en"),
            "my",
            "<p>Fan Xian</p>",
            "Use ဖန်ရှန်း for Fan Xian.",
            "Use formal prose.",
        );
        let provider = super::super::ai_translate::TranslationProvider::Cli(
            crate::domain::CliProviderId::Codex,
            binary,
        );
        let result = provider
            .execute(
                "model",
                &request,
                Some("low"),
                dir.path(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                |chunk| {
                    times.push(started.elapsed());
                    output.extend_from_slice(chunk);
                },
                |_| {},
            )
            .unwrap();
        let thread: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("thread.json")).unwrap())
                .unwrap();
        let turn: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("turn.json")).unwrap()).unwrap();
        assert_eq!(
            thread["params"]["developerInstructions"],
            request.instructions
        );
        assert_eq!(turn["params"]["input"][0]["text"], request.input);
        assert_eq!(result, "<p>မြန်မာ</p>".as_bytes());
        assert_eq!(output, result);
        assert_eq!(times.len(), 2);
        assert!(times[0] < times[1]);
    }
    #[cfg(unix)]
    #[test]
    fn process_cancellation_timeout_and_malformed_output_fail_cleanly() {
        let args = vec!["--model".into(), "m".into(), "prompt".into()];
        let (dir, binary) = stub("sleep 3");
        let cancel = AtomicBool::new(true);
        assert_eq!(
            run(
                &binary,
                &args,
                dir.path(),
                &cancel,
                Duration::from_secs(5),
                |_| {}
            )
            .unwrap_err()
            .code,
            "translation_cancelled"
        );
        assert_eq!(
            run(
                &binary,
                &args,
                dir.path(),
                &AtomicBool::new(false),
                Duration::from_millis(50),
                |_| {}
            )
            .unwrap_err()
            .code,
            "translation_timeout"
        );
        let (dir, binary) = stub("echo 'not-json'");
        assert!(
            run(
                &binary,
                &args,
                dir.path(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                |_| {}
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "explicit authenticated installed Codex probe; sends synthetic text only"]
    fn installed_codex_delivers_multiple_text_deltas() {
        let binary =
            super::super::ai::find_cli_binary(crate::domain::CliProviderId::Codex).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let args = super::super::ai_translate::agent_args(
            crate::domain::CliProviderId::Codex,
            "gpt-6.1-sol",
            "Reply exactly <p>hello</p>. Do not use tools.",
            Some("low"),
        );
        let started = Instant::now();
        let mut times = Vec::new();
        let mut output = Vec::new();
        let result = run(
            &binary,
            &args,
            dir.path(),
            &AtomicBool::new(false),
            Duration::from_secs(60),
            |chunk| {
                times.push(started.elapsed());
                output.extend_from_slice(chunk);
            },
        )
        .unwrap();
        assert_eq!(output, result);
        assert_eq!(result, b"<p>hello</p>");
        assert!(times.len() > 1, "Expected multiple actual text deltas");
        assert!(times[0] < started.elapsed());
        eprintln!(
            "Codex live probe: {} deltas, first {:?}, finished {:?}",
            times.len(),
            times[0],
            started.elapsed()
        );
    }
}
