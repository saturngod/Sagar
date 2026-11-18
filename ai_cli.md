# AI CLI: detection and invocation flow

Providers (`CliProviderId`, `src-tauri/src/domain/ai.rs`):

| id | label | binaries searched |
|---|---|---|
| `codex` | Codex | `codex` |
| `opencode` | OpenCode | `opencode` |
| `claude_code` | Claude Code | `claude` |
| `agy` | Antigravity | `agy` |
| `qoder` | Qoder | `qodercli`, `qoder` |

## 1. Detecting which CLIs are available

```
UI (AI settings)
  └─ Tauri command  list_ai_providers          (commands/mod.rs)
       └─ services::ai::list_ai_providers      (services/ai.rs)
            ├─ search_directories()            ← built ONCE for all providers
            │     1. process $PATH
            │     2. login-shell $PATH         (shell_path_directories)
            │          runs `$SHELL -l -c 'printf %s "$PATH"'`
            │          (fish: `string join : $PATH`), 32 KB cap, fallback if it fails
            │          → needed because Finder/Dock launches inherit a sparse PATH
            │     3. well-known install dirs   (fallback_directories)
            │          ~/.local/bin, ~/.cargo/bin, bun, npm-global, nvm/fnm/volta
            │          node version bins, /opt/homebrew/bin, /usr/local/bin,
            │          Windows %LOCALAPPDATA% / profile dirs for agy, ...
            ├─ for each CliProviderId::all():
            │     find_cli_binary_in(dirs, id)
            │       └─ for each binary name → find_on_path_in()
            │            candidate_file_names(): adds .exe/.cmd/.bat on Windows,
            │            skips npm POSIX shims, prefers real executables
            └─ returns CliProviderStatus { id, label, binaries, available }
```

- List view is **filesystem-only**: no process is spawned, no network.
- Result is `available = binary file found`.

### Explicit "Check" button (connectivity)

```
UI Check → check_ai_cli(provider_id)
  └─ services::ai::check_ai_cli
       ├─ CliProviderId::parse(id)             → invalid_provider error if unknown
       ├─ find_cli_binary(id)                  → None → available:false, "Not found…"
       └─ probe_cli_version(binary)
            └─ run_cli_output(binary, ["--version"], 8s timeout)
                 ok  → available:true, connected:true,  version (≤120 chars)
                 err → available:true, connected:false, message
```

## 2. Getting the model list

```
UI (model picker / "Refresh models")
  └─ list_translation_models(agent, refresh)         (services/ai.rs)
       ├─ TranslationAgent::parse(agent)             Cli(id) | Custom(id)
       └─ match agent → per-provider source (below)
            returns TranslationModelList { agent, source, models[], message }
            source = "live" | "cached" | "catalog"   (tells the UI how fresh it is)
            model  = { id, label, detail?, reasoning_efforts? }
```

CLI lookup first: `find_cli_or_unavailable(id)` → error `provider_not_available` if the binary is not found. All CLI calls go through `run_cli_output(binary, args, 15s)` (augmented PATH, timeout, output drained).

### Per provider

| Provider | Primary source | Fallback chain | Efforts |
|---|---|---|---|
| Codex | `codex debug models` (JSON, `parse_codex_catalog`, source `live`) | `codex debug models --bundled` (`catalog`) → built-in `CODEX_KNOWN_MODELS` | read from each model entry's advertised efforts |
| OpenCode | `opencode api model.list` (V2 JSON: provider/model/variants) | `opencode models` (plain `provider/model` lines) → error `models_unavailable` | `variants` of each model |
| Qoder | `qodercli --list-models` (plain table, header skipped) | control-channel request over stdio (`list_models_control`, correlation-checked, bounded timeout) | model-specific if the control catalog reports them, else Default only |
| Agy | `agy models` (`parse_agy_models`) | none; non-zero exit → "Authenticate in agy first, then retry" | Default |
| Claude Code | **no CLI command exists** → models.dev catalog (see below) | cached → built-in aliases + `CLAUDE_KNOWN_MODELS` | `claude_efforts(model_id)` |
| Custom API | `GET {base_url}/models` with the stored key, protocol-specific auth (`models_request`: OpenAI bearer, Anthropic headers, Gemini `x-goog-api-key`) | saved model only (`source: catalog`) | `protocol_efforts(protocol)` |

### Claude Code catalog (models.dev) — `claude_model_catalog_from`

