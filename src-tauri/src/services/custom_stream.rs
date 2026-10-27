//! OpenAI Chat Completions, Gemini Interactions and Anthropic Messages SSE. Secrets and provider bodies never
//! leave this Rust transport; only assistant content reaches proposal review.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{error::AppError, storage::ai_providers::CustomSecret};
use serde_json::{Value, json};

const LIMIT: usize = 1_000_000;

fn failed(message: &str) -> AppError {
    AppError::new("provider_failed", message)
}

#[derive(Default)]
struct Decoder {
    protocol: String,
    steps: std::collections::HashMap<u64, String>,
    line: Vec<u8>,
    data: String,
    output: Vec<u8>,
    stopped: bool,
    done: bool,
    received: usize,
    events: usize,
    reasoning_bytes: usize,
}

impl Decoder {
    fn push(&mut self, bytes: &[u8], emit: &mut impl FnMut(&[u8])) -> Result<(), AppError> {
        self.received = self.received.saturating_add(bytes.len());
        if self.received > 16 * LIMIT {
            return Err(failed("The provider stream exceeded the size limit."));
        }
        for &byte in bytes {
            if self.done {
                break;
            }
            if byte == b'\n' {
                let line = std::str::from_utf8(&self.line)
                    .map_err(|_| failed("The provider returned invalid UTF-8."))?
                    .trim_end_matches('\r');
                if line.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    self.event(data.trim(), emit)?;
                } else if let Some(data) = line.strip_prefix("data:") {
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                }
                self.line.clear();
            } else {
                self.line.push(byte);
            }
            if self.line.len() + self.data.len() > LIMIT {
                return Err(failed("The provider returned an oversized stream event."));
            }
        }
        Ok(())
    }

    fn event(&mut self, data: &str, emit: &mut impl FnMut(&[u8])) -> Result<(), AppError> {
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" && (self.protocol.is_empty() || self.protocol == "openai") {
            if !self.stopped {
                return Err(failed(
                    "The provider ended without a completed answer. Retry to continue.",
                ));
            }
            self.done = true;
            return Ok(());
        }
        self.events += 1;
        let value: Value = serde_json::from_str(data)
            .map_err(|_| failed("The provider returned an invalid streaming response."))?;
        if value.get("error").is_some() {
            return Err(failed(
                "The provider reported an API error. Check its configuration and retry.",
            ));
        }
        if self.protocol == "gemini" {
            return self.gemini_event(&value, emit);
        }
        if self.protocol == "anthropic" {
            return self.anthropic_event(&value, emit);
        }
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return Err(failed(
                "The provider returned an invalid streaming response.",
            ));
        };
        for choice in choices {
            if choice.get("index").and_then(Value::as_u64) != Some(0) {
                continue;
            }
            let delta = &choice["delta"];
            self.reasoning_bytes += delta
                .get("reasoning_content")
                .and_then(Value::as_str)
                .map_or(0, str::len);
            if delta.get("tool_calls").is_some() || delta.get("function_call").is_some() {
                return Err(failed(
                    "The provider requested a tool instead of returning text.",
                ));
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if self.stopped || self.output.len() + text.len() > LIMIT {
                    return Err(failed("The provider returned too much or unexpected text."));
                }
                self.output.extend_from_slice(text.as_bytes());
                emit(text.as_bytes());
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if reason != "stop" {
                    return Err(failed(match reason {
                        "length" => {
                            "The provider reached its output limit. Retry to continue complete sections."
                        }
                        "content_filter" => {
                            "The provider filtered the response. Review its content policy."
                        }
                        _ => {
                            "The provider did not finish a text answer. Retry or choose another model."
                        }
                    }));
                }
                self.stopped = true;
            }
        }
        Ok(())
    }

    fn text(&mut self, text: &str, emit: &mut impl FnMut(&[u8])) -> Result<(), AppError> {
        if self.stopped || self.output.len() + text.len() > LIMIT {
            return Err(failed("The provider returned too much or unexpected text."));
        }
        self.output.extend_from_slice(text.as_bytes());
        emit(text.as_bytes());
        Ok(())
    }

    fn gemini_event(
        &mut self,
        value: &Value,
        emit: &mut impl FnMut(&[u8]),
    ) -> Result<(), AppError> {
        let invalid =
            || failed("Gemini returned an invalid Interactions stream. Check the API endpoint.");
        match value["event_type"].as_str() {
            Some("step.start") => {
                let index = value["index"].as_u64().ok_or_else(invalid)?;
                let kind = value["step"]["type"].as_str().ok_or_else(invalid)?;
                if !matches!(kind, "model_output" | "thought") {
                    return Err(failed(
                        "Gemini requested a tool or returned non-text content instead of an answer.",
                    ));
                }
                if self.steps.len() >= 128 || self.steps.insert(index, kind.to_string()).is_some() {
                    return Err(invalid());
                }
                if kind == "model_output" {
                    for content in value["step"]["content"].as_array().into_iter().flatten() {
                        if content["type"] != "text" {
                            return Err(invalid());
                        }
                        self.text(content["text"].as_str().ok_or_else(invalid)?, emit)?;
                    }
                }
            }
            Some("step.delta") => {
                let index = value["index"].as_u64().ok_or_else(invalid)?;
                let kind = self.steps.get(&index).ok_or_else(invalid)?.as_str();
                let delta = &value["delta"];
                match (kind, delta["type"].as_str()) {
                    ("model_output", Some("text")) => {
                        self.text(delta["text"].as_str().ok_or_else(invalid)?, emit)?
                    }
                    ("thought", Some("thought_summary")) => {
                        self.reasoning_bytes +=
                            delta["content"]["text"].as_str().map_or(0, str::len);
                    }
                    ("thought", Some("thought_signature"))
                    | ("model_output", Some("text_annotation_delta")) => {}
                    _ => return Err(invalid()),
                }
            }
            Some("step.stop") => {
                let index = value["index"].as_u64().ok_or_else(invalid)?;
                if self.steps.remove(&index).is_none() {
                    return Err(invalid());
                }
            }
            Some("interaction.completed") => {
                if value["interaction"]["status"] != "completed" || !self.steps.is_empty() {
                    return Err(failed(
                        "Gemini did not complete the answer. Retry to continue complete sections or choose another model.",
                    ));
                }
                self.stopped = true;
                self.done = true;
            }
            Some("interaction.status_update") => {
                if matches!(
                    value["status"].as_str(),
                    Some("failed" | "cancelled" | "incomplete" | "requires_action")
                ) {
                    return Err(failed(
                        "Gemini did not complete the answer. Retry or choose another model.",
                    ));
                }
            }
            Some("interaction.created") => {}
            _ => return Err(invalid()),
        }
        Ok(())
    }

    fn anthropic_event(
        &mut self,
        value: &Value,
        emit: &mut impl FnMut(&[u8]),
    ) -> Result<(), AppError> {
        match value["type"].as_str() {
            Some("content_block_start") => {
                let block = &value["content_block"];
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = block["text"].as_str() {
                            self.text(text, emit)?;
                        }
                    }
                    Some("thinking" | "redacted_thinking") => {}
                    _ => {
                        return Err(failed(
                            "The provider returned non-text content instead of an answer.",
                        ));
                    }
                }
            }
            Some("content_block_delta") => {
                let delta = &value["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => self.text(
                        delta["text"].as_str().ok_or_else(|| {
                            failed("The provider returned an invalid text delta.")
                        })?,
                        emit,
                    )?,
                    Some("thinking_delta") => {
                        self.reasoning_bytes += delta["thinking"].as_str().map_or(0, str::len)
                    }
                    Some("signature_delta") => {}
                    _ => {
                        return Err(failed(
                            "The provider returned non-text content instead of an answer.",
                        ));
                    }
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    if reason != "end_turn" {
                        return Err(failed(
                            "The provider did not finish a text answer. Retry or choose another model.",
                        ));
                    }
                    self.stopped = true;
                }
            }
            Some("message_stop") => {
                if !self.stopped {
                    return Err(failed("The provider ended without a completed answer."));
                }
                self.done = true;
            }
            Some("message_start" | "content_block_stop" | "ping") => {}
            _ => {} // Unknown non-content events may be added by the API.
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<u8>, AppError> {
        if !self.done || !self.stopped || self.output.is_empty() {
            return Err(failed(
                "The provider stream ended before a complete answer. Retry to continue.",
            ));
        }
        Ok(self.output)
    }
}

