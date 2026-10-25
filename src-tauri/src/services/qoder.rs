//! Qoder's headless CLI control channel. Catalog queries send no user prompt
//! and therefore never start inference. Credentials stay in Qoder's local login.
use crate::{domain::TranslationModel, error::AppError};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_CATALOG_BYTES: usize = 4_000_000;

pub(super) fn base_args() -> Vec<String> {
    [
        "--print",
        "--output-format",
        "stream-json",
        "--tools",
        "",
        "--strict-mcp-config",
        "--setting-sources",
        "",
        "--no-session-persistence",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn unavailable() -> AppError {
    AppError::new(
        "models_unavailable",
        "Qoder models could not be loaded. Run qodercli login and check your connection, then retry.",
    )
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn list_models(
    binary: &Path,
    timeout: Duration,
) -> Result<Vec<TranslationModel>, AppError> {
    let work_dir = tempfile::tempdir().map_err(|_| unavailable())?;
    let mut command = Command::new(binary);
    command
        .args(base_args())
        .args(["--input-format", "stream-json"])
        .current_dir(work_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = super::ai::cli_child_path_env(binary) {
        command.env("PATH", path);
    }
    let mut child = Process(command.spawn().map_err(|_| unavailable())?);
    let stdout = child.0.stdout.take().ok_or_else(unavailable)?;
    let (sender, receiver) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout.take((MAX_CATALOG_BYTES + 1) as u64));
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let input = child.0.stdin.as_mut().ok_or_else(unavailable)?;
    serde_json::to_writer(
        &mut *input,
        &json!({
            "type":"control_request", "request_id":"sagar-models",
            "request":{"subtype":"get_models", "fetchStrategy":"live"}
        }),
    )
    .map_err(|_| unavailable())?;
    input
        .write_all(b"\n")
        .and_then(|_| input.flush())
        .map_err(|_| unavailable())?;
    let deadline = Instant::now() + timeout;
    let mut bytes = 0;
    loop {
        let line = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| unavailable())?;
        bytes += line.len();
        if bytes > MAX_CATALOG_BYTES {
            return Err(unavailable());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = serde_json::from_slice(&line).map_err(|_| unavailable())?;
        if value["type"] != "control_response" || value["response"]["request_id"] != "sagar-models"
        {
            continue;
        }
        if value["response"]["subtype"] != "success" {
            return Err(unavailable());
        }
        let models = parse_models(&value["response"]["response"]);
        return if models.is_empty() {
            Err(unavailable())
        } else {
            Ok(models)
        };
    }
}

fn parse_models(body: &Value) -> Vec<TranslationModel> {
    let mut seen = HashSet::new();
    body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            if model["isEnabled"] == false {
                return None;
            }
            let id = model["value"].as_str()?.trim();
            if id.is_empty()
                || id.len() > 128
                || id.chars().any(char::is_control)
                || !seen.insert(id.to_owned())
            {
                return None;
            }
            let levels: Vec<String> = if let Some(efforts) = model["efforts"].as_array() {
                efforts
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            } else {
                model
                    .pointer("/thinking_config/enabled/efforts")
                    .and_then(Value::as_object)
                    .map(|efforts| efforts.keys().cloned().collect())
                    .unwrap_or_default()
            };
            let efforts = ["low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .filter(|effort| levels.iter().any(|level| level == effort))
                .map(str::to_owned)
                .collect();
            Some(TranslationModel {
                id: id.to_owned(),
                label: model["displayName"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(id)
                    .to_owned(),
                detail: None,
                reasoning_efforts: Some(efforts),
            })
        })
        .take(500)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_ids_and_model_specific_efforts() {
        let models = parse_models(&json!({"models":[
            {"value":"dfmodel", "displayName":"DeepSeek-Flash", "efforts":["high","low","max","bogus"]},
            {"value":"qmodel", "thinking_config":{"enabled":{"efforts":{"medium":{},"xhigh":{}}}}},
            {"value":"auto"}, {"value":"disabled","isEnabled":false},
            {"value":"dfmodel"}, {"value":""}, {"value":"bad\nvalue"}
        ]}));
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "dfmodel");
        assert_eq!(models[0].label, "DeepSeek-Flash");
        assert_eq!(
            models[0].reasoning_efforts.as_ref().unwrap(),
            &["low", "high", "max"]
        );
        assert_eq!(
            models[1].reasoning_efforts.as_ref().unwrap(),
            &["medium", "xhigh"]
        );
        assert!(models[2].reasoning_efforts.as_ref().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_control_channel_checks_correlation_errors_and_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("qodercli");
        let response = json!({"type":"control_response","response":{
            "subtype":"success","request_id":"sagar-models",
            "response":{"models":[{"value":"dfmodel","displayName":"DeepSeek-Flash","efforts":["high"]}]}
        }});
        for (script, succeeds) in [
            (format!("read request\nprintf '%s\\n' '{}'\nexec sleep 5", response), true),
            ("read request\necho '{\"type\":\"control_response\",\"response\":{\"request_id\":\"sagar-models\",\"subtype\":\"error\"}}'".into(), false),
            ("read request\necho 'invalid json'".into(), false),
            ("exec sleep 5".into(), false),
            (format!("read request\necho '{}'", response.to_string().replace("sagar-models", "stale")), false),
        ] {
            std::fs::write(&binary, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            let start = Instant::now();
            let result = list_models(&binary, Duration::from_millis(300));
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    #[ignore = "explicit installed Qoder probe; loads model metadata only"]
    fn installed_catalog() {
        let binary =
            super::super::ai::find_cli_binary(crate::domain::CliProviderId::Qoder).unwrap();
        let models = list_models(&binary, Duration::from_secs(20)).unwrap();
        assert!(!models.is_empty());
        assert!(models.iter().any(|model| {
            model
                .reasoning_efforts
                .as_ref()
                .is_some_and(|efforts| !efforts.is_empty())
        }));
    }
}
