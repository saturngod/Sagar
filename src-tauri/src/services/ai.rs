//! AI provider settings (settings-only slice).
//!
//! Built-in CLI providers (Codex, OpenCode, Claude Code) are detected on
//! `PATH`; the list view never spawns processes or touches the network.
//! An explicit per-provider check runs `<binary> --version` with a timeout.
//! Custom API providers are stored in app-owned SQLite with
//! masked reads, and an explicit test performs a single `GET {base}/models`
//! request carrying only the stored key. No book content is ever sent.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{
    AiProviderList, CliCheckResult, CliProviderId, CliProviderStatus, CustomProviderSummary,
    CustomTestResult, TranslationModel, TranslationModelList, TranslationSelection,
};
use crate::error::AppError;
use crate::storage::{self, ai_providers};

/// Bounded wait for `<binary> --version`.
const VERSION_TIMEOUT: Duration = Duration::from_secs(8);
/// Bounded wait for a custom provider `/models` probe.
const MODELS_TIMEOUT: Duration = Duration::from_secs(10);
/// Model listings can be slow on a first run (caches, remote refresh).
const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_VERSION_CHARS: usize = 120;
const MAX_MODELS: usize = 2000;
const MAX_MODEL_ID_CHARS: usize = 256;

pub struct SaveCustomInput {
    pub protocol: String,
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

fn open_settings_db(data_dir: &Path) -> Result<rusqlite::Connection, AppError> {
    storage::initialize(data_dir)?;
    storage::open_database(&data_dir.join("sagar.db"))
}

// ------------------------------------------------------------ CLI lookup

/// File names tried for one binary. Windows installers (including npm shims)
/// use executable extensions. Never select npm's extensionless POSIX shim
/// on Windows: it can exist beside the runnable `.cmd` launcher.
fn candidate_file_names(binary: &str) -> Vec<OsString> {
    candidate_file_names_for(binary, cfg!(windows))
}

fn candidate_file_names_for(binary: &str, windows: bool) -> Vec<OsString> {
    let suffixes: &[&str] = if windows {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    suffixes
        .iter()
        .map(|suffix| OsString::from(format!("{binary}{suffix}")))
        .collect()
}

fn find_on_path_in(directories: &[PathBuf], binary: &str) -> Option<PathBuf> {
    let candidates = candidate_file_names(binary);
    find_candidate_in(directories, &candidates)
}

fn find_candidate_in(directories: &[PathBuf], candidates: &[OsString]) -> Option<PathBuf> {
    for directory in directories {
        for candidate in candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn path_directories() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default()
}

/// Home directory for per-user install lookups. Absolute paths only so a
/// relative `HOME` can never escape into the search list.
fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
            })
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
}

/// Respect relocated Windows local app data, with the normal profile layout
/// as a fallback when the desktop process does not inherit LOCALAPPDATA.
#[cfg(any(windows, test))]
fn windows_agy_directories(home: &Path, local_appdata: Option<&Path>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(local) = local_appdata.filter(|path| path.is_absolute()) {
        directories.push(local.join("agy").join("bin"));
    }
    let default = home.join("AppData").join("Local").join("agy").join("bin");
    if !directories.contains(&default) {
        directories.push(default);
    }
    directories
}

/// Well-known per-user install locations for `home`, without touching the
/// filesystem beyond best-effort directory listings. Callers filter by
/// file existence at lookup time, so missing directories are harmless.
fn fallback_directories_for(home: &Path) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for relative in [
        ".opencode/bin",
        ".local/bin",
        "bin",
        ".cargo/bin",
        ".bun/bin",
        ".deno/bin",
        ".volta/bin",
        ".asdf/shims",
        ".local/share/mise/shims",
        ".config/herd-lite/bin",
        "Library/pnpm/bin",
        // Laravel Herd ships its own PHP/Node on macOS; harmless when absent.
        "Library/Application Support/Herd/bin",
    ] {
        directories.push(home.join(relative));
    }
    #[cfg(windows)]
    {
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        directories.extend(windows_agy_directories(home, local_appdata.as_deref()));
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            if appdata.is_absolute() {
                directories.push(appdata.join("npm"));
            }
        }
    }
    // nvm installs one `bin` directory per Node version (for example npm
    // shims such as `codex`); expand whatever versions exist right now.
    let nvm_nodes = home.join(".nvm").join("versions").join("node");
    if let Ok(entries) = std::fs::read_dir(&nvm_nodes) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .map(|path| path.join("bin"))
            .collect();
        versions.sort();
        directories.extend(versions);
    }
    // Other version managers keep one `bin` per Node release outside the
    // login-shell PATH that GUI launches see. Expand them the same way so a
    // `#!/usr/bin/env node` shim (for example a bun/npm-installed `claude`)
    // resolves its runtime even when the shell probe misses it.
    directories.extend(node_version_bins(home));
    directories
}

/// Versioned Node `bin` directories for managers other than nvm. Each base
/// holds one subdirectory per installed release; only existing `bin`
/// directories are returned.
fn node_version_bins(home: &Path) -> Vec<PathBuf> {
    // (base directory, tail from a version directory to its `bin`)
    let layouts: Vec<(PathBuf, Vec<&str>)> = vec![
        (
            home.join(".local")
                .join("share")
                .join("fnm")
                .join("node-versions"),
            vec!["installation", "bin"],
        ),
        (
            home.join(".fnm").join("node-versions"),
            vec!["installation", "bin"],
        ),
        (
            home.join(".local")
                .join("share")
                .join("mise")
                .join("installs")
                .join("node"),
            vec!["bin"],
        ),
        (
            home.join(".volta").join("tools").join("image").join("node"),
            vec!["bin"],
        ),
        (
            home.join(".asdf").join("installs").join("nodejs"),
            vec!["bin"],
        ),
        (home.join("n").join("versions").join("node"), vec!["bin"]),
    ];
    let mut bins = Vec::new();
    for (base, tail) in layouts {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .map(|path| tail.iter().fold(path, |path, part| path.join(part)))
            .filter(|path| path.is_dir())
            .collect();
        versions.sort();
        bins.extend(versions);
    }
    // `n` also defaults to a system-wide tree outside `$HOME`.
    #[cfg(not(windows))]
    {
        let system_n = PathBuf::from("/usr/local/n/versions/node");
        if let Ok(entries) = std::fs::read_dir(&system_n) {
            let mut versions: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_dir())
                .map(|path| path.join("bin"))
                .filter(|path| path.is_dir())
                .collect();
            versions.sort();
            bins.extend(versions);
        }
    }
    bins
}

/// System-wide locations that GUI launches often miss but interactive shells
/// include. Kept small and constant; no user input is involved.
fn system_fallback_directories() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        Vec::new()
    }
    #[cfg(not(windows))]
    {
        [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            // Unlinked Homebrew Node kegs keep a `node` binary here even when
            // the versioned formula is not linked into `/opt/homebrew/bin`.
            "/opt/homebrew/opt/node/bin",
            "/usr/local/bin",
            "/usr/local/opt/node/bin",
            "/opt/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/home/linuxbrew/.linuxbrew/sbin",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
}

fn fallback_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = home_directory() {
        directories.extend(fallback_directories_for(&home));
    }
    directories.extend(system_fallback_directories());
    directories
}

/// Splits one login-shell `$PATH` dump into directories. Pure helper so the
/// parsing is unit-testable without spawning a shell.
#[cfg_attr(windows, allow(dead_code))]
fn split_shell_path_output(text: &str) -> Vec<PathBuf> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    std::env::split_paths(text.trim()).collect()
}

/// Reads the login shell's `PATH` without executing anything else. Bundled
/// desktop apps launched from Finder/Dock inherit a sparse `PATH` from
/// `launchd`, while terminal sessions see brew, nvm, `~/.local/bin` and
/// `~/.opencode/bin` entries. Only a constant `printf` script ever runs; no
/// user input reaches `argv`. Failures return an empty list so callers fall
/// back to the process `PATH` plus well-known directories.
#[cfg(unix)]
fn shell_path_directories() -> Vec<PathBuf> {
    const SHELL_TIMEOUT: Duration = Duration::from_secs(2);
    const MAX_PATH_BYTES: usize = 32 * 1024;

    let mut shells: Vec<PathBuf> = Vec::new();
    if let Some(configured) = std::env::var_os("SHELL").map(PathBuf::from)
        && configured.is_absolute()
    {
        shells.push(configured);
    }
    for fallback in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let path = PathBuf::from(fallback);
        if !shells.contains(&path) {
            shells.push(path);
        }
    }

    for shell in shells {
        // Fish lists PATH differently; join with `:` so the output splits
        // like POSIX shells. Only a constant script ever runs.
        let probe = if shell.file_name().is_some_and(|name| name == "fish") {
            "string join : $PATH"
        } else {
            "printf %s \"$PATH\""
        };
        let mut command = std::process::Command::new(&shell);
        command
            .args(["-l", "-c", probe])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let Ok(child) = command.spawn() else {
            continue;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        let Ok(Ok(output)) = receiver.recv_timeout(SHELL_TIMEOUT) else {
            continue;
        };
        if !output.status.success() || output.stdout.is_empty() {
            continue;
        }
        if output.stdout.len() > MAX_PATH_BYTES {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let directories = split_shell_path_output(&text);
        if !directories.is_empty() {
            return directories;
        }
    }
    Vec::new()
}

#[cfg(windows)]
fn shell_path_directories() -> Vec<PathBuf> {
    Vec::new()
}

/// Combined lookup order: process `PATH` first (preserves user precedence),
/// then the login-shell `PATH`, then well-known install locations.
/// Duplicates are removed while keeping the first occurrence.
fn search_directories() -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for directory in path_directories()
        .into_iter()
        .chain(shell_path_directories())
        .chain(fallback_directories())
    {
        if seen.insert(directory.clone()) {
            ordered.push(directory);
        }
    }
    ordered
}