/// Parse only the Anthropic Messages envelope; never echo response bodies.
fn anthropic_json(bytes: &[u8], emit: &mut impl FnMut(&[u8])) -> Result<Vec<u8>, AppError> {
    let invalid = || {
        failed(
            "The endpoint returned JSON that is not an Anthropic Messages response. Check its API format and base URL.",
        )
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value.get("error").is_some() || value["type"] == "error" {
        return Err(failed(
            "The endpoint returned an API error inside an HTTP success response. Check the API key, model access, quota and base URL.",
        ));
    }
    if value["type"] != "message" || value["role"] != "assistant" {
        return Err(invalid());
    }
    let content = value["content"].as_array().ok_or_else(invalid)?;
    let mut output = Vec::new();
    for block in content {
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().ok_or_else(invalid)?;
                if output.len() + text.len() > LIMIT {
                    return Err(failed("The provider returned too much text."));
                }
                output.extend_from_slice(text.as_bytes());
            }
            Some("thinking" | "redacted_thinking") => {}
            _ => {
                return Err(failed(
                    "The provider returned non-text content instead of an answer.",
                ));
            }
        }
    }
    if output.is_empty() {
        return Err(failed("The provider returned no answer text."));
    }
    // Preserve recoverable text on a token limit/refusal, but never call it complete.
    emit(&output);
    if value["stop_reason"] != "end_turn" {
        return Err(failed(
            "The provider did not finish a text answer. Retry to continue complete sections or choose another model.",
        ));
    }
    Ok(output)
}

fn response_format(content_type: &str) -> &'static str {
    match content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "text/event-stream" => "SSE",
        "application/json" => "JSON",
        "text/html" => "HTML",
        "text/plain" => "plain text",
        "" => "missing content type",
        _ => "unrecognized content type",
    }
}

