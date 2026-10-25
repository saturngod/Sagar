//! AI provider settings models (settings-only slice).
//!
//! No book content is ever attached to these DTOs. API keys are never
//! serialized back to the frontend; summaries only report whether a key is
//! present.

/// Built-in CLI provider identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliProviderId {
    Codex,
    Opencode,
    ClaudeCode,
    Agy,
    Qoder,
}

impl CliProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            CliProviderId::Codex => "codex",
            CliProviderId::Opencode => "opencode",
            CliProviderId::ClaudeCode => "claude_code",
            CliProviderId::Agy => "agy",
            CliProviderId::Qoder => "qoder",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CliProviderId::Codex => "Codex",
            CliProviderId::Opencode => "OpenCode",
            CliProviderId::ClaudeCode => "Claude Code",
            CliProviderId::Agy => "Antigravity (agy)",
            CliProviderId::Qoder => "Qoder",
        }
    }

    /// Binary names searched on `PATH` (without OS suffixes; Windows
    /// extensions are added at lookup time).
    pub fn binaries(&self) -> &'static [&'static str] {
        match self {
            CliProviderId::Codex => &["codex"],
            CliProviderId::Opencode => &["opencode"],
            CliProviderId::ClaudeCode => &["claude"],
            CliProviderId::Agy => &["agy"],
            CliProviderId::Qoder => &["qodercli", "qoder"],
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "codex" => Some(CliProviderId::Codex),
            "opencode" => Some(CliProviderId::Opencode),
            "claude_code" => Some(CliProviderId::ClaudeCode),
            "agy" => Some(CliProviderId::Agy),
            "qoder" => Some(CliProviderId::Qoder),
            _ => None,
        }
    }

    pub fn all() -> [CliProviderId; 5] {
        [
            CliProviderId::Codex,
            CliProviderId::Opencode,
            CliProviderId::ClaudeCode,
            CliProviderId::Agy,
            CliProviderId::Qoder,
        ]
    }
}

/// PATH-only availability for one built-in CLI (no process is spawned and no
/// network request is made for the list view).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliProviderStatus {
    pub id: String,
    pub label: String,
    pub binaries: Vec<String>,
    pub available: bool,
}

/// Result of an explicit per-provider check: PATH lookup plus a bounded
/// `<binary> --version` probe. No book content or API keys are involved.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCheckResult {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub connected: bool,
    pub version: Option<String>,
    /// Short human-readable outcome; never contains filesystem paths.
    pub message: String,
}

/// A user-added API provider with an explicit wire protocol. The secret itself is never
/// serialized; only its presence is reported.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderSummary {
    pub protocol: String,
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderList {
    pub cli: Vec<CliProviderStatus>,
    pub custom: Vec<CustomProviderSummary>,
}

/// Outcome of a user-triggered `GET {base}/models` probe. The request carries
/// only the stored key; no book text is ever sent.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTestResult {
    pub ok: bool,
    pub model_count: Option<usize>,
    /// Short human-readable outcome; never echoes the key or URL.
    pub message: String,
}

/// One selectable translation model. `detail` is a short human hint, never a
/// filesystem path or secret.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationModel {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub reasoning_efforts: Option<Vec<String>>,
}

/// Models offered for one agent. `source` is `live` (queried from the tool or
/// endpoint just now) or `catalog` (built-in known list / saved fallback).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationModelList {
    pub agent: String,
    pub source: String,
    pub models: Vec<TranslationModel>,
    /// Short human-readable note about where the list came from.
    pub message: String,
}

/// The persisted translation choice for future translation work. Selecting a
/// model never starts translation and never sends book content anywhere.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSelection {
    pub reasoning_effort: Option<String>,
    pub agent: String,
    pub model: String,
    pub updated_at: String,
}

/// Handle for a running CLI translation job.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranslationStart {
    pub job_id: String,
}

/// Terminal or progress event for a translation job. The saved draft is never
/// modified by the backend; the frontend inserts `proposal_html` only on user
/// confirmation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranslationEvent {
    pub job_id: String,
    pub chapter_id: String,
    /// `started` | `status` | `stream` | `done` | `failed` | `cancelled`.
    pub kind: String,
    pub proposal_html: Option<String>,
    /// Full-so-far translated text for `stream` events; text
    /// shown locally only, never logged or telemetered.
    pub partial_text: Option<String>,
    /// Short human-readable outcome; never contains book text or secrets.
    pub message: String,
}

/// Handle for a running HTML-fix job. The input is the user's current draft
/// HTML (not the stored source); the backend never modifies the saved draft.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHtmlFixStart {
    pub job_id: String,
}

/// Terminal or progress event for an HTML-fix job. The saved draft is never
/// modified by the backend; the frontend fills `fixed_html` into the editor
/// only on user confirmation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHtmlFixEvent {
    pub job_id: String,
    pub chapter_id: String,
    /// `started` | `status` | `stream` | `done` | `failed` | `cancelled`.
    pub kind: String,
    pub fixed_html: Option<String>,
    /// Full-so-far translated text for `stream` events; text
    /// shown locally only, never logged or telemetered.
    pub partial_text: Option<String>,
    /// Short human-readable outcome; never contains book text or secrets.
    pub message: String,
}