fn find_cli_binary_in(directories: &[PathBuf], id: CliProviderId) -> Option<PathBuf> {
    id.binaries()
        .iter()
        .find_map(|binary| find_on_path_in(directories, binary))
}

pub(crate) fn find_cli_binary(id: CliProviderId) -> Option<PathBuf> {
    find_cli_binary_in(&search_directories(), id)
}

fn cli_available_in(directories: &[PathBuf], id: CliProviderId) -> bool {
    find_cli_binary_in(directories, id).is_some()
}

/// Builds the `PATH` value given to spawned CLI children: the tool's own
/// directory first (so sibling runtimes resolve), then every discovery
/// directory. Pure helper so the ordering is unit-testable.
fn build_child_path(parent: Option<&Path>, directories: &[PathBuf]) -> Option<OsString> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for directory in parent
        .into_iter()
        .chain(directories.iter().map(PathBuf::as_path))
    {
        if seen.insert(directory) {
            ordered.push(directory.to_path_buf());
        }
    }
    if ordered.is_empty() {
        return None;
    }
    std::env::join_paths(ordered).ok()
}

/// Directory holding one runtime binary (for example `node`) inside the
/// discovery directories, if present.
fn find_runtime_dir(directories: &[PathBuf], binary: &str) -> Option<PathBuf> {
    find_on_path_in(directories, binary).and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Augmented `PATH` for one tool binary. Installed CLIs are often runtime
/// shims (`claude` installed through bun/npm is a `#!/usr/bin/env node`
/// script, as is `codex`): the bundled app's own sparse environment would
/// make the child fail with "interpreter not found" even though discovery
/// resolved the tool. Same combined directories as lookup, so the child sees
/// at least what discovery saw, with the tool's own directory first and the
/// resolved `node` directory promoted right behind it (a shim in
/// `~/.bun/bin` rarely sits next to its `node` in an nvm/fnm/mise tree).
/// Returns `None` when no value can be built, in which case callers inherit
/// the parent environment unchanged.
pub(crate) fn cli_child_path_env(binary: &Path) -> Option<OsString> {
    let directories = search_directories();
    let mut combined = Vec::new();
    if let Some(node_dir) = find_runtime_dir(&directories, "node") {
        combined.push(node_dir);
    }
    combined.extend(directories);
    build_child_path(binary.parent(), &combined)
}

/// Runs a CLI with fixed arguments and a timeout. Only caller-chosen constant
/// arguments are ever passed; no shell is involved and no user input reaches
/// `argv`. Errors are described generically so no filesystem paths leak into
/// messages or logs.
fn run_cli_output(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, AppError> {
    let mut command = std::process::Command::new(binary);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // The child must resolve its own runtime (for example `node` for the
    // Codex shim) even when the app launched with a sparse GUI environment.
    if let Some(path) = cli_child_path_env(binary) {
        command.env("PATH", path);
    }
    let child = command.spawn().map_err(|_| {
        AppError::new(
            "cli_check_failed",
            "The tool was found but could not be started.",
        )
    })?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    receiver
        .recv_timeout(timeout)
        .map_err(|_| {
            AppError::new(
                "cli_check_timeout",
                "The tool was found but did not respond in time.",
            )
        })?
        .map_err(|_| {
            AppError::new(
                "cli_check_failed",
                "The tool was found but reported an error.",
            )
        })
}

/// Runs `<binary> --version` with a timeout (see [`run_cli_output`]).
fn probe_cli_version(binary: &Path) -> Result<String, AppError> {
    let output = run_cli_output(binary, &["--version"], VERSION_TIMEOUT)?;
    if !output.status.success() {
        return Err(AppError::new(
            "cli_check_failed",
            "The tool was found but reported an error.",
        ));
    }
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    let first_line = text.lines().next().unwrap_or("").trim();
    let version: String = first_line.chars().take(MAX_VERSION_CHARS).collect();
    if version.is_empty() {
        return Err(AppError::new(
            "cli_check_failed",
            "The tool responded without version information.",
        ));
    }
    Ok(version)
}

// ---------------------------------------------------------- validation

fn validate_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim().to_string();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::new(
            "invalid_provider_name",
            "Enter a provider name of 1–64 characters.",
        ));
    }
    Ok(name)
}

pub(crate) fn validate_model(raw: &str) -> Result<String, AppError> {
    let model = raw.trim().to_string();
    if model.is_empty() || model.chars().count() > 128 {
        return Err(AppError::new(
            "invalid_model",
            "Enter a model name of 1–128 characters.",
        ));
    }
    Ok(model)
}

/// `required` on create; on update `None`/blank keeps the stored secret.
fn validate_api_key(raw: Option<&str>, required: bool) -> Result<Option<String>, AppError> {
    match raw.map(str::trim) {
        None | Some("") if !required => Ok(None),
        None | Some("") => Err(AppError::new("invalid_api_key", "Enter an API key.")),
        Some(key) => {
            if key.chars().count() > 2048 || key.contains(['\n', '\r']) {
                return Err(AppError::new(
                    "invalid_api_key",
                    "That API key looks invalid; check for extra line breaks.",
                ));
            }
            Ok(Some(key.to_string()))
        }
    }
}