#[cfg(test)]
fn run(
    secret: &CustomSecret,
    model: &str,
    prompt: &str,
    cancel: &AtomicBool,
    timeout: Duration,
    emit: impl FnMut(&[u8]),
) -> Result<Vec<u8>, AppError> {
    run_with_status(
        secret,
        model,
        &super::ai_translate::AiRequest {
            instructions: String::new(),
            input: prompt.into(),
        },
        None,
        cancel,
        timeout,
        emit,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_with_status(
    secret: &CustomSecret,
    model: &str,
    request_data: &super::ai_translate::AiRequest,
    effort: Option<&str>,
    cancel: &AtomicBool,
    timeout: Duration,
    mut emit: impl FnMut(&[u8]),
    mut status_update: impl FnMut(&str),
) -> Result<Vec<u8>, AppError> {
    let start = std::time::Instant::now();
    status_update("Sending API request; waiting for response headers.");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| failed("The API request could not start."))?;
    runtime.block_on(async {
        let request = async {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(30))
                .build().map_err(|_| failed("The API request could not start."))?;
            let (url, body) = match secret.protocol.as_str() {
                "gemini" => {
                    let model = model.strip_prefix("models/").unwrap_or(model);
                    if model.is_empty() || !model.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) {
                        return Err(failed("Use a valid Gemini model ID from the model list."));
                    }
                    (format!("{}/interactions", super::ai::normalize_provider_base_url(&secret.base_url, "gemini")?),
                        json!({"model":model,"system_instruction":request_data.instructions,"input":request_data.input,"stream":true,"store":false}))
                }
                "anthropic" => (format!("{}/messages", super::ai::normalize_provider_base_url(&secret.base_url, "anthropic")?),
                    json!({"model":model,"max_tokens":8192,"system":request_data.instructions,"messages":[{"role":"user","content":request_data.input}],"stream":true})),
                "openai" => {
                    let mut body = json!({"model":model,"messages":[{"role":"system","content":request_data.instructions},{"role":"user","content":request_data.input}],"stream":true});
                    if let Some(effort) = effort { body["reasoning_effort"] = json!(effort); }
                    (format!("{}/chat/completions", secret.base_url), body)
                }
                _ => return Err(failed("Choose a supported API format in Settings.")),
            };
            let request = client.post(url).header("Accept", "text/event-stream").json(&body);
            let request = match secret.protocol.as_str() {
                "gemini" => request.header("x-goog-api-key", &secret.api_key),
                "anthropic" => request.bearer_auth(&secret.api_key).header("x-api-key", &secret.api_key).header("anthropic-version", "2023-06-01"),
                _ => request.bearer_auth(&secret.api_key),
            };
            let mut response = request.send().await.map_err(|_| failed("Could not connect to the provider. Check its API endpoint and retry."))?;
            let status = response.status();
            status_update(&format!("HTTP {} received after {:.1}s.", status.as_u16(), start.elapsed().as_secs_f32()));
            if !status.is_success() {
                let explanation = match status.as_u16() {
                    401 | 403 => "Access denied. Check the API key and model access.",
                    404 => "API endpoint or model not found. Check the base URL and model ID.",
                    429 => "Rate limit or quota reached. Check the provider quota or retry later.",
                    500 => "The provider reported an internal error. Retry; if it persists, try another model or check the provider status.",
                    503 => "The provider reported service unavailable. Retry later or try another model.",
                    500..=599 => "The provider reported a server error. Retry later or check the provider status.",
                    _ => "Request rejected. Check the API format, endpoint and selected model.",
                };
                return Err(failed(&format!("HTTP {}: {explanation}", status.as_u16())));
            }
            let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
            let format = response_format(content_type);
            status_update(&format!("Response format: {format}."));
            let mut prefix = Vec::new();
            if format != "SSE" {
                if secret.protocol != "anthropic" {
                    return Err(failed(&format!("HTTP {} returned {format} instead of an SSE stream. Check the API format and base URL.", status.as_u16())));
                }
                // Compatible gateways sometimes label SSE as JSON/plain text, or
                // ignore stream=true and return a complete Messages JSON envelope.
                loop {
                    let Some(chunk) = response.chunk().await.map_err(|_| failed("The provider connection was interrupted. Retry to continue."))? else { break; };
                    if prefix.len() + chunk.len() > 16 * LIMIT { return Err(failed("The provider response exceeded the size limit.")); }
                    prefix.extend_from_slice(&chunk);
                    let first = prefix.iter().position(|byte| !byte.is_ascii_whitespace());
                    if let Some(first) = first {
                        let data = &prefix[first..];
                        if data.starts_with(b"data:") || data.starts_with(b"event:") || data.starts_with(b":") {
                            status_update("Receiving SSE data with a nonstandard content type.");
                            break;
                        }
                        if data[0] == b'<' {
                            return Err(failed("The endpoint returned an HTML page instead of an Anthropic API response. Check the API base URL or gateway access."));
                        }
                        if data.len() >= 6 && data[0] != b'{' {
                            return Err(failed("The endpoint returned an unrecognized response instead of Anthropic Messages. Check the API format and base URL."));
                        }
                    }
                }
                let first = prefix.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(prefix.len());
                let data = &prefix[first..];
                if !(data.starts_with(b"data:") || data.starts_with(b"event:") || data.starts_with(b":")) {
                    status_update("Reading a complete Anthropic JSON response; live streaming is unavailable for this response.");
                    return anthropic_json(&prefix, &mut emit);
                }
            }
            status_update("Response is a streaming connection; waiting for the first data.");
            let mut last_update = std::time::Instant::now();
            let mut decoder = Decoder { protocol: secret.protocol.clone(), ..Default::default() };
            if !prefix.is_empty() { decoder.push(&prefix, &mut emit)?; }
            while !decoder.done {
                let Some(chunk) = response.chunk().await.map_err(|_| failed("The provider connection was interrupted. Retry to continue."))? else { break; };
                let first = decoder.received == 0;
                let was_reasoning = decoder.reasoning_bytes > 0;
                let had_content = !decoder.output.is_empty();
                decoder.push(&chunk, &mut emit)?;
                if first || was_reasoning != (decoder.reasoning_bytes > 0)
                    || had_content != !decoder.output.is_empty() || decoder.done
                    || last_update.elapsed() >= Duration::from_secs(2) {
                    status_update(&format!("{:.1}s: {} stream bytes, {} events, {} answer bytes, {} reasoning bytes. {}",
                        start.elapsed().as_secs_f32(), decoder.received, decoder.events,
                        decoder.output.len(), decoder.reasoning_bytes,
                        if decoder.done { "Answer complete." }
                        else if !decoder.output.is_empty() { "Receiving answer text." }
                        else if decoder.reasoning_bytes > 0 { "Provider is reasoning; answer text has not started." }
                        else { "Waiting for answer text." }));
                    last_update = std::time::Instant::now();
                }
                if decoder.done { break; }
            }
            decoder.finish()
        };
        let mut request = std::pin::pin!(request);
        let start = std::time::Instant::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(AppError::new("translation_cancelled", "The request was cancelled."));
            }
            if start.elapsed() >= timeout {
                return Err(AppError::new("translation_timeout", "The provider timed out. Retry to continue."));
            }
            if let Ok(result) = tokio::time::timeout(Duration::from_millis(100), &mut request).await {
                return result;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(text: &str, reason: &str) -> String {
        format!(
            "data: {}\r\n\r\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"index":0,"delta":{"content":text,"reasoning_content":"hidden"},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]})
        )
    }

    fn native_stream(protocol: &str, text: &str, reason: &str) -> String {
        let events = if protocol == "gemini" {
            vec![
                json!({"event_type":"interaction.created","interaction":{"status":"in_progress"}}),
                json!({"event_type":"step.start","index":0,"step":{"type":"thought"}}),
                json!({"event_type":"step.delta","index":0,"delta":{"type":"thought_summary","content":{"type":"text","text":"private reasoning"}}}),
                json!({"event_type":"step.stop","index":0}),
                json!({"event_type":"step.start","index":1,"step":{"type":"model_output"}}),
                json!({"event_type":"step.delta","index":1,"delta":{"type":"text","text":text}}),
                json!({"event_type":"step.stop","index":1}),
                json!({"event_type":"interaction.completed","interaction":{"status":if reason == "STOP" { "completed" } else { "incomplete" }}}),
            ]
        } else {
            vec![
                json!({"type":"message_start","message":{"content":[]}}),
                json!({"type":"content_block_start","content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private reasoning"}}),
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":text}}),
                json!({"type":"message_delta","delta":{"stop_reason":reason}}),
                json!({"type":"message_stop"}),
            ]
        };
        events
            .into_iter()
            .map(|event| format!("data: {event}\r\n\r\n"))
            .collect()
    }

    #[test]
    fn native_protocols_stream_unicode_with_correct_request_and_auth() {
        for (protocol, reason, endpoint, header) in [
            (
                "gemini",
                "STOP",
                "/v1/interactions",
                "x-goog-api-key: test-secret",
            ),
            (
                "anthropic",
                "end_turn",
                "/v1/messages",
                "x-api-key: test-secret",
            ),
        ] {
            let (mut secret, server) = mock_server(
                "200 OK",
                native_stream(protocol, "<p>မြန်မာ</p>", reason),
                false,
            );
            secret.protocol = protocol.into();
            let mut emitted = Vec::new();
            let output = run_with_status(
                &secret,
                "selected-model",
                &super::super::ai_translate::AiRequest {
                    instructions: "Use ဖန်ရှန်း for Fan Xian.".into(),
                    input: "private prompt".into(),
                },
                None,
                &AtomicBool::new(false),
                Duration::from_secs(3),
                |chunk| emitted.extend_from_slice(chunk),
                |_| {},
            )
            .unwrap();
            assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
            assert_eq!(output, emitted);
            let request = server.join().unwrap();
            assert!(
                request.starts_with(&format!("POST {endpoint} HTTP/1.1")),
                "{request}"
            );
            assert!(request.to_ascii_lowercase().contains(header));
            if protocol == "anthropic" {
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer test-secret")
                );
            } else {
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
            }
            assert!(!request.lines().next().unwrap().contains("test-secret"));
            let body: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert!(body.get("reasoning_effort").is_none());
            if protocol == "gemini" {
                assert_eq!(body["input"], "private prompt");
                assert_eq!(body["system_instruction"], "Use ဖန်ရှန်း for Fan Xian.");
                assert_eq!(body["model"], "selected-model");
                assert_eq!(body["stream"], true);
                assert_eq!(body["store"], false);
                assert!(body.get("previous_interaction_id").is_none());
            } else {
                assert!(request.contains("anthropic-version: 2023-06-01"));
                assert_eq!(body["system"], "Use ဖန်ရှန်း for Fan Xian.");
                assert_eq!(body["messages"].as_array().unwrap().len(), 1);
                assert_eq!(body["messages"][0]["content"], "private prompt");
                assert_eq!(body["max_tokens"], 8192);
                assert_eq!(body["model"], "selected-model");
            }
            let mut decoder = Decoder {
                protocol: protocol.into(),
                ..Default::default()
            };
            let mut fragmented = Vec::new();
            for byte in native_stream(protocol, "<p>မြန်မာ</p>", reason).bytes() {
                decoder
                    .push(&[byte], &mut |chunk| fragmented.extend_from_slice(chunk))
                    .unwrap();
            }
            assert_eq!(decoder.finish().unwrap(), output);
            assert_eq!(fragmented, output);
        }
    }

    #[test]
    fn gemini_interactions_accept_full_endpoint_and_prefixed_model() {
        for suffix in ["", "/interactions/"] {
            let (mut secret, server) = mock_server(
                "200 OK",
                native_stream("gemini", "<p>မြန်မာ</p>", "STOP"),
                false,
            );
            secret.protocol = "gemini".into();
            secret.base_url = format!(
                "{}{suffix}",
                secret.base_url.replace("/v1", "/proxy/v1beta")
            );
            let mut trace = Vec::new();
            run_with_status(
                &secret,
                "models/gemini-3.8-flash",
                &super::super::ai_translate::AiRequest {
                    instructions: "Translate into Myanmar.".into(),
                    input: "private book prompt".into(),
                },
                None,
                &AtomicBool::new(false),
                Duration::from_secs(3),
                |_| {},
                |message| trace.push(message.to_string()),
            )
            .unwrap();
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /proxy/v1beta/interactions HTTP/1.1"));
            assert!(request.contains("x-goog-api-key: test-secret"));
            let body: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert_eq!(body["model"], "gemini-3.8-flash");
            assert_eq!(body["input"], "private book prompt");
            assert_eq!(body["store"], false);
            assert_eq!(body["stream"], true);
            for line in trace {
                for private in [
                    "private book prompt",
                    "test-secret",
                    "private reasoning",
                    "မြန်မာ",
                    "/proxy/",
                ] {
                    assert!(!line.contains(private));
                }
            }
        }
    }

    #[test]
    fn gemini_interactions_preserve_partial_text_on_incomplete_or_broken_stream() {
        let good = native_stream("gemini", "<p>မြန်မာ</p>", "STOP");
        for tail in [
            json!({"event_type":"interaction.completed","interaction":{"status":"incomplete"}}),
            json!({"event_type":"interaction.completed","interaction":{"status":"failed"}}),
            json!({"event_type":"error","error":{"message":"private provider response"}}),
            json!({"event_type":"step.delta","index":99,"delta":{"type":"text","text":"unexpected"}}),
            json!({"event_type":"step.start","index":2,"step":{"type":"function_call"}}),
        ] {
            let prefix = good
                .split("data: {\"event_type\":\"interaction.completed\"")
                .next()
                .unwrap();
            let input = format!("{prefix}data: {tail}\n\n");
            let mut decoder = Decoder {
                protocol: "gemini".into(),
                ..Default::default()
            };
            let mut partial = Vec::new();
            let error = decoder
                .push(input.as_bytes(), &mut |chunk| {
                    partial.extend_from_slice(chunk)
                })
                .unwrap_err();
            assert_eq!(partial, "<p>မြန်မာ</p>".as_bytes());
            assert!(!error.message.contains("private provider response"));
        }
        let mut decoder = Decoder {
            protocol: "gemini".into(),
            ..Default::default()
        };
        let prefix = good
            .split("data: {\"event_type\":\"interaction.completed\"")
            .next()
            .unwrap();
        decoder.push(prefix.as_bytes(), &mut |_| {}).unwrap();
        assert!(decoder.finish().is_err());
    }

    #[test]
    fn anthropic_transport_preserves_proxy_prefix_and_accepts_full_endpoint() {
        for (suffix, path) in [
            ("/anthropic", "/anthropic/v1/messages"),
            ("/v1/messages", "/v1/messages"),
            ("", "/v1/messages"),
        ] {
            let (mut secret, server) = mock_server(
                "200 OK",
                native_stream("anthropic", "<p>မြန်မာ</p>", "end_turn"),
                false,
            );
            secret.protocol = "anthropic".into();
            secret.base_url = format!("{}{suffix}", secret.base_url.strip_suffix("/v1").unwrap());
            run(
                &secret,
                "deepseek-flash",
                "synthetic prompt",
                &AtomicBool::new(false),
                Duration::from_secs(3),
                |_| {},
            )
            .unwrap();
            let request = server.join().unwrap();
            assert!(
                request.starts_with(&format!("POST {path} HTTP/1.1")),
                "{request}"
            );
            assert!(request.contains("x-api-key: test-secret"));
            let body: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert_eq!(body["model"], "deepseek-flash");
        }
    }

    #[test]
    fn anthropic_compatible_responses_accept_json_or_mislabeled_sse() {
        for (content_type, body) in [
            ("application/json", json!({"type":"message","role":"assistant","content":[{"type":"thinking","thinking":"private reasoning"},{"type":"text","text":"<p>မြန်မာ</p>"}],"stop_reason":"end_turn"}).to_string()),
            ("application/json", native_stream("anthropic", "<p>မြန်မာ</p>", "end_turn")),
            ("text/plain", native_stream("anthropic", "<p>မြန်မာ</p>", "end_turn")),
        ] {
            let (mut secret, server) = mock_server_with_type("200 OK", body, true, content_type);
            secret.protocol = "anthropic".into();
            secret.base_url = secret.base_url.replace("/v1", "/api/anthropic");
            let mut partial = Vec::new();
            let mut trace = Vec::new();
            let result = run_with_status(&secret, "glm-5.3", &super::super::ai_translate::AiRequest { instructions: "Translate into Myanmar.".into(), input: "private prompt".into() }, None, &AtomicBool::new(false), Duration::from_secs(3), |chunk| partial.extend_from_slice(chunk), |line| trace.push(line.to_string())).unwrap();
            assert_eq!(result, "<p>မြန်မာ</p>".as_bytes());
            assert_eq!(result, partial);
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /api/anthropic/v1/messages HTTP/1.1"));
            assert!(request.contains("authorization: Bearer test-secret"));
            let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert_eq!(body["model"], "glm-5.3");
            assert_eq!(body["stream"], true);
            for line in trace { for private in ["private prompt", "private reasoning", "test-secret", "မြန်မာ"] { assert!(!line.contains(private)); } }
        }
    }

    #[test]
    fn anthropic_json_wait_remains_cancellable_and_bounded_by_deadline() {
        for cancellation in [false, true] {
            let (mut secret, server) =
                mock_server_with_type("200 OK", "{".into(), true, "application/json");
            secret.protocol = "anthropic".into();
            let cancel = AtomicBool::new(false);
            std::thread::scope(|scope| {
                if cancellation {
                    let cancel = &cancel;
                    scope.spawn(move || {
                        std::thread::sleep(Duration::from_millis(50));
                        cancel.store(true, Ordering::SeqCst);
                    });
                }
                let error = run(
                    &secret,
                    "glm-5.3",
                    "synthetic",
                    &cancel,
                    Duration::from_millis(if cancellation { 1000 } else { 80 }),
                    |_| {},
                )
                .unwrap_err();
                assert_eq!(
                    error.code,
                    if cancellation {
                        "translation_cancelled"
                    } else {
                        "translation_timeout"
                    }
                );
            });
            server.join().unwrap();
        }
    }

    #[test]
    fn anthropic_non_sse_errors_are_private_and_partial_answers_remain_recoverable() {
        for (content_type, body, expected) in [
            ("text/html", "<html>private provider body</html>".into(), "HTML page"),
            ("application/json", json!({"error":{"message":"private provider body"}}).to_string(), "API error"),
            ("application/json", json!({"choices":[{"message":{"content":"private provider body"}}]}).to_string(), "not an Anthropic"),
            ("application/json", json!({"type":"message","role":"assistant","content":[{"type":"tool_use"}],"stop_reason":"tool_use"}).to_string(), "non-text"),
            ("application/json", json!({"type":"message","role":"assistant","content":[{"type":"text","text":"<p>မြန်မာ"}],"stop_reason":"max_tokens"}).to_string(), "did not finish"),
        ] {
            let (mut secret, server) = mock_server_with_type("200 OK", body, false, content_type);
            secret.protocol = "anthropic".into();
            let mut partial = Vec::new();
            let error = run(&secret, "glm-5.3", "private prompt", &AtomicBool::new(false), Duration::from_secs(3), |chunk| partial.extend_from_slice(chunk)).unwrap_err();
            assert!(error.message.contains(expected), "{}", error.message);
            assert!(!error.message.contains("private provider body"));
            if expected == "did not finish" { assert_eq!(partial, "<p>မြန်မာ".as_bytes()); }
            else { assert!(partial.is_empty()); }
            server.join().unwrap();
        }
    }

    #[test]
    fn native_protocols_reject_truncation_tools_blocks_and_errors() {
        for (protocol, event) in [
            (
                "gemini",
                json!({"event_type":"interaction.completed","interaction":{"status":"incomplete"}}),
            ),
            (
                "gemini",
                json!({"event_type":"step.start","index":0,"step":{"type":"function_call"}}),
            ),
            (
                "gemini",
                json!({"event_type":"interaction.status_update","status":"failed"}),
            ),
            (
                "anthropic",
                json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"}}),
            ),
            (
                "anthropic",
                json!({"type":"content_block_start","content_block":{"type":"tool_use"}}),
            ),
            ("anthropic", json!({"type":"message_stop"})),
        ] {
            let mut decoder = Decoder {
                protocol: protocol.into(),
                ..Default::default()
            };
            assert!(
                decoder
                    .push(format!("data: {event}\n\n").as_bytes(), &mut |_| {})
                    .is_err()
            );
        }
        for protocol in ["gemini", "anthropic"] {
            let mut decoder = Decoder {
                protocol: protocol.into(),
                ..Default::default()
            };
            assert!(
                decoder
                    .push(
                        b"data: {\"error\":{\"message\":\"private\"}}\n\n",
                        &mut |_| {}
                    )
                    .is_err()
            );
            let mut decoder = Decoder {
                protocol: protocol.into(),
                ..Default::default()
            };
            let partial = if protocol == "gemini" {
                decoder.push(b"data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"model_output\"}}\n\n", &mut |_| {}).unwrap();
                json!({"event_type":"step.delta","index":0,"delta":{"type":"text","text":"partial"}})
            } else {
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"partial"}})
            };
            decoder
                .push(format!("data: {partial}\n\n").as_bytes(), &mut |_| {})
                .unwrap();
            assert!(decoder.finish().is_err());
        }
    }

    fn mock_server(
        status: &str,
        body: String,
        pause: bool,
    ) -> (CustomSecret, std::thread::JoinHandle<String>) {
        mock_server_with_type(status, body, pause, "text/event-stream")
    }

    fn mock_server_with_type(
        status: &str,
        body: String,
        pause: bool,
        content_type: &str,
    ) -> (CustomSecret, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let status = status.to_string();
        let content_type = content_type.to_string();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).unwrap();
            let size: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_string)
                })
                .unwrap()
                .parse()
                .unwrap();
            let mut payload = vec![0; size];
            socket.read_exact(&mut payload).unwrap();
            request.extend_from_slice(&payload);
            write!(socket, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n{body}").unwrap();
            socket.flush().unwrap();
            if pause {
                std::thread::sleep(Duration::from_millis(500));
            }
            String::from_utf8(request).unwrap()
        });
        (
            CustomSecret {
                protocol: "openai".into(),
                base_url: url,
                api_key: "test-secret".into(),
                model: "saved-model".into(),
            },
            handle,
        )
    }

    #[test]
    fn http_transport_uses_saved_endpoint_key_selected_model_and_stream() {
        let (secret, server) = mock_server("200 OK", stream("<p>မြန်မာ</p>", "stop"), false);
        let mut emitted = Vec::new();
        let prompt = super::super::ai_translate::build_book_translation_request(
            Some("en"),
            "my",
            "<p>A synthetic story.</p>",
            "Preserve names.",
            "Use formal prose.",
        );
        let output = run_with_status(
            &secret,
            "selected-model",
            &prompt,
            None,
            &AtomicBool::new(false),
            Duration::from_secs(3),
            |b| emitted.extend_from_slice(b),
            |_| {},
        )
        .unwrap();
        assert_eq!(output, emitted);
        assert_eq!(output, "<p>မြန်မာ</p>".as_bytes());
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-secret")
        );
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "selected-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], prompt.instructions);
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], prompt.input);
        assert!(!prompt.input.contains("Preserve names"));
        assert!(prompt.instructions.contains("Preserve names"));
        assert!(!prompt.instructions.contains("<p>A synthetic story.</p>"));
        let sent = body["messages"][0]["content"].as_str().unwrap();
        assert!(sent.contains("from English (en) into Myanmar · မြန်မာ (my)"));
        assert!(sent.contains("Translation language (Book Settings): Myanmar · မြန်မာ (my)"));
        assert!(sent.contains("prose and headings in Myanmar · မြန်မာ (my)"));
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn trace_reports_stages_without_secrets_and_forwards_explicit_effort() {
        let (secret, server) = mock_server("200 OK", stream("<p>answer</p>", "stop"), false);
        let mut trace = Vec::new();
        run_with_status(
            &secret,
            "m",
            &super::super::ai_translate::AiRequest {
                instructions: "Translate into Myanmar.".into(),
                input: "private prompt".into(),
            },
            Some("low"),
            &AtomicBool::new(false),
            Duration::from_secs(3),
            |_| {},
            |message| trace.push(message.to_string()),
        )
        .unwrap();
        assert!(trace[0].contains("waiting for response headers"));
        assert!(trace.iter().any(|line| line.contains("HTTP 200")));
        assert!(trace.iter().any(|line| line.contains("reasoning bytes")));
        assert!(trace.last().unwrap().contains("Answer complete"));
        for line in &trace {
            for private in ["test-secret", "private prompt", "hidden", "<p>answer</p>"] {
                assert!(!line.contains(private));
            }
        }
        let request = server.join().unwrap();
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["reasoning_effort"], "low");
    }

    #[test]
    fn reasoning_only_activity_is_counted_without_entering_answer() {
        let mut decoder = Decoder::default();
        let event = format!(
            "data: {}\n\n",
            json!({"choices":[{"index":0,"delta":{"reasoning_content":"private reasoning"}}]})
        );
        let mut answer = Vec::new();
        decoder
            .push(event.as_bytes(), &mut |chunk| {
                answer.extend_from_slice(chunk)
            })
            .unwrap();
        assert!(answer.is_empty());
        assert_eq!(decoder.events, 1);
        assert_eq!(decoder.reasoning_bytes, "private reasoning".len());
        assert!(!decoder.done);
    }

    #[test]
    fn chapter_translation_and_retry_preserve_snapshot_and_saved_draft() {
        use crate::storage::projects::{NewChapter, NewProject};
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::services::workspace::open_db(dir.path()).unwrap();
        crate::storage::projects::create_project(
            &mut conn,
            &NewProject {
                original_source_path: None,
                id: "p".into(),
                source_sha256: "synthetic".into(),
                source_snapshot_path: "projects/p/source.epub".into(),
                title: "Synthetic".into(),
                author: None,
                source_language: Some("en".into()),
                target_language: "my".into(),
                created_at: "2026-10-14T00:00:00Z".into(),
            },
            &[(
                NewChapter {
                    id: "c".into(),
                    spine_index: 0,
                    manifest_item_id: "c".into(),
                    resource_href: "c.xhtml".into(),
                    title: "Chapter".into(),
                },
                vec![],
            )],
        )
        .unwrap();
        let snapshot = dir.path().join("projects/p/source.epub");
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&snapshot).unwrap());
        zip.start_file(
            "mimetype",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("c.xhtml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Chapter</title></head><body><p>One</p><p>Two</p></body></html>").unwrap();
        zip.finish().unwrap();
        let before = std::fs::read(&snapshot).unwrap();
        let saved_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapter_documents", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "UPDATE projects SET translation_instructions=?1",
            ["Use ဖန်ရှန်း for Fan Xian."],
        )
        .unwrap();
        for (protocol, reason) in [
            ("openai", "stop"),
            ("gemini", "STOP"),
            ("anthropic", "end_turn"),
        ] {
            for previous in [None, Some("<p>မြန်မာ</p><p>unfinished")] {
                let text = if previous.is_some() {
                    "<p>နှစ်</p>"
                } else {
                    "<p>မြန်မာ</p><p>နှစ်</p>"
                };
                let body = if protocol == "openai" {
                    stream(text, reason)
                } else {
                    native_stream(protocol, text, reason)
                };
                let (secret, server) = mock_server("200 OK", body, false);
                let provider = crate::services::ai::save_ai_custom_provider(
                    dir.path(),
                    crate::services::ai::SaveCustomInput {
                        protocol: protocol.into(),
                        id: None,
                        name: "Test".into(),
                        base_url: secret.base_url,
                        api_key: Some(secret.api_key),
                        model: "glm-test".into(),
                    },
                )
                .unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                crate::services::ai_translate::start_ai_translation(
                    dir.path(),
                    crate::services::ai_translate::StartTranslationInput {
                        chapter_id: "c".into(),
                        agent: format!("custom:{}", provider.id),
                        model: "glm-test".into(),
                        consent: true,
                        previous_output: previous.map(str::to_string),
                        current_instructions: Some("Use formal prose.".into()),
                        reasoning_effort: None,
                    },
                    move |event| {
                        send.send(event).unwrap();
                    },
                )
                .unwrap();
                let terminal = loop {
                    let event = receive.recv_timeout(Duration::from_secs(5)).unwrap();
                    if event.kind == "done" || event.kind == "failed" {
                        break event;
                    }
                };
                assert_eq!(terminal.kind, "done", "{}", terminal.message);
                assert_eq!(
                    terminal.proposal_html.as_deref(),
                    Some("<p>မြန်မာ</p><p>နှစ်</p>")
                );
                let request = server.join().unwrap();
                let payload: Value =
                    serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
                let (instructions, input) = match protocol {
                    "gemini" => (&payload["system_instruction"], &payload["input"]),
                    "anthropic" => (&payload["system"], &payload["messages"][0]["content"]),
                    _ => (
                        &payload["messages"][0]["content"],
                        &payload["messages"][1]["content"],
                    ),
                };
                let instructions = instructions.as_str().unwrap();
                assert!(instructions.contains("Use ဖန်ရှန်း for Fan Xian."));
                assert!(instructions.contains("Use formal prose."));
                assert_eq!(
                    input,
                    if previous.is_some() {
                        "<p>Two</p>"
                    } else {
                        "<p>One</p><p>Two</p>"
                    }
                );
                assert!(!instructions.contains("<p>One</p>"));
                assert!(!instructions.contains("<p>Two</p>"));
            }
        }
        assert_eq!(std::fs::read(snapshot).unwrap(), before);
        let saved_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapter_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(saved_after, saved_before);
    }

    #[test]
    fn saved_custom_provider_runs_html_repair_through_service() {
        for (protocol, reason) in [
            ("openai", "stop"),
            ("gemini", "STOP"),
            ("anthropic", "end_turn"),
        ] {
            let body = if protocol == "openai" {
                stream("<p>မြန်မာ</p>", reason)
            } else {
                native_stream(protocol, "<p>မြန်မာ</p>", reason)
            };
            let (secret, server) = mock_server("200 OK", body, false);
            let dir = tempfile::tempdir().unwrap();
            let provider = crate::services::ai::save_ai_custom_provider(
                dir.path(),
                crate::services::ai::SaveCustomInput {
                    protocol: protocol.into(),
                    id: None,
                    name: "GLM test".into(),
                    base_url: secret.base_url,
                    api_key: Some(secret.api_key),
                    model: "glm-test".into(),
                },
            )
            .unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let started = crate::services::ai_translate::start_ai_html_fix(
                dir.path(),
                crate::services::ai_translate::StartHtmlFixInput {
                    chapter_id: "synthetic-chapter".into(),
                    html: "<p>မြန်မာ</p>".into(),
                    agent: format!("custom:{}", provider.id),
                    model: "glm-test".into(),
                    consent: true,
                    reasoning_effort: None,
                },
                move |event| {
                    send.send(event).unwrap();
                },
            )
            .unwrap();
            let terminal = loop {
                let event = receive.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_eq!(event.job_id, started.job_id);
                if event.kind == "done" || event.kind == "failed" {
                    break event;
                }
            };
            assert_eq!(terminal.kind, "done", "{}", terminal.message);
            assert_eq!(terminal.fixed_html.as_deref(), Some("<p>မြန်မာ</p>"));
            let request = server.join().unwrap();
            let payload: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            let (instructions, input) = match protocol {
                "gemini" => (&payload["system_instruction"], &payload["input"]),
                "anthropic" => (&payload["system"], &payload["messages"][0]["content"]),
                _ => (
                    &payload["messages"][0]["content"],
                    &payload["messages"][1]["content"],
                ),
            };
            assert!(instructions.as_str().unwrap().contains("Do NOT translate"));
            assert!(!instructions.as_str().unwrap().contains("မြန်မာ"));
            assert_eq!(input, "<p>မြန်မာ</p>");
        }
    }

    #[test]
    fn http_errors_are_sanitized_and_redirects_are_rejected() {
        for status in [
            "401 Unauthorized",
            "429 Too Many Requests",
            "500 Server Error",
            "503 Service Unavailable",
            "302 Found",
        ] {
            let (secret, server) = mock_server(status, "secret provider body".into(), false);
            let error = run(
                &secret,
                "m",
                "prompt",
                &AtomicBool::new(false),
                Duration::from_secs(3),
                |_| {},
            )
            .unwrap_err();
            assert!(!error.message.contains("secret"));
            assert!(
                error
                    .message
                    .starts_with(&format!("HTTP {}:", &status[..3]))
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn cancellation_and_deadline_interrupt_silent_streams() {
        for protocol in ["openai", "gemini", "anthropic"] {
            for cancellation in [false, true] {
                let (mut secret, server) = mock_server("200 OK", ": waiting\n\n".into(), true);
                secret.protocol = protocol.into();
                let cancel = AtomicBool::new(false);
                std::thread::scope(|scope| {
                    if cancellation {
                        let cancel = &cancel;
                        scope.spawn(move || {
                            std::thread::sleep(Duration::from_millis(80));
                            cancel.store(true, Ordering::SeqCst);
                        });
                    }
                    let start = std::time::Instant::now();
                    let error = run(
                        &secret,
                        "m",
                        "prompt",
                        &cancel,
                        Duration::from_millis(if cancellation { 1000 } else { 80 }),
                        |_| {},
                    )
                    .unwrap_err();
                    assert_eq!(
                        error.code,
                        if cancellation {
                            "translation_cancelled"
                        } else {
                            "translation_timeout"
                        }
                    );
                    assert!(start.elapsed() < Duration::from_millis(450));
                });
                server.join().unwrap();
            }
        }
    }

    #[test]
    fn fragmented_unicode_stream_emits_only_content() {
        let mut decoder = Decoder::default();
        let mut emitted = Vec::new();
        for byte in stream("<p>မြန်မာ</p>", "stop").bytes() {
            decoder
                .push(&[byte], &mut |b| emitted.extend_from_slice(b))
                .unwrap();
        }
        assert_eq!(decoder.finish().unwrap(), "<p>မြန်မာ</p>".as_bytes());
        assert_eq!(emitted, "<p>မြန်မာ</p>".as_bytes());
    }

    #[test]
    fn rejects_truncation_errors_tools_and_limits() {
        for data in [
            stream("partial", "length"),
            "data: [DONE]\n\n".into(),
            "data: {\"error\":{\"message\":\"secret\"}}\n\n".into(),
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[]}}]}\n\n".into(),
            "data: broken\n\n".into(),
        ] {
            let mut decoder = Decoder::default();
            let error = decoder.push(data.as_bytes(), &mut |_| {}).unwrap_err();
            assert!(!error.message.contains("secret"));
        }
        let mut decoder = Decoder::default();
        decoder
            .push(
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
                &mut |_| {},
            )
            .unwrap();
        assert!(decoder.finish().is_err());
        assert!(
            Decoder::default()
                .push(&vec![b'x'; LIMIT + 1], &mut |_| {})
                .is_err()
        );
    }
}