```
refresh == false:
  cache fresh (< 24h)?            → source "cached"  (no network)
  last failure < 1h ago?          → fallback (don't hammer the network)
fetch_models_dev("https://models.dev/models.json")
  plain GET, no redirects, 10s timeout, ≤ 8 MB, no book content/credentials
  parse_models_dev: keep Anthropic models released after 2025-10-15, newest first
  ok   → write cache to app_meta (key claude_models_dev_cache), clear failure marker → "live"
  fail → write failure timestamp → fallback
fallback: last saved cache → "cached"; none → built-in list → "catalog"
final list = aliases (sonnet/opus/haiku/fable) + live models + any CLAUDE_KNOWN_MODELS not already present
```

A mutex (`MODELS_DEV_LOCK`) serialises concurrent refreshes.

### Validation applied to every source

- `clean_model_id`: trimmed, non-empty, ≤ 256 chars, no control characters/line breaks.
- `push_unique` de-duplicates by id; list capped at 2000 models.
- Users can always type a model ID manually; chosen model + effort are checked by `validate_model` / `validate_reasoning_effort` before saving (`set_translation_selection`) and stored in app settings SQLite.

## 3. Calling the CLI for a translation

```
UI → start_ai_translation                      (commands/mod.rs)
  └─ ai_translate::start_ai_translation        (services/ai_translate.rs)
       ├─ TranslationAgent::parse(agent)       → Cli(id) | Custom(id)
       ├─ TranslationProvider::prepare
       │     Cli(id) → find_cli_binary(id)     (same discovery as above;
       │                                        error provider_not_available)
       ├─ create_work_dir(job_id)              isolated temp dir, not a git repo
       └─ std::thread::spawn → provider.execute(model, request, effort,
                                                work_dir, cancel, timeout, on_chunk)
            └─ agent_args(id, model, prompt, effort)   fixed argv, no shell,
                                                       user input never becomes a flag
```

Dispatch in `execute`:

| CLI | Path | Prompt delivery |
|---|---|---|
| Codex | `codex_stream::run_with_instructions` — spawns Codex **App Server** (JSON-RPC over stdio), handshake with model/effort/prompt, ephemeral read-only policy, tool approvals declined | JSON-RPC message |
| OpenCode | `opencode_stream::run_with_instructions` — local HTTP/SSE server | HTTP request |
| Claude Code | `set_cli_instructions` (instructions file in work dir) → `execute_streaming` with `-p --model M --output-format stream-json --verbose --include-partial-messages --max-turns 5` | **stdin** |
| Qoder | same as above with `qoder::base_args() --model M --include-partial-messages --permission-mode dont_ask` | **stdin** |
| Agy | same, `--print= --input-format stream-json --model M --output-format stream-json --print-timeout 30m` | **stdin** (one JSON `user` line) |

Effort (if set) is spliced in: Codex `--config model_reasoning_effort="…"`, Qoder `--reasoning-effort`, OpenCode `--variant`, Claude/Agy `--effort`.

### Spawning (`execute_streaming`)

```
cli_command(binary)                 Command::new + CREATE_NO_WINDOW on Windows
  .args(args).current_dir(work_dir)
  .env("PATH", cli_child_path_env)  tool's own dir first, then discovery dirs
                                    (so node/bun runtimes beside the shim resolve)
  .stdin(piped if prompt via stdin) .stdout/.stderr piped
  .spawn()
    ├─ writer thread: write prompt to stdin, then close (EOF)
    ├─ reader threads: stdout chunks → per-provider stream decoder → on_chunk
    │                  stderr → bounded tail for error messages
    └─ loop: poll cancel flag + hard deadline
         cancel / timeout / oversize → kill_cli (kills process tree) and reap
```

### Result handling

```
decoded stream text → validate_proposal (strip code fences, size limits)
  → check_skeleton (tags/anchors/IDs of source vs proposal must match)
  → event emitted to UI; user reviews before it is saved
```

Cancel: `cancel_ai_translation(job_id)` sets the shared flag, which `execute_streaming` polls.

## Key files

- `src-tauri/src/domain/ai.rs` — `CliProviderId`, status DTOs
- `src-tauri/src/services/ai.rs` — discovery, `--version` probe, model listing, `cli_command`, `run_cli_output`
- `src-tauri/src/services/ai_translate.rs` — `agent_args`, `agent_stdin`, `TranslationProvider::{prepare, execute}`, `execute_streaming`
- `src-tauri/src/services/{codex_stream,opencode_stream,qoder}.rs` — provider-specific protocols
- `src-tauri/src/commands/mod.rs` — Tauri command adapters