/// Validates and normalizes an OpenAI-compatible base URL: explicit
/// http(s) scheme, no credentials, no query/fragment, no trailing slash.
pub fn normalize_base_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err(AppError::new(
            "invalid_base_url",
            "Use a full http(s) URL such as https://api.example.com/v1.",
        ));
    }
    let mut url = reqwest::Url::parse(trimmed).map_err(|_| {
        AppError::new(
            "invalid_base_url",
            "Use a full http(s) URL such as https://api.example.com/v1.",
        )
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(AppError::new(
            "invalid_base_url",
            "The URL must start with http:// or https://.",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::new(
            "invalid_base_url",
            "The URL must not contain credentials.",
        ));
    }
    if url.host_str().is_none_or(|host| host.is_empty()) {
        return Err(AppError::new(
            "invalid_base_url",
            "The URL must include a host name.",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::new(
            "invalid_base_url",
            "The URL must not contain ? or # parts.",
        ));
    }
    let stripped = url.path().trim_end_matches('/').to_string();
    url.set_path(if stripped.is_empty() { "/" } else { &stripped });
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// Normalize versioned API bases and full endpoint URLs while preserving proxy prefixes.
/// Anthropic also accepts SDK-style roots; Gemini accepts a full Interactions URL.
pub(crate) fn normalize_provider_base_url(raw: &str, protocol: &str) -> Result<String, AppError> {
    let base = normalize_base_url(raw)?;
    if protocol == "gemini" {
        return Ok(base
            .strip_suffix("/interactions")
            .unwrap_or(&base)
            .to_string());
    }
    if protocol != "anthropic" {
        return Ok(base);
    }
    if let Some(base) = base.strip_suffix("/messages") {
        if !base.ends_with("/v1") {
            return Err(AppError::new(
                "invalid_base_url",
                "Use an Anthropic base URL or a full endpoint ending in /v1/messages.",
            ));
        }
        return Ok(base.to_string());
    }
    if base.ends_with("/v1") {
        Ok(base)
    } else {
        Ok(format!("{base}/v1"))
    }
}

// -------------------------------------------------------------- service

pub fn list_ai_providers(data_dir: &Path) -> Result<AiProviderList, AppError> {
    let conn = open_settings_db(data_dir)?;
    // One combined lookup for all three providers so the login-shell probe
    // runs once instead of once per provider.
    let directories = search_directories();
    let cli = CliProviderId::all()
        .into_iter()
        .map(|id| CliProviderStatus {
            id: id.as_str().to_string(),
            label: id.label().to_string(),
            binaries: id.binaries().iter().map(|name| name.to_string()).collect(),
            available: cli_available_in(&directories, id),
        })
        .collect();
    let custom = ai_providers::list_custom(&conn)?;
    Ok(AiProviderList { cli, custom })
}

pub fn check_ai_cli(data_dir: &Path, provider_id: &str) -> Result<CliCheckResult, AppError> {
    let _ = open_settings_db(data_dir)?;
    let Some(id) = CliProviderId::parse(provider_id.trim()) else {
        return Err(AppError::new(
            "invalid_provider",
            "This provider is not supported.",
        ));
    };
    let Some(binary) = find_cli_binary(id) else {
        return Ok(CliCheckResult {
            id: id.as_str().to_string(),
            label: id.label().to_string(),
            available: false,
            connected: false,
            version: None,
            message:
                "Not found. Install the tool, then reopen Sagar so it can see the new install."
                    .to_string(),
        });
    };
    match probe_cli_version(&binary) {
        Ok(version) => Ok(CliCheckResult {
            id: id.as_str().to_string(),
            label: id.label().to_string(),
            available: true,
            connected: true,
            version: Some(version),
            message: "The tool responded.".to_string(),
        }),
        Err(error) => Ok(CliCheckResult {
            id: id.as_str().to_string(),
            label: id.label().to_string(),
            available: true,
            connected: false,
            version: None,
            message: error.message,
        }),
    }
}

pub fn save_ai_custom_provider(
    data_dir: &Path,
    input: SaveCustomInput,
) -> Result<CustomProviderSummary, AppError> {
    if !matches!(input.protocol.as_str(), "openai" | "gemini" | "anthropic") {
        return Err(AppError::new(
            "invalid_protocol",
            "Choose a supported API format.",
        ));
    }
    let name = validate_name(&input.name)?;
    let base_url = normalize_provider_base_url(&input.base_url, &input.protocol)?;
    let model = validate_model(&input.model)?;
    let creating = input.id.as_deref().is_none_or(|id| id.trim().is_empty());
    let api_key = validate_api_key(input.api_key.as_deref(), creating)?;
    if !creating
        && let Some(id) = input.id.as_deref()
        && id.trim().is_empty()
    {
        return Err(AppError::new(
            "invalid_provider_id",
            "This provider cannot be saved safely.",
        ));
    }

    let conn = open_settings_db(data_dir)?;
    let now = chrono::Utc::now().to_rfc3339();
    if creating {
        let id = uuid::Uuid::new_v4().to_string();
        ai_providers::create_custom(
            &conn,
            &ai_providers::NewCustomProvider {
                protocol: input.protocol.clone(),
                id: id.clone(),
                name,
                base_url,
                api_key: api_key.unwrap_or_default(),
                model,
                created_at: now.clone(),
            },
        )?;
        let list = ai_providers::list_custom(&conn)?;
        list.into_iter()
            .find(|provider| provider.id == id)
            .ok_or_else(|| AppError::new("storage_error", "The provider could not be saved."))
    } else {
        let id = input.id.unwrap_or_default();
        ai_providers::update_custom(
            &conn,
            id.trim(),
            &name,
            &base_url,
            api_key.as_deref(),
            &model,
            &now,
            &input.protocol,
        )
    }
}

pub fn delete_ai_custom_provider(data_dir: &Path, id: &str) -> Result<(), AppError> {
    if id.trim().is_empty() {
        return Err(AppError::new(
            "invalid_provider_id",
            "This provider cannot be removed safely.",
        ));
    }
    let conn = open_settings_db(data_dir)?;
    if !ai_providers::delete_custom(&conn, id.trim())? {
        return Err(AppError::new(
            "provider_not_found",
            "This provider no longer exists in local settings.",
        ));
    }
    Ok(())
}

/// User-triggered connectivity probe: a single `GET {base}/models` with the
/// stored key. No book content is attached to the request.
pub fn test_ai_custom_provider(data_dir: &Path, id: &str) -> Result<CustomTestResult, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::new(
            "invalid_provider_id",
            "This provider cannot be tested safely.",
        ));
    }
    let conn = open_settings_db(data_dir)?;
    let secret = ai_providers::get_secret(&conn, id.trim())?;
    let base_url = normalize_provider_base_url(&secret.base_url, &secret.protocol)?;
    if secret.api_key.trim().is_empty() {
        return Err(AppError::new(
            "invalid_api_key",
            "Add an API key before testing this provider.",
        ));
    }
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(MODELS_TIMEOUT)
        .build()
        .map_err(|_| AppError::new("connection_failed", "The connection test could not start."))?;
    let response = models_request(&client, &base_url, &secret.api_key, &secret.protocol)
        .header("Accept", "application/json")
        .send();
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_timeout() => {
            return Ok(CustomTestResult {
                ok: false,
                model_count: None,
                message: "The connection timed out.".to_string(),
            });
        }
        Err(error) if error.is_connect() => {
            return Ok(CustomTestResult {
                ok: false,
                model_count: None,
                message: "The server could not be reached.".to_string(),
            });
        }
        Err(_) => {
            return Ok(CustomTestResult {
                ok: false,
                model_count: None,
                message: "The connection test failed.".to_string(),
            });
        }
    };
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Ok(CustomTestResult {
            ok: false,
            model_count: None,
            message: "The API key was rejected.".to_string(),
        });
    }
    if status.as_u16() == 404 {
        return Ok(CustomTestResult {
            ok: false,
            model_count: None,
            message: "No model list was found at this URL.".to_string(),
        });
    }
    if !status.is_success() {
        return Ok(CustomTestResult {
            ok: false,
            model_count: None,
            message: format!("The server returned an error (status {}).", status.as_u16()),
        });
    }
    let model_count = response.json::<serde_json::Value>().ok().and_then(|body| {
        body.get(if secret.protocol == "gemini" {
            "models"
        } else {
            "data"
        })?
        .as_array()
        .map(|items| items.len())
    });
    Ok(CustomTestResult {
        ok: true,
        model_count,
        message: match model_count {
            Some(count) => format!("Connected — {count} models listed."),
            None => "Connected.".to_string(),
        },
    })
}

// --------------------------------------------- translation model catalog
//
// The Translation Model tab offers one agent (Codex / OpenCode / Claude Code
// or a saved custom provider) plus a searchable model list. Model discovery
// per agent:
// - Codex: `codex debug models --bundled` (offline) then `codex debug
//   models`, parsed defensively; a small known-models catalog on failure.
// - OpenCode: `opencode models` (local cache, no `--refresh`, so no
//   network), parsed as `provider/model` lines.
// - Claude Code: no machine-readable list exists, so a built-in catalog of
//   aliases and current full IDs; any ID can still be entered manually.
// - Custom: live `GET {base}/models`, falling back to the saved model.
// Listing never sends book content and never executes translation.

/// A translation agent: a built-in CLI or a stored custom provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranslationAgent {
    Cli(CliProviderId),
    Custom(String),
}

impl TranslationAgent {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if let Some(id) = trimmed.strip_prefix("custom:") {
            let id = id.trim();
            if id.is_empty() {
                return None;
            }
            return Some(TranslationAgent::Custom(id.to_string()));
        }
        CliProviderId::parse(trimmed).map(TranslationAgent::Cli)
    }

    /// Stable storage key: `codex` | `opencode` | `claude_code` | `custom:<id>`.
    pub(crate) fn key(&self) -> String {
        match self {
            TranslationAgent::Cli(id) => id.as_str().to_string(),
            TranslationAgent::Custom(id) => format!("custom:{id}"),
        }
    }
}

fn invalid_agent() -> AppError {
    AppError::new(
        "invalid_agent",
        "Choose Codex, OpenCode, Claude Code or a saved custom provider.",
    )
}

/// Known Codex models, used only when the live catalog cannot be read. Model
/// availability changes; the live `codex debug models` listing wins whenever
/// the tool responds.
const CODEX_KNOWN_MODELS: &[(&str, &str)] = &[
    (
        "gpt-5.6-sol",
        "Most capable coding, research and computer use",
    ),
    ("gpt-5.6-terra", "Balanced everyday work"),
    ("gpt-5.6-luna", "Fast and affordable"),
    ("gpt-5.3-codex-spark", "Near-instant iteration"),
];

/// Claude Code offers no machine-readable model list, so this built-in
/// catalog of aliases and current full IDs is the source. Any model ID can
/// still be entered manually.
const CLAUDE_KNOWN_MODELS: &[(&str, &str)] = &[
    (
        "sonnet",
        "Sonnet alias — best speed and intelligence balance",
    ),
    ("opus", "Opus alias — most capable"),
    ("haiku", "Haiku alias — fastest"),
    ("fable", "Fable alias — long-running agents"),
    ("claude-sonnet-5", "Sonnet 5"),
    ("claude-opus-5-5", "Opus 5.5"),
    ("claude-opus-5", "Opus 5"),
    ("claude-haiku-4-5", "Haiku 4.5"),
    ("claude-opus-4-8", "Opus 4.8"),
    ("claude-opus-4-7", "Opus 4.7"),
    ("claude-sonnet-4-6", "Sonnet 4.6"),
    ("claude-opus-4-6", "Opus 4.6"),
    ("claude-fable-5-1", "Fable 5.1"),
];

fn known_models(entries: &[(&str, &str)]) -> Vec<TranslationModel> {
    entries
        .iter()
        .map(|(id, detail)| TranslationModel {
            id: id.to_string(),
            label: id.to_string(),
            detail: Some(detail.to_string()),
            reasoning_efforts: None,
        })
        .collect()
}

/// Validates a model ID from any source: non-empty, bounded, no control
/// characters or line breaks.
fn clean_model_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    if id.is_empty() || id.chars().count() > MAX_MODEL_ID_CHARS {
        return None;
    }
    if id.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(id.to_string())
}

fn push_unique(models: &mut Vec<TranslationModel>, seen: &mut HashSet<String>, id: String) {
    if seen.insert(id.clone()) {
        models.push(TranslationModel {
            id: id.clone(),
            label: id,
            detail: None,
            reasoning_efforts: None,
        });
    }
}

/// Extracts the JSON payload from CLI output that may include log lines
/// around the document.
fn extract_json_document(text: &str) -> Option<&str> {
    let start = text.find(['{', '['])?;
    let end = text.rfind(['}', ']'])?;
    if end < start {
        return None;
    }
    Some(&text[start..=end])
}

fn codex_entry(value: &serde_json::Value) -> Option<TranslationModel> {
    match value {
        serde_json::Value::String(id) => clean_model_id(id).map(|id| TranslationModel {
            id: id.clone(),
            label: id,
            detail: None,
            reasoning_efforts: None,
        }),
        serde_json::Value::Object(map) => {
            let id = ["id", "slug", "model", "name"]
                .iter()
                .find_map(|key| map.get(*key)?.as_str())?;
            let id = clean_model_id(id)?;
            let label = ["display_name", "label"]
                .iter()
                .find_map(|key| map.get(*key)?.as_str())
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| id.clone());
            let detail = map
                .get("description")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|detail| !detail.is_empty())
                .map(|detail| detail.chars().take(140).collect());
            let reasoning_efforts = map
                .get("supported_reasoning_levels")
                .and_then(|v| v.as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|v| v.as_str().or_else(|| v.get("effort")?.as_str()))
                        .filter(|v| valid_effort_token(v))
                        .map(str::to_string)
                        .collect()
                });
            Some(TranslationModel {
                id,
                label,
                detail,
                reasoning_efforts,
            })
        }
        _ => None,
    }
}

/// Parses `codex debug models` JSON defensively: the command is experimental
/// and its shape may vary. Accepts arrays of strings/objects and objects with
/// `models`/`data` arrays or maps.
fn parse_codex_catalog(text: &str) -> Vec<TranslationModel> {
    let document = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .or_else(|| extract_json_document(text).and_then(|doc| serde_json::from_str(doc).ok()));
    let Some(body) = document else {
        return Vec::new();
    };
    let mut entries: Vec<&serde_json::Value> = Vec::new();
    let mut mapped: Vec<TranslationModel> = Vec::new();
    match &body {
        serde_json::Value::Array(items) => entries.extend(items.iter()),
        serde_json::Value::Object(map) => {
            for key in ["models", "data", "catalog"] {
                if let Some(serde_json::Value::Array(items)) = map.get(key) {
                    entries.extend(items.iter());
                    break;
                }
            }
            if entries.is_empty()
                && let Some(serde_json::Value::Object(models)) = map.get("models")
            {
                let mut seen = HashSet::new();
                for id in models.keys() {
                    if let Some(id) = clean_model_id(id) {
                        push_unique(&mut mapped, &mut seen, id);
                    }
                    if mapped.len() >= MAX_MODELS {
                        break;
                    }
                }
                return mapped;
            }
        }
        _ => return Vec::new(),
    }
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for item in entries {
        if let Some(entry) = codex_entry(item)
            && seen.insert(entry.id.clone())
        {
            models.push(entry);
        }
        if models.len() >= MAX_MODELS {
            break;
        }
    }
    models
}

/// Parses the OpenCode V2 model catalog returned by `opencode api model.list`.
///
/// The document is `{ "location": …, "data": [ … ] }` and every entry carries
/// the provider, the model id, a display name and the advertised variants.
/// Older V1 output was a plain `provider/model` list, so that shape is still
/// accepted as a fallback.
fn parse_opencode_catalog(text: &str) -> Vec<TranslationModel> {
    let document = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .or_else(|| extract_json_document(text).and_then(|doc| serde_json::from_str(doc).ok()));
    if let Some(body) = document {
        let entries = body
            .get("data")
            .and_then(|value| value.as_array())
            .or_else(|| body.as_array());
        if let Some(entries) = entries {
            let mut models = Vec::new();
            let mut seen = HashSet::new();
            for entry in entries {
                let Some(provider) = entry["providerID"].as_str() else {
                    continue;
                };
                let Some(model) = entry["modelID"]
                    .as_str()
                    .filter(|model| !model.contains('/'))
                else {
                    continue;
                };
                let Some(id) = clean_model_id(&format!("{provider}/{model}")) else {
                    continue;
                };
                if !seen.insert(id.clone()) {
                    continue;
                }
                let detail = entry["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(|name| name.chars().take(140).collect());
                let reasoning_efforts = entry["variants"].as_array().map(|variants| {
                    variants
                        .iter()
                        .filter_map(|variant| variant["id"].as_str().or_else(|| variant.as_str()))
                        .filter(|token| valid_effort_token(token))
                        .map(str::to_string)
                        .collect()
                });
                models.push(TranslationModel {
                    label: id.clone(),
                    id,
                    detail,
                    reasoning_efforts,
                });
                if models.len() >= MAX_MODELS {
                    break;
                }
            }
            if !models.is_empty() {
                return models;
            }
        }
    }
    parse_opencode_models(text)
}

/// Parses `opencode models` output: one `provider/model` reference per line.
/// Lines without a slash (headers, hints) are skipped.
fn parse_opencode_models(text: &str) -> Vec<TranslationModel> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let line = line.trim().trim_matches('"').trim();
        if line.is_empty() || !line.contains('/') {
            continue;
        }
        if let Some(id) = clean_model_id(line) {
            push_unique(&mut models, &mut seen, id);
        }
        if models.len() >= MAX_MODELS {
            break;
        }
    }
    models
}

fn valid_effort_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn claude_efforts(model: &str) -> Vec<String> {
    let name = model.to_ascii_lowercase();
    let levels: &[&str] = if name.contains("4-6") || name.contains("4.6") {
        &["low", "medium", "high", "max"]
    } else if matches!(name.as_str(), "opus" | "sonnet" | "fable")
        || name.contains("fable-5")
        || name.contains("opus-5")
        || name.contains("sonnet-5")
        || (name.contains("haiku-5-5") || name.contains("haiku-5.5"))
        || name.contains("opus-4-7")
        || name.contains("opus-4-8")
    {
        &["low", "medium", "high", "xhigh", "max"]
    } else {
        &[]
    };
    levels.iter().map(|v| v.to_string()).collect()
}

/// User-selectable API effort overrides, independent of provider/model identity.
/// The endpoint decides which values it accepts; Default omits the parameter.
fn custom_reasoning_efforts() -> Vec<String> {
    [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect()
}

/// Validate at both save and execution boundaries. Default passes no override.
pub fn validate_reasoning_effort(
    data_dir: &Path,
    agent: &str,
    model: &str,
    effort: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(effort) = effort.filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let invalid = || {
        AppError::new(
            "invalid_reasoning_effort",
            "Choose a supported reasoning effort for this agent and model, or use Default.",
        )
    };
    if !valid_effort_token(effort) {
        return Err(invalid());
    }
    let supported = if agent == "agy" {
        Some(vec!["low".into(), "medium".into(), "high".into()])
    } else if agent == "claude_code" {
        Some(claude_efforts(model))
    } else if matches!(agent, "codex" | "opencode" | "qoder") {
        list_translation_models(data_dir, agent)?
            .models
            .into_iter()
            .find(|entry| entry.id == model)
            .and_then(|entry| entry.reasoning_efforts)
    } else if let Some(TranslationAgent::Custom(id)) = TranslationAgent::parse(agent) {
        let conn = open_settings_db(data_dir)?;
        let secret = ai_providers::get_secret(&conn, &id)?;
        Some(protocol_efforts(&secret.protocol))
    } else {
        return Err(invalid());
    };
    if let Some(levels) = supported {
        if !levels.iter().any(|level| level == effort) {
            return Err(invalid());
        }
    } else if matches!(agent, "opencode" | "qoder")
        || ![
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ]
        .contains(&effort)
    {
        return Err(invalid());
    }
    Ok(Some(effort.to_string()))
}

fn parse_agy_models(text: &str) -> Vec<TranslationModel> {
    let mut seen = HashSet::new();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let id = clean_model_id(parts.next()?)?;
            if !id.contains('-')
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
                || !seen.insert(id.clone())
            {
                return None;
            }
            let label = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&id)
                .to_string();
            Some(TranslationModel {
                id,
                label,
                detail: None,
                reasoning_efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
            })
        })
        .take(MAX_MODELS)
        .collect()
}

/// Parses an OpenAI-compatible `/models` body (`{ data: [{ id }] }`).
fn parse_openai_model_list(body: &serde_json::Value) -> Vec<TranslationModel> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    if let Some(items) = body.get("data").and_then(|data| data.as_array()) {
        for item in items {
            let Some(id) = item.get("id").and_then(|id| id.as_str()) else {
                continue;
            };
            let Some(id) = clean_model_id(id) else {
                continue;
            };
            if seen.insert(id.clone()) {
                let detail = item
                    .get("owned_by")
                    .and_then(|owner| owner.as_str())
                    .map(str::trim)
                    .filter(|owner| !owner.is_empty())
                    .map(|owner| format!("by {owner}"));
                models.push(TranslationModel {
                    id: id.clone(),
                    label: id,
                    detail,
                    reasoning_efforts: None,
                });
            }
            if models.len() >= MAX_MODELS {
                break;
            }
        }
    }
    models
}

fn protocol_efforts(protocol: &str) -> Vec<String> {
    if protocol == "openai" {
        custom_reasoning_efforts()
    } else {
        Vec::new()
    }
}

fn models_request(
    client: &reqwest::blocking::Client,
    base: &str,
    key: &str,
    protocol: &str,
) -> reqwest::blocking::RequestBuilder {
    let request = client.get(format!("{base}/models"));
    match protocol {
        "gemini" => request.header("x-goog-api-key", key),
        "anthropic" => request
            .bearer_auth(key)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(key),
    }
}

fn parse_api_model_list(body: &serde_json::Value, protocol: &str) -> Vec<TranslationModel> {
    if protocol != "gemini" {
        return parse_openai_model_list(body);
    }
    let data: Vec<serde_json::Value> = body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
        })
        .filter_map(|item| {
            item["name"]
                .as_str()
                .map(|name| serde_json::json!({"id":name.strip_prefix("models/").unwrap_or(name)}))
        })
        .collect();
    parse_openai_model_list(&serde_json::json!({"data":data}))
}

/// Fetches a live model list using the saved API protocol. Carries only
/// the stored key; no book content is attached.
fn fetch_api_models(
    base_url: &str,
    api_key: &str,
    protocol: &str,
) -> Result<Vec<TranslationModel>, AppError> {
    let base_url = normalize_provider_base_url(base_url, protocol)?;
    if api_key.trim().is_empty() {
        return Err(AppError::new(
            "invalid_api_key",
            "Add an API key before listing models.",
        ));
    }
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(MODELS_TIMEOUT)
        .build()
        .map_err(|_| AppError::new("connection_failed", "The model list could not be loaded."))?;
    let response = models_request(&client, &base_url, api_key, protocol)
        .header("Accept", "application/json")
        .send()
        .map_err(|_| AppError::new("connection_failed", "The model list could not be loaded."))?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "models_unavailable",
            "The provider did not return a model list.",
        ));
    }
    response
        .json::<serde_json::Value>()
        .map(|body| parse_api_model_list(&body, protocol))
        .map_err(|_| AppError::new("models_unavailable", "The model list could not be read."))
}

fn find_cli_or_unavailable(id: CliProviderId) -> Result<PathBuf, AppError> {
    find_cli_binary(id).ok_or_else(|| {
        AppError::new(
            "provider_not_available",
            format!(
                "{} was not found. Install the tool, then reopen Sagar to list its models.",
                id.label()
            ),
        )
    })
}

pub fn list_translation_models(
    data_dir: &Path,
    agent: &str,
) -> Result<TranslationModelList, AppError> {
    // Ensure migrations ran so custom-provider lookups never hit a missing table.
    let conn = open_settings_db(data_dir)?;
    let Some(parsed) = TranslationAgent::parse(agent) else {
        return Err(invalid_agent());
    };
    match parsed {
        TranslationAgent::Cli(CliProviderId::Codex) => {
            let binary = find_cli_or_unavailable(CliProviderId::Codex)?;
            // Offline-first: the bundled catalog needs no network.
            for args in [
                &["debug", "models", "--bundled"][..],
                &["debug", "models"][..],
            ] {
                if let Ok(output) = run_cli_output(&binary, args, MODEL_LIST_TIMEOUT)
                    && output.status.success()
                {
                    let models = parse_codex_catalog(&String::from_utf8_lossy(&output.stdout));
                    if !models.is_empty() {
                        return Ok(TranslationModelList {
                            agent: CliProviderId::Codex.as_str().to_string(),
                            source: "live".to_string(),
                            models,
                            message: "Live catalog from the Codex tool.".to_string(),
                        });
                    }
                }
            }
            Ok(TranslationModelList {
                agent: CliProviderId::Codex.as_str().to_string(),
                source: "catalog".to_string(),
                models: known_models(CODEX_KNOWN_MODELS),
                message: "The live catalog could not be read; showing known models.".to_string(),
            })
        }
        TranslationAgent::Cli(CliProviderId::Opencode) => {
            let binary = find_cli_or_unavailable(CliProviderId::Opencode)?;
            // OpenCode V2 dropped the `--verbose` flag and now serves the
            // catalog from its local server. `opencode api` performs service
            // discovery and authentication itself and never leaves the
            // machine. The plain `models` listing stays as a fallback.
            let catalog = run_cli_output(&binary, &["api", "model.list"], MODEL_LIST_TIMEOUT)
                .ok()
                .filter(|output| output.status.success())
                .map(|output| parse_opencode_catalog(&String::from_utf8_lossy(&output.stdout)))
                .filter(|models| !models.is_empty())
                .or_else(|| {
                    run_cli_output(&binary, &["models"], MODEL_LIST_TIMEOUT)
                        .ok()
                        .filter(|output| output.status.success())
                        .map(|output| {
                            parse_opencode_catalog(&String::from_utf8_lossy(&output.stdout))
                        })
                        .filter(|models| !models.is_empty())
                });
            match catalog {
                Some(models) => Ok(TranslationModelList {
                    agent: CliProviderId::Opencode.as_str().to_string(),
                    source: "live".to_string(),
                    models,
                    message: "Live catalog from the OpenCode tool.".to_string(),
                }),
                None => Err(AppError::new(
                    "models_unavailable",
                    "The model list could not be read from the tool.",
                )),
            }
        }
        TranslationAgent::Cli(CliProviderId::Qoder) => {
            let binary = find_cli_or_unavailable(CliProviderId::Qoder)?;
            let models = super::qoder::list_models(&binary, MODEL_LIST_TIMEOUT)?;
            Ok(TranslationModelList {
                agent: "qoder".into(),
                source: "live".into(),
                models,
                message: "Live models and supported reasoning efforts from Qoder.".into(),
            })
        }
        TranslationAgent::Cli(CliProviderId::Agy) => {
            let binary = find_cli_or_unavailable(CliProviderId::Agy)?;
            let output = run_cli_output(&binary, &["models"], MODEL_LIST_TIMEOUT)?;
            if !output.status.success() {
                return Err(AppError::new(
                    "models_unavailable",
                    "Antigravity models could not be loaded. Authenticate in agy first, then retry.",
                ));
            }
            Ok(TranslationModelList {
                agent: "agy".into(),
                source: "live".into(),
                models: parse_agy_models(&String::from_utf8_lossy(&output.stdout)),
                message: "Live list from agy; a model ID can also be entered manually.".into(),
            })
        }
        TranslationAgent::Cli(CliProviderId::ClaudeCode) => Ok(TranslationModelList {
            agent: CliProviderId::ClaudeCode.as_str().to_string(),
            source: "catalog".to_string(),
            models: known_models(CLAUDE_KNOWN_MODELS)
                .into_iter()
                .map(|mut model| {
                    model.reasoning_efforts = Some(claude_efforts(&model.id));
                    model
                })
                .collect(),
            message: "Known models for Claude Code; any model ID can also be entered manually."
                .to_string(),
        }),
        TranslationAgent::Custom(id) => {
            let secret = ai_providers::get_secret(&conn, &id)?;
            match fetch_api_models(&secret.base_url, &secret.api_key, &secret.protocol) {
                Ok(mut models) if !models.is_empty() => {
                    for model in &mut models {
                        model.reasoning_efforts = Some(protocol_efforts(&secret.protocol));
                    }
                    Ok(TranslationModelList {
                        agent: format!("custom:{id}"),
                        source: "live".to_string(),
                        models,
                        message: "Live list from the provider.".to_string(),
                    })
                }
                _ => {
                    let fallback = clean_model_id(&secret.model);
                    Ok(TranslationModelList {
                        agent: format!("custom:{id}"),
                        source: "catalog".to_string(),
                        models: fallback
                            .map(|id| TranslationModel {
                                id: id.clone(),
                                label: id,
                                detail: Some("Saved model".to_string()),
                                reasoning_efforts: Some(protocol_efforts(&secret.protocol)),
                            })
                            .into_iter()
                            .collect(),
                        message: "The live list could not be reached; showing the saved model."
                            .to_string(),
                    })
                }
            }
        }
    }
}

pub fn get_translation_selection(
    data_dir: &Path,
) -> Result<Option<TranslationSelection>, AppError> {
    let conn = open_settings_db(data_dir)?;
    ai_providers::get_translation_selection(&conn)
}

/// Persists the translation agent + model choice for future translation work.
/// Selecting never starts translation and never sends book content anywhere.
pub fn set_translation_selection(
    data_dir: &Path,
    agent: &str,
    model: &str,
    reasoning_effort: Option<&str>,
) -> Result<TranslationSelection, AppError> {
    let Some(parsed) = TranslationAgent::parse(agent) else {
        return Err(invalid_agent());
    };
    let model = validate_model(model)?;
    let conn = open_settings_db(data_dir)?;
    if let TranslationAgent::Custom(id) = &parsed {
        // The referenced provider must still exist.
        ai_providers::get_secret(&conn, id)?;
    }
    let effort = validate_reasoning_effort(data_dir, &parsed.key(), &model, reasoning_effort)?;
    let now = chrono::Utc::now().to_rfc3339();
    ai_providers::set_translation_selection(&conn, &parsed.key(), &model, effort.as_deref(), &now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_base_urls_accept_sdk_roots_and_full_endpoints() {
        for (input, expected) in [
            ("https://api.anthropic.com", "https://api.anthropic.com/v1"),
            (
                "https://api.anthropic.com/v1/",
                "https://api.anthropic.com/v1",
            ),
            (
                "https://api.anthropic.com/v1/messages",
                "https://api.anthropic.com/v1",
            ),
            (
                "https://api.deepseek.com/anthropic",
                "https://api.deepseek.com/anthropic/v1",
            ),
            (
                "https://api.deepseek.com/anthropic/v1/messages/",
                "https://api.deepseek.com/anthropic/v1",
            ),
            (
                "http://localhost:8000/proxy/v1/messages",
                "http://localhost:8000/proxy/v1",
            ),
        ] {
            assert_eq!(
                normalize_provider_base_url(input, "anthropic").unwrap(),
                expected
            );
        }
        assert_eq!(
            normalize_provider_base_url("https://example.invalid/openai", "openai").unwrap(),
            "https://example.invalid/openai"
        );
        assert!(
            normalize_provider_base_url(
                "https://example.invalid/v1/messages?key=secret",
                "anthropic"
            )
            .is_err()
        );
    }

    #[test]
    fn native_model_discovery_and_connection_test_use_protocol_auth() {
        use std::io::{Read, Write};
        for protocol in ["gemini", "anthropic"] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!(
                "http://{}/v1{}",
                listener.local_addr().unwrap(),
                if protocol == "gemini" {
                    "/interactions"
                } else {
                    ""
                }
            );
            let body = if protocol == "gemini" {
                serde_json::json!({"models":[{"name":"models/gemini-test","supportedGenerationMethods":["generateContent"]},{"name":"models/embedding","supportedGenerationMethods":["embedContent"]}]})
            } else { serde_json::json!({"data":[{"id":"claude-test"}]}) }.to_string();
            let server = std::thread::spawn(move || {
                for _ in 0..2 {
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
                    let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                    assert!(request.starts_with("get /v1/models http/1.1"));
                    assert!(request.contains(if protocol == "gemini" {
                        "x-goog-api-key: private-key"
                    } else {
                        "x-api-key: private-key"
                    }));
                    if protocol == "anthropic" {
                        assert!(request.contains("authorization: bearer private-key"));
                    } else {
                        assert!(!request.contains("authorization:"));
                    }
                    if protocol == "anthropic" {
                        assert!(request.contains("anthropic-version: 2023-06-01"));
                    }
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                }
            });
            let dir = tempfile::tempdir().unwrap();
            let saved = save_ai_custom_provider(
                dir.path(),
                SaveCustomInput {
                    protocol: protocol.into(),
                    id: None,
                    name: "Native API".into(),
                    base_url: base,
                    api_key: Some("private-key".into()),
                    model: "saved-model".into(),
                },
            )
            .unwrap();
            assert_eq!(saved.protocol, protocol);
            assert!(test_ai_custom_provider(dir.path(), &saved.id).unwrap().ok);
            let agent = format!("custom:{}", saved.id);
            let models = list_translation_models(dir.path(), &agent).unwrap();
            assert_eq!(models.source, "live");
            assert_eq!(models.models.len(), 1);
            assert_eq!(
                models.models[0].id,
                if protocol == "gemini" {
                    "gemini-test"
                } else {
                    "claude-test"
                }
            );
            assert_eq!(models.models[0].reasoning_efforts, Some(vec![]));
            assert!(
                validate_reasoning_effort(dir.path(), &agent, "saved-model", Some("low")).is_err()
            );
            assert_eq!(
                validate_reasoning_effort(dir.path(), &agent, "saved-model", None).unwrap(),
                None
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn qoder_selection_and_binary_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let selection = set_translation_selection(dir.path(), "qoder", "dfmodel", None).unwrap();
        assert_eq!(selection.agent, "qoder");
        assert_eq!(
            get_translation_selection(dir.path())
                .unwrap()
                .unwrap()
                .model,
            "dfmodel"
        );
        assert!(
            validate_reasoning_effort(dir.path(), "qoder", "dfmodel", Some("bad effort")).is_err()
        );
        assert_eq!(CliProviderId::parse("qoder"), Some(CliProviderId::Qoder));
        assert!(CliProviderId::all().contains(&CliProviderId::Qoder));
        let binary = dir
            .path()
            .join(format!("qoder{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, b"stub").unwrap();
        assert_eq!(
            find_cli_binary_in(&[dir.path().into()], CliProviderId::Qoder),
            Some(binary)
        );
    }

    #[test]
    fn agy_models_selection_and_discovery() {
        let models = parse_agy_models(
            "Available models:\ngemini-example-high  Gemini Example (High)\ngemini-example-high Duplicate\n",
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "Gemini Example (High)");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path()
                .join(format!("agy{}", std::env::consts::EXE_SUFFIX)),
            b"stub",
        )
        .unwrap();
        assert!(cli_available_in(
            &[dir.path().to_path_buf()],
            CliProviderId::Agy
        ));
        let saved =
            set_translation_selection(dir.path(), "agy", "gemini-example-high", Some("high"))
                .unwrap();
        assert_eq!(
            get_translation_selection(dir.path())
                .unwrap()
                .unwrap()
                .agent,
            saved.agent
        );
        assert!(validate_reasoning_effort(dir.path(), "agy", "m", Some("xhigh")).is_err());
    }

    #[test]
    fn custom_effort_works_for_any_provider_and_model() {
        let dir = tempfile::tempdir().unwrap();
        for base_url in [
            "https://api.z.ai/api/paas/v4",
            "https://compatible.example/v1",
            "http://localhost:1234/v1",
        ] {
            let provider = save_ai_custom_provider(
                dir.path(),
                SaveCustomInput {
                    protocol: "openai".into(),
                    id: None,
                    name: "Custom".into(),
                    base_url: base_url.into(),
                    api_key: Some("synthetic".into()),
                    model: "saved-model".into(),
                },
            )
            .unwrap();
            let agent = format!("custom:{}", provider.id);
            for model in ["glm-5.3", "glm-5.3-flash", "arbitrary-manual-model"] {
                for effort in custom_reasoning_efforts() {
                    let saved = set_translation_selection(dir.path(), &agent, model, Some(&effort))
                        .unwrap();
                    assert_eq!(saved.reasoning_effort.as_deref(), Some(effort.as_str()));
                    assert_eq!(
                        validate_reasoning_effort(dir.path(), &agent, model, Some(&effort))
                            .unwrap(),
                        Some(effort)
                    );
                }
                assert_eq!(
                    validate_reasoning_effort(dir.path(), &agent, model, None).unwrap(),
                    None
                );
                assert!(
                    validate_reasoning_effort(dir.path(), &agent, model, Some("invalid")).is_err()
                );
            }
        }
    }

    #[test]
    fn codex_catalog_reads_slug_and_advertised_efforts() {
        let models = parse_codex_catalog(
            r#"{"models":[{"slug":"gpt-example","supported_reasoning_levels":[{"effort":"low"},{"effort":"xhigh"}]}]}"#,
        );
        assert_eq!(models[0].id, "gpt-example");
        assert_eq!(
            models[0].reasoning_efforts.as_ref().expect("efforts"),
            &["low", "xhigh"]
        );
    }

    #[test]
    fn opencode_catalog_reads_v2_provider_model_and_variants() {
        let models = parse_opencode_catalog(
            r#"{"location":{"directory":"/tmp"},"data":[
                {"id":"example","modelID":"example","providerID":"openai","name":"Example Model",
                 "settings":{"baseURL":"https://example.invalid"},
                 "variants":[{"id":"low","settings":{"reasoningEffort":"low"}},
                             {"id":"xhigh","settings":{"reasoningEffort":"xhigh"}}]},
                {"id":"plain","modelID":"plain","providerID":"opencode","name":"Plain","variants":[]},
                {"id":"skipped","providerID":"openai"}
            ]}"#,
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "openai/example");
        assert_eq!(models[0].label, "openai/example");
        assert_eq!(models[0].detail.as_deref(), Some("Example Model"));
        assert_eq!(
            models[0].reasoning_efforts.as_ref().expect("variants"),
            &["low", "xhigh"]
        );
        assert_eq!(models[1].id, "opencode/plain");
        assert!(
            models[1]
                .reasoning_efforts
                .as_ref()
                .expect("empty variants")
                .is_empty()
        );
    }

    #[test]
    fn opencode_catalog_falls_back_to_plain_provider_model_lines() {
        let models = parse_opencode_catalog("anthropic/claude-sonnet-4-5\nopenai/gpt-5.2\n");
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, vec!["anthropic/claude-sonnet-4-5", "openai/gpt-5.2"]);
    }

    #[test]
    #[ignore = "explicit probe against the installed OpenCode CLI; reads the local catalog only"]
    fn installed_opencode_lists_a_parsable_v2_catalog() {
        let binary = find_cli_binary(crate::domain::CliProviderId::Opencode)
            .expect("OpenCode CLI must be installed");
        let output = run_cli_output(&binary, &["api", "model.list"], MODEL_LIST_TIMEOUT)
            .expect("opencode api model.list");
        assert!(output.status.success(), "opencode api model.list failed");
        let models = parse_opencode_catalog(&String::from_utf8_lossy(&output.stdout));
        assert!(!models.is_empty(), "no models parsed from the live catalog");
        assert!(
            models
                .iter()
                .all(|model| model.id.contains('/') && model.id.split('/').count() == 2),
            "every model id must be provider/model: {:?}",
            models.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
        eprintln!(
            "OpenCode live catalog: {} models, first {}",
            models.len(),
            models[0].id
        );
    }

    #[test]
    fn effort_validation_rejects_invalid_values_and_unsupported_claude_models() {
        let dir = tempfile::tempdir().expect("temp");
        assert_eq!(
            validate_reasoning_effort(dir.path(), "claude_code", "claude-opus-4-6", Some("high"))
                .expect("high"),
            Some("high".into())
        );
        assert!(
            validate_reasoning_effort(dir.path(), "claude_code", "claude-opus-4-6", Some("xhigh"))
                .is_err()
        );
        assert!(
            validate_reasoning_effort(dir.path(), "claude_code", "claude-haiku-4-5", Some("low"))
                .is_err()
        );
        assert!(validate_reasoning_effort(dir.path(), "codex", "m", Some("high\n--flag")).is_err());
        assert!(validate_reasoning_effort(dir.path(), "custom:p", "m", Some("high")).is_err());
        assert_eq!(
            validate_reasoning_effort(dir.path(), "opencode", "m", None).expect("default"),
            None
        );
    }

    #[test]
    fn base_url_normalization_keeps_the_path_and_rejects_risky_urls() {
        assert_eq!(
            normalize_base_url("https://api.example.com/v1/").expect("trailing slash"),
            "https://api.example.com/v1"
        );
        assert_eq!(
            normalize_base_url("http://localhost:1234/v1").expect("local http"),
            "http://localhost:1234/v1"
        );
        for rejected in [
            "api.example.com/v1",
            "ftp://example.com/v1",
            "https://user:pass@example.com/v1",
            "https://example.com/v1?key=secret",
            "https://example.com/v1#models",
            "https://",
            "",
        ] {
            assert!(
                normalize_base_url(rejected).is_err(),
                "must reject {rejected}"
            );
        }
    }

    #[test]
    fn provider_field_limits_are_enforced() {
        assert!(validate_name("  ").is_err());
        assert!(validate_name(&"n".repeat(65)).is_err());
        assert!(validate_model("").is_err());
        assert!(validate_api_key(None, true).is_err());
        assert!(
            validate_api_key(Some(""), false)
                .expect("blank keeps")
                .is_none()
        );
        assert!(validate_api_key(Some("line\nbreak"), true).is_err());
    }

    #[test]
    fn path_lookup_finds_staged_binaries_without_a_shell() {
        let dir = tempfile::tempdir().expect("temp dir");
        let binary = dir
            .path()
            .join(format!("codex{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, b"stub").expect("stage binary");
        let found = find_on_path_in(&[dir.path().to_path_buf()], "codex").expect("binary found");
        assert_eq!(found, binary);
        assert!(find_on_path_in(&[dir.path().to_path_buf()], "ghost").is_none());
        assert!(find_on_path_in(&[], "codex").is_none());
    }

    #[test]
    fn windows_lookup_skips_npm_posix_shims_and_prefers_executables() {
        let dir = tempfile::tempdir().expect("temp dir");
        let directories = vec![dir.path().to_path_buf()];
        for tool in ["codex", "opencode", "claude", "agy"] {
            let candidates = candidate_file_names_for(tool, true);
            std::fs::write(dir.path().join(tool), b"#!/bin/sh\n").unwrap();
            assert_eq!(find_candidate_in(&directories, &candidates), None);
            for extension in ["bat", "cmd", "exe"] {
                let launcher = dir.path().join(format!("{tool}.{extension}"));
                std::fs::write(&launcher, b"stub").unwrap();
                assert_eq!(find_candidate_in(&directories, &candidates), Some(launcher));
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_check_launches_cmd_beside_posix_shim() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bin = dir.path().join("npm with spaces");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("codex"), b"#!/bin/sh\nexit 1\n").unwrap();
        std::fs::write(
            bin.join("codex.cmd"),
            b"@echo off\r\necho codex-test 1.0\r\n",
        )
        .unwrap();
        let binary = find_cli_binary_in(&[bin.clone()], CliProviderId::Codex).unwrap();
        assert_eq!(binary, bin.join("codex.cmd"));
        assert_eq!(probe_cli_version(&binary).unwrap(), "codex-test 1.0");
    }

    #[test]
    fn windows_agy_lookup_uses_local_appdata_and_profile_fallback() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("profile with spaces");
        let local = dir.path().join("relocated local data");
        let default_bin = home.join("AppData/Local/agy/bin");
        let relocated_bin = local.join("agy/bin");
        let directories = windows_agy_directories(&home, Some(&local));
        assert_eq!(
            directories,
            vec![relocated_bin.clone(), default_bin.clone()]
        );
        assert_eq!(
            windows_agy_directories(&home, None),
            vec![default_bin.clone()]
        );
        assert_eq!(
            windows_agy_directories(&home, Some(Path::new("relative"))),
            vec![default_bin.clone()]
        );
        assert_eq!(
            windows_agy_directories(&home, Some(&home.join("AppData/Local"))),
            vec![default_bin.clone()]
        );
        // Exercise real discovery without relying on this test process's PATH.
        let filename = format!("agy{}", std::env::consts::EXE_SUFFIX);
        for bin in [default_bin, relocated_bin] {
            std::fs::create_dir_all(&bin).expect("stage install directory");
            let binary = bin.join(&filename);
            std::fs::write(&binary, b"stub").expect("stage agy");
            assert_eq!(find_on_path_in(&directories, "agy"), Some(binary));
        }
    }

    #[test]
    fn saving_rejects_invalid_input_before_storage() {
        let dir = tempfile::tempdir().expect("temp dir");
        let error = save_ai_custom_provider(
            dir.path(),
            SaveCustomInput {
                protocol: "openai".into(),
                id: None,
                name: "Local".into(),
                base_url: "not-a-url".into(),
                api_key: Some("key".into()),
                model: "m".into(),
            },
        )
        .expect_err("bad URL rejected");
        assert_eq!(error.code, "invalid_base_url");

        let created = save_ai_custom_provider(
            dir.path(),
            SaveCustomInput {
                protocol: "openai".into(),
                id: None,
                name: "Local".into(),
                base_url: "http://localhost:1234/v1/".into(),
                api_key: Some("key".into()),
                model: "m".into(),
            },
        )
        .expect("create");
        assert_eq!(created.base_url, "http://localhost:1234/v1");
        assert!(created.has_api_key);
        let serialized = serde_json::to_string(&created).expect("serialize");
        assert!(!serialized.contains("key"), "{serialized}");
    }

    #[test]
    fn translation_agents_parse_and_reject_safely() {
        assert_eq!(
            TranslationAgent::parse("codex"),
            Some(TranslationAgent::Cli(CliProviderId::Codex))
        );
        assert_eq!(
            TranslationAgent::parse("  custom:abc-123 "),
            Some(TranslationAgent::Custom("abc-123".into()))
        );
        for rejected in ["", "ghost", "custom:", "custom:  ", "Codex"] {
            assert!(TranslationAgent::parse(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn codex_catalog_parses_arrays_objects_and_maps() {
        let from_strings = parse_codex_catalog(r#"["gpt-5.6-sol", "gpt-5.6-terra"]"#);
        assert_eq!(from_strings.len(), 2);
        assert_eq!(from_strings[0].id, "gpt-5.6-sol");

        let from_objects = parse_codex_catalog(
            r#"{"models": [{"id": "gpt-5.6-sol", "display_name": "Sol"}, 42, {"no_id": true}]}"#,
        );
        assert_eq!(from_objects.len(), 1);
        assert_eq!(from_objects[0].label, "Sol");

        let from_map =
            parse_codex_catalog(r#"{"models": {"gpt-5.6-sol": {}, "gpt-5.6-luna": {}}}"#);
        assert_eq!(from_map.len(), 2);

        assert!(parse_codex_catalog("not json at all").is_empty());
        assert!(parse_codex_catalog(r#"{"unrelated": 1}"#).is_empty());
    }

    #[test]
    fn opencode_output_parses_provider_model_lines() {
        let models = parse_opencode_models(
            "anthropic/claude-sonnet-4-5\n\nopenai/gpt-5.2\nAvailable models:\nduplicated\nanthropic/claude-sonnet-4-5\n",
        );
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, vec!["anthropic/claude-sonnet-4-5", "openai/gpt-5.2"]);
    }

    #[test]
    fn openai_model_list_extracts_ids_with_owners() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "gpt-4o-mini", "owned_by": "openai"},
                {"id": ""},
                {"other": 1}
            ]
        });
        let models = parse_openai_model_list(&body);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-4o-mini");
        assert_eq!(models[0].detail.as_deref(), Some("by openai"));
    }

    #[test]
    fn translation_selection_round_trips_and_validates() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(
            get_translation_selection(dir.path())
                .expect("read empty")
                .is_none()
        );
        let saved =
            set_translation_selection(dir.path(), "codex", "gpt-5.6-sol", None).expect("save");
        assert_eq!(saved.agent, "codex");
        assert_eq!(saved.model, "gpt-5.6-sol");
        let loaded = get_translation_selection(dir.path())
            .expect("read")
            .expect("selection saved");
        assert_eq!(loaded.model, "gpt-5.6-sol");

        assert_eq!(
            set_translation_selection(dir.path(), "ghost", "m", None)
                .expect_err("bad agent")
                .code,
            "invalid_agent"
        );
        assert_eq!(
            set_translation_selection(dir.path(), "codex", "  ", None)
                .expect_err("blank model")
                .code,
            "invalid_model"
        );
        assert_eq!(
            set_translation_selection(dir.path(), "custom:ghost", "m", None)
                .expect_err("ghost provider")
                .code,
            "provider_not_found"
        );

        let custom = save_ai_custom_provider(
            dir.path(),
            SaveCustomInput {
                protocol: "openai".into(),
                id: None,
                name: "Local".into(),
                base_url: "http://localhost:1234/v1".into(),
                api_key: Some("key".into()),
                model: "m".into(),
            },
        )
        .expect("custom provider");
        let saved = set_translation_selection(
            dir.path(),
            &format!("custom:{}", custom.id),
            "other-model",
            None,
        )
        .expect("custom selection");
        assert_eq!(saved.model, "other-model");
    }

    #[test]
    fn fallback_lists_standard_tool_directories() {
        let home = Path::new("/tmp/example-home");
        let directories = fallback_directories_for(home);
        for relative in [
            ".opencode/bin",
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".volta/bin",
        ] {
            assert!(
                directories.contains(&home.join(relative)),
                "missing {relative}"
            );
        }
        // System prefixes cover the package managers GUI launches miss:
        // Homebrew (Apple Silicon and Intel), MacPorts and Linuxbrew.
        // Windows relies on PATH plus per-user install locations, so the
        // system list is intentionally empty there.
        #[cfg(not(windows))]
        for system in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ] {
            assert!(
                system_fallback_directories().contains(&PathBuf::from(system)),
                "missing {system}"
            );
        }
        #[cfg(windows)]
        assert!(
            system_fallback_directories().is_empty(),
            "windows has no unix system fallbacks"
        );
    }

    #[test]
    fn fallback_expands_nvm_version_bins() {
        let home = tempfile::tempdir().expect("temp home");
        let first = home.path().join(".nvm/versions/node/v20.0.0/bin");
        let second = home.path().join(".nvm/versions/node/v22.0.0/bin");
        std::fs::create_dir_all(&first).expect("stage nvm bin");
        std::fs::create_dir_all(&second).expect("stage nvm bin");
        let directories = fallback_directories_for(home.path());
        assert!(directories.contains(&first), "first node bin listed");
        assert!(directories.contains(&second), "second node bin listed");
    }

    #[test]
    fn shell_path_output_splits_without_a_shell() {
        #[cfg(unix)]
        {
            let directories = split_shell_path_output("/opt/homebrew/bin:/usr/local/bin");
            assert_eq!(
                directories,
                vec![
                    PathBuf::from("/opt/homebrew/bin"),
                    PathBuf::from("/usr/local/bin")
                ]
            );
        }
        assert!(split_shell_path_output("   ").is_empty());
        assert!(split_shell_path_output("").is_empty());
    }

    #[test]
    fn staged_tool_is_found_through_combined_directories() {
        // Regression for bundled desktop launches: the process PATH alone
        // misses per-user installs (opencode, claude, nvm shims), so the
        // combined lookup must still resolve a staged tool directory.
        let dir = tempfile::tempdir().expect("temp dir");
        let binary = dir
            .path()
            .join(format!("opencode{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, b"stub").expect("stage binary");
        let directories = vec![dir.path().to_path_buf()];
        let found = find_cli_binary_in(&directories, CliProviderId::Opencode).expect("tool found");
        assert_eq!(found, binary);
        assert!(cli_available_in(&directories, CliProviderId::Opencode));
        assert!(!cli_available_in(&directories, CliProviderId::Codex));

        let home = tempfile::tempdir().expect("temp home");
        let tool_dir = home.path().join(".opencode/bin");
        std::fs::create_dir_all(&tool_dir).expect("stage tool dir");
        let staged = tool_dir.join(format!("opencode{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&staged, b"stub").expect("stage tool");
        let fallbacks = fallback_directories_for(home.path());
        assert_eq!(
            find_on_path_in(&fallbacks, "opencode").expect("fallback hit"),
            staged
        );
    }

    #[test]
    fn child_path_puts_tool_dir_first_and_dedupes() {
        let parent = Path::new("/tmp/tools/bin");
        let directories = vec![PathBuf::from("/usr/bin"), PathBuf::from("/tmp/tools/bin")];
        let joined = build_child_path(Some(parent), &directories).expect("path built");
        let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(
            parts,
            vec![PathBuf::from("/tmp/tools/bin"), PathBuf::from("/usr/bin"),]
        );
        assert!(build_child_path(None, &[]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn shim_tools_resolve_their_runtime_through_the_child_path() {
        // Regression for the Codex `#!/usr/bin/env node` launcher: the child
        // must resolve its runtime from the augmented PATH even when the
        // app itself launched with a sparse GUI environment.
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("temp dir");
        let runtime = dir.path().join("myhelper");
        std::fs::write(&runtime, "#!/bin/sh\necho runtime-ok\n").expect("stage runtime");
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755))
            .expect("chmod runtime");
        let shim = dir.path().join("myshim");
        std::fs::write(&shim, "#!/usr/bin/env myhelper\n").expect("stage shim");
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
            .expect("chmod shim");
        let output = run_cli_output(&shim, &[], Duration::from_secs(10)).expect("shim runs");
        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("runtime-ok"),
            "runtime resolved through child PATH"
        );
    }
}
