# Plan.md — EPUB Translation Studio

### Qoder CLI provider — 2026-10-24

Added Qoder to built-in CLI discovery/checks and persisted translation choices.
Both `qodercli` and `qoder` executable names are supported. The headless CLI
control channel loads internal model IDs, labels and per-model reasoning
capabilities without sending a user prompt or starting inference. Effort choices
come from `efforts` or `thinking_config.enabled.efforts`, with unsupported and
unknown-model overrides rejected at the Rust save/execution boundaries.

Translation, selected-mode batches and HTML repair use the existing headless
process service with Qoder's stream-json output and `--reasoning-effort` flag.
Only answer text enters progress; terminal success is required, duplicate or
malformed completions are rejected, and daily usage exhaustion has a specific
error message. Tools, external MCP configuration, filesystem settings and
session persistence are disabled for these runs. Existing timeout/cancellation,
proposal validation, consent and draft-save behavior remain in use. No JS SDK,
Python helper or application web server was added. The existing tempfile crate
is now a runtime dependency for isolated catalog-query directories.

Verification: `cargo fmt --check` and the complete Rust suite passed (261 passed,
6 ignored). `bun run check` passed with the existing bundle-size warning;
frontend tests passed (97 tests, 867 assertions). Added regression coverage for
catalog parsing/control responses, timeout cleanup, model selection persistence,
Qoder streaming with split Myanmar UTF-8, failed/duplicate results, tool requests,
CLI arguments and the model-specific effort picker. The installed Qoder 1.1.78
catalog probe also passed explicitly. A synthetic generation using production
flags confirmed tools/MCP were empty and the selected model was DeepSeek-Flash;
it returned Qoder daily-usage error 110, so successful live generation remains
unverified. No running Sagar build was available for rendered verification.
Normal/minimum-size light/dark UI, native keyboard/IME behavior and Windows/Linux
remain unverified. No milestone checkboxes changed.

Next unfinished task: verify a successful Qoder translation/HTML repair and
native provider/model/effort selection after the account usage limit resets.

### AI translation can continue in the background — 2026-10-20

Whole-chapter and selected-paragraph AI runs no longer stop when their dialog
closes. The dialog offers "Continue in background"; closing it (X button,
Escape, chapter switch) also keeps the run alive instead of cancelling it, so
switching chapters or reading the source while a translation runs no longer
throws the work away. Only the explicit Cancel action stops a run.

An app-level job tracker (`features/translator/aiJobs.tsx`) mirrors the Rust
worker through the existing `ai-translation-event` and `ai-selection-event`
streams, and a status bar below the workspace toolbar lists every background
run with its chapter, mode, elapsed time, and Cancel / Open / Dismiss actions.
The translation header button spins while a run works on the current chapter.
Reopening the AI dialog for a chapter re-attaches to its running job and shows
a finished proposal that is still waiting for review; a background whole-chapter
result fills an empty draft exactly like a foreground run, and a draft with
content keeps the proposal for an explicit Insert.

No Rust change was needed: the worker already runs on its own thread and
reports through app-level events. Selected-paragraph runs keep their saved
checkpoints, so they also survive an app restart; a whole-chapter proposal is
held in memory and is lost if the app quits before the run finishes.

Run times are shown as a readable duration instead of a bare second count:
`42s`, then `5m 30s`, then `1h 5m 30s`. One shared formatter
(`lib/duration.ts`) is used by the background status bar, the translation
dialog and the HTML-repair dialog, and the surrounding sentence fragments in
the Burmese, Mon, Arakanese and Shan catalogs were updated so the unit is
never stated twice.

Verification: `bun run check` passed (Biome and production build; the existing
bundle-size warning remains) and the frontend suite passed (96 tests, 856
assertions), including new background regressions for both modes and unit
coverage for the duration format. Two existing selection tests were updated
because closing a dialog no longer cancels its run. Native window behavior
(minimizing the app during a run), live provider runs, light/dark rendering
and Windows/Linux were not verified. Next unfinished task: exercise a
background run in rebuilt native Sagar and confirm the proposal reaches the
draft after the dialog was closed.

### Whole-chapter AI output kept for any completed response — 2026-10-20

Whole-chapter AI translation no longer discards a completed provider response
because Rich text or strict XHTML validation cannot represent it. A new
permissive `accept_document` path canonicalizes supported HTML and otherwise
stores the returned markup unchanged, including malformed nesting, unsupported
elements such as `font`, and empty links (`<a href="">`). Draft saves and whole-
chapter AI proposals both use this path; structural skeleton differences are no
longer a completion failure. Empty results, oversized responses, and active
content (scripts, embedded elements, event handlers, `javascript:`/`vbscript:`,
non-image `data:` URLs, active CSS, `srcdoc`) are still rejected, so only genuine
HTTP/network/provider or active-content errors leave the draft untouched.
Non-empty drafts are never silently replaced; the AI dialog keeps its preview
and explicit Insert action.

Export now degrades per chapter instead of aborting the book. If a translated
chapter cannot be spliced, or any of its images cannot be packaged, that chapter
keeps its original text and export returns a `chapter_translation_unusable`
warning; the saved draft remains intact. AI proposal and Fix HTML previews are
scrubbed in the frontend (scripts, embedded contexts, event/URL attributes,
network fetches and active CSS) while Apply still inserts the raw tool result.

Validation: `cargo fmt --check` and the full Rust suite passed (256 passed,
5 existing ignored), including new save/reload/export regressions for unsupported
HTML, empty links, malformed markup and invalid images. `bun run check` passed
(Biome and production build; existing bundle-size warning remains) and the
frontend suite passed (90 tests, 828 assertions). No milestone checkboxes
advanced. Native insertion/reload/export, IME input, light/dark rendering and
Windows/Linux behavior were not verified. Selected-paragraph AI append still uses
its stricter duplicate-ID check and was not changed. Next unfinished task: exercise
whole-chapter AI insertion, reload and export in rebuilt native Sagar.

### Highlighted code Windows line endings — 2026-10-16

Corrected the highlighted-code round-trip test to expect XML-normalized LF
rather than raw fixture checkout line endings. The regression now exercises both
LF and CRLF inputs on every platform, retaining exact markup and code indentation
checks plus reload/Unicode checks. Production validation is unchanged; no milestone
checkboxes advanced.

Validation: Rust formatting and all three rich_text unit tests passed locally,
including explicit CRLF coverage. Native Windows execution was not available.
Next unfinished task: rerun the Rust suite in the Windows build.

### Shared language prompt test expectations — 2026-10-16

Updated the existing prompt and custom HTTP transport tests to expect the shared
catalog's Myanmar, Shan and Mon labels, including their Myanmar-script names.
Production prompt behavior is unchanged. This continues language compatibility
polish; no milestone checkboxes advanced.

Validation: `cargo fmt --check` and the full `cargo test` suite passed (238 passed,
four existing ignored), including all three reported failures. No UI or native
behavior changed; frontend and native checks were not run. Next unfinished task:
verify the language picker and glyph rendering in rebuilt native Sagar.

### Rich highlighted code metadata — 2026-10-16

Allow passive `data-highlighted` only on XHTML code elements in document
validation and retain it through Rich normalization, editing and serialization.
Marked pre/code blocks use the existing pre container so inner code attributes
and syntax spans survive instead of being discarded by the stock code block.
A shared synthetic fixture covers inline/block code, whitespace, Myanmar text,
reload and rejection of event/unrecognized attributes. Continues Milestone 5
compatibility polish; no milestone checkboxes advanced.

Validation: build, changed-source Biome and Rust formatting passed; all 18 Rust
rich-text tests and the targeted Rich regression passed. The broader frontend
file had 27 passing tests and the previously documented local-image-count
failure. Full check remains blocked by existing formatting in mnw/rki/shn
catalogs; build retains its bundle-size warning. Native rendering, IME,
light/dark/minimum-size and cross-platform behavior were not verified. Next
unfinished task: rebuild Sagar and retry the affected proposal in native Rich
view. Full EPUB export/reopen was not exercised for this attribute change.

### Select heading groups and preserve time in translations — 2026-10-16

Fixed the mismatch between source preview, Rich editing and selected-paragraph
validation. Heading groups are selectable as one intact unit, retaining their
existing locator/identity. Time elements and datetime now survive Rust document
validation and Rich normalization, editing and reload; inline placement and
active-attribute checks remain enforced. Existing blocked selection cache entries
use the existing revalidation path on reopen; no migration or reimport is needed.
Also corrected an extra angle bracket in unsupported-element diagnostics.
This continues Milestone 5 compatibility polish; no milestone checkboxes advanced.

Validation: bun run check and all 83 frontend tests passed (existing bundle-size
warning). Rust formatting and the full Rust suite passed before the additional
export regression; that new save/reload/export/reopen regression also passed.
The synthetic fixture covers selection, text reconstruction, Myanmar editing,
datetime retention, source immutability and invalid active/nested content.
Native rendered/IME, normal/minimum window sizes, themes and Windows/Linux
remain unverified. Next unfinished task: rebuild Sagar and reopen the affected
chapter to verify both checkboxes and the translation-to-Rich flow in native UI.


### Rich legacy div alignment — 2026-10-16

Fixed `div align` rejection in Rust document validation and attribute loss in
Rich normalization/serialization and both text/block div editor nodes. The
user's Myanmar example is a regression fixture covering escaped angle brackets,
Rich typing/reload, SQLite save, draft EPUB export/reopen and source immutability.
This continues Milestone 5 compatibility polish; no milestone checkboxes advanced.

Validation: lint/TypeScript/build passed (existing bundle-size warning); Rust
formatting and both targeted validation/export tests passed. The frontend file
passed 25 tests with its previously documented local-image-count failure.
Native visual/IME, light/dark/minimum-window and Windows/Linux checks remain
unverified. Next unfinished task: verify this HTML → Rich flow in rebuilt native
Sagar.


### Rich chapter parent and heading groups — 2026-10-16

Rust document validation and the frontend Rich schema/serializer now preserve
`data-parent` and `hgroup`. Heading groups participate in block normalization
and comparison so indentation does not prevent HTML → Rich conversion. Existing
abbr support retains the user's nested abbreviation and EPUB semantics. The
synthetic chapter-heading-group fixture covers conversion, Myanmar editing,
reload, SQLite save, export and reopening the EPUB, with original bytes intact.
No migration or existing draft rewrite is required.

Validation: lint/build and Rust formatting passed; the full Rust suite passed
232 tests with four existing ignored. The affected frontend file passed 24 tests,
including the new regression; its existing local-image display test failed
(expected three images, got two), reproduced against unchanged HEAD frontend
sources. The targeted new regression also passed independently. The existing
bundle-size warning remains. Native visual/IME, theme/minimum-window and
Windows/Linux checks remain unverified. Next unfinished task: reopen this draft
in rebuilt native Sagar and verify HTML → Rich editing. No milestone checkboxes
advanced; this continues Milestone 5 compatibility polish.

### Original preview time text — 2026-10-16

Fixed the preview sanitizer dropping `<time>` and its entire text subtree.
The passive element and its unnamespaced datetime attribute now survive preview;
event handlers, styles and scripts remain filtered. A synthetic XHTML regression
fixture verifies “In the year 1878” in both full-chapter and block previews,
with a second test covering nested emphasis and active-content removal. Source
snapshots, translations and export serialization are unchanged.

Validation: Rust formatting and full suite passed (231 tests passed, four
existing ignored); frontend lint/build passed with the existing bundle-size
warning. Native rendering, light/dark/minimum-window and Windows/Linux behavior
remain unverified. Next unfinished task: rebuild/reopen the affected chapter in
native Sagar to verify the year appears. This is a Milestone 5 preview polish
regression; no milestone checkboxes advanced.

### Corrected saved ZAI API format — 2026-10-16

The repeated “HTTP 200 returned JSON instead of an SSE stream” message was traced
to the non-Anthropic branch. Read-only inspection of local settings confirmed the
selected ZAI provider had the Anthropic endpoint and glm-5.3 model but protocol
openai. Corrected that provider through the existing Rust settings service to
anthropic, retaining its stored key and model. The endpoint normalizes to
`https://api.z.ai/api/anthropic/v1`; Messages requests append `/messages`.
Verified the persisted protocol and active selection without reading/displaying
secrets or sending book text. A temporary maintenance example invoked the service
and was removed afterward. No implementation change or additional automated test
was needed. No live provider request made. Next unfinished task: reopen Sagar and
retry the existing ZAI selection; native/provider success remains unverified.


### Anthropic-compatible response formats and Z.ai auth — 2026-10-16

The user reported a successful HTTP response rejected for lacking the SSE content
type, using `https://api.z.ai/api/anthropic` and `glm-5.3`. Z.ai documents that base
URL with ANTHROPIC_AUTH_TOKEN. Anthropic calls and model probes now send Bearer
authentication alongside x-api-key; the same saved key goes only to the chosen
endpoint. Existing base normalization retains `/api/anthropic/v1/messages`.

The transport reports a fixed response-format classification in request details.
For Anthropic only, bounded response inspection accepts SSE carrying a nonstandard
content type or a complete Messages JSON envelope when stream=true was ignored.
A JSON success requires assistant/message identity, text-only output (thinking is
excluded), nonempty text and end_turn. Tool/error/wrong-protocol/HTML responses
remain rejected. Incomplete JSON answers emit recoverable partial text before
failure; existing proposal validation and draft-review boundaries still apply.
No extra request or automatic protocol fallback occurs. Cancellation/deadlines
cover inspection and JSON reads, and mislabeled completed SSE avoids waiting for
connection closure. Response bodies, credentials, book text and reasoning remain
excluded from diagnostics. References:
https://docs.z.ai/devpack/tool/claude and
https://platform.claude.com/docs/en/api/messages/create.

Local HTTP tests cover the user's Z.ai path/model with Bearer authentication,
JSON/mislabeled SSE, private error handling, HTML/wrong-protocol/tool rejection,
recoverable truncation and cancellation. Rust formatting and the full suite
passed (173 unit, 13 selection, 22 export, 20 import tests; four existing tests
ignored); the 17 transport tests passed after adding the final JSON cancellation
regression. No frontend code changed in this follow-up. This does not establish the actual
response body returned by the user's account; no live authenticated Z.ai request
was made. No UI layout or persistence schema changed. Next unfinished task:
rebuild/restart Sagar and retry the saved provider, checking response-format/status
details if it fails. Native and Windows/Linux behavior remain unverified.
No milestone checkboxes advanced.


### Gemini Interactions and precise HTTP failures — 2026-10-16

The user reported “The provider is temporarily unavailable” after testing Gemini.
That message covered every HTTP 5xx response and does not establish the precise
status, selected model or cause; no authenticated Google request was reproduced.
This follow-up corrects the confirmed API mismatch discussed with the user's
Interactions example: Gemini now posts model/input/stream=true/store=false to
`{base}/interactions`, authenticating with x-goog-api-key. Both a versioned base
and full Interactions URL work, preserving proxy prefixes; models/ is removed
from model IDs. Existing settings and saved translations remain intact.

The SSE adapter follows Google's current step.start/delta/stop and interaction
lifecycle events, emitting only model-output text and counting thought summaries
without exposing them. Completed status and closed steps are required; tools,
non-text output, malformed/truncated streams and failed/incomplete status are
rejected with recoverable partial text. Translation, Retry, HTML repair and
cancellation still use the existing services. Gemini reasoning/output-token
configuration remains at provider defaults; this change does not add controls
for the example's max_output_tokens or thinking_level. No automatic HTTP retry
or fallback to a different API/model is introduced.

HTTP failures now retain their numeric status with fixed, actionable messages
for internal errors, service unavailability, quota, access and missing endpoints/
models, without returning provider bodies or private content. This improves
diagnosis; it does not guarantee Google's availability. Settings help names the
Interactions API and stateless behavior. API reference:
https://ai.google.dev/api/interactions-api.

Validation: frontend lint/build and four affected component tests passed; the
existing bundle-size warning remains. Rust formatting and full suite passed
(169 unit, 13 selection, 22 export, 20 import tests; four existing tests ignored),
with additional Interactions regressions covering full endpoint/proxy paths,
model prefix, request privacy, partial recovery and HTTP 500/503 status feedback.
Native light/dark/minimum-window/keyboard, Windows/Linux and live Gemini checks
remain unverified. Next unfinished task: rebuild/restart Sagar and retry the
chosen Gemini model; inspect the numeric HTTP status if Google still rejects it.
No milestone checkboxes advanced.


### Anthropic BYOK proxy URLs — 2026-10-15

The user confirmed direct API key + configurable endpoint support, including
DeepSeek's Anthropic-compatible API. Anthropic URL handling now accepts SDK roots
(`https://api.deepseek.com/anthropic`), versioned bases and full Messages URLs
(`https://api.anthropic.com/v1/messages`). Root inputs gain `/v1`; full endpoints
are normalized to their containing base. Translation and model probes use the
same normalized base, retaining proxy prefixes and avoiding duplicated paths.
The settings help now describes these inputs. Existing saved versioned bases
continue working; no additional schema migration is needed. Model IDs remain
editable manually when a compatible service does not offer a model list.
Reference: https://api-docs.deepseek.com/guides/anthropic_api/ and the official
Anthropic SDK's `/v1/messages` request path. Regression tests cover root/versioned/
full/proxy URL normalization and real local HTTP requests with DeepSeek-style
paths, API-key authentication and the selected model. Frontend lint/build and
four affected component tests passed; Rust formatting and full suite passed
(169 unit, 13 selection, 22 export, 20 import tests; four existing tests ignored),
with the URL unit regression rerun after the final input-validation adjustment.
The existing bundle-size warning remains. No live DeepSeek call made.
Next unfinished task: rebuild/restart and verify with the user's chosen key/model.
Native visual/interaction and Windows/Linux checks remain unverified.


### Gemini and Anthropic-compatible APIs — 2026-10-15

Explicitly requested follow-up to the existing custom API translation workflow.
Settings → AI Providers now offers OpenAI Compatible, Gemini API Compatible and
Anthropic (Claude) API Compatible. Claude API support means the Anthropic Messages
protocol used by Claude Code; the existing Claude Code CLI remains a separate
provider. The form explains the protocol and versioned base URL, supplies official
Gemini/Anthropic endpoint defaults, and keeps custom endpoints editable.

Schema migration 12 adds a persisted protocol, defaulting existing providers to
OpenAI without changing secrets or translations. Rust model discovery/testing uses
protocol-specific authentication. Native Gemini Generate Content and Anthropic
Messages streaming feed the same translation, Retry, HTML repair, consent,
validation and proposal-review services. Keys remain masked; redirects are disabled.
Only answer text enters proposals; thinking is counted without exposing it, and
blocked/truncated/tool/error responses fail with recoverable partial output.
Gemini/Anthropic use provider-default reasoning; OpenAI overrides are unchanged.
Anthropic requests allow 8192 output tokens; truncated output uses existing Retry.
Model discovery uses the returned page; other IDs can be entered manually.

References: https://ai.google.dev/api/generate-content and
https://platform.claude.com/docs/en/build-with-claude/streaming.
Validation: frontend lint/build passed (existing bundle-size warning), all 75
frontend tests passed, Rust formatting and full suite passed: 167 unit, 13 AI
selection integration, 22 export and 20 import tests; four existing tests ignored.
Local HTTP regressions cover native auth/payload/model discovery, split Myanmar
Unicode, completion/error handling, translation/Retry/HTML repair, source/draft
immutability and cancellation/deadlines. Migration tests preserve existing keys;
component tests cover format selection/default URLs/persistence and default effort.
No live provider credentials were used. Native visual, keyboard, minimum-window,
light/dark appearance and Windows/Linux checks remain unverified: computer-use
inspection reported the Mac locked. Next unfinished task: rebuild/restart Sagar
and verify both providers with a real chapter, Cancel, Retry and HTML repair.
No milestone checkboxes advanced.


### Selection formatting popup — 2026-10-15

Milestone 2 editing follow-up: selecting text in Translation → Rich text now
shows a floating toolbar for bold, italic, underline, strike, highlight, inline
code, superscript, subscript and links. Link entry supports book-relative URLs,
#anchors and http(s)/mailto with visible validation, Save, Remove link and Cancel.
Editing an existing link expands its range; removal is disabled for links with
book attributes so navigation IDs/semantics cannot disappear silently.
The popup preserves selection, supports Tab/arrow keys/Escape, follows scroll/
resize, and hides for read-only/HTML views and composition. Existing book inline
containers remain the serialization model; highlight/sup/sub tools only act on
compatible inline selections, preserving attributes and disabling unsafe ranges.
Changes use the existing draft/autosave path; no schema or dependency changes.

Validation: frontend lint/build passed (existing bundle-size warning); all 73
frontend tests passed, with the four popup tests rerun after the final link
safeguard. Rust formatting and 16 rich-text validation tests passed. Browser
component checks cover light/dark appearance, normal/minimum layouts including
960×640, empty/typed/restored content, Myanmar text, selection formatting, link
validation/correction and keyboard focus/dismissal. Native IME, current desktop
build and Windows/Linux behavior remain unverified. Next unfinished task:
rebuild/restart Sagar and check native selection/IME in a real chapter. No
milestone checkboxes advanced.

### Borderless table styles in Rich text — 2026-10-15

Milestone 2 compatibility follow-up: the translation validator now permits
allowlisted inline table borders, spacing, padding, sizing and alignment styles,
while retaining URL/function/positioning restrictions. Rich tables derive an
editor-only border hint from parsed CSS (including shorthand precedence) or
legacy border="0". Borderless layout/verse tables omit default cell borders and
padding; explicit cell styles, regular grids and selection feedback remain.
Protected table previews derive the same hint without mounting imported CSS.
Saved/exported XHTML retains styles, classes and EPUB semantics, excluding the
editor hint. Book stylesheets are still not applied inside the Rich editor.

The supplied verse fixture covers normalization, Myanmar typing, reload,
external draft insertion, HTML/Rich switching, save/export/reopen and unchanged
source bytes. All 50 frontend tests, lint/build and the full Rust suite passed;
the existing bundle-size warning and ignored probes remain. Browser component checks cover
light/dark appearance at 1280×800 and 960×640, empty/typed/restored content and
Myanmar text. Native IME, current desktop rebuild and Windows/Linux checks remain
open. Next unfinished task: rebuild/restart Sagar and verify the affected chapter
in the desktop app. No milestone checkboxes advanced.

### Rich text normalization and book compatibility — 2026-10-14

Supersedes preserving every empty decoration below. Conversion now removes
empty decorative headings/formatting/wrappers when only class attributes remain,
retaining IDs, EPUB/ARIA semantics, styles, meaningful whitespace and assets.
XHTML self-closing containers are parsed as XML before HTML conversion. Mixed
inline/block containers, bare list items, quotations and plain table cells gain
paragraph wrappers; only inter-block indentation is discarded. Cleanup enters
the existing draft/autosave service after successful validation and preservation
checks, with stale-result protection. Normal typing/composition does not reload
or normalize the editor. Eight common inline semantic tags and ARIA attributes
now round-trip. Tables with caption/thead/tfoot remain intact as protected,
readable content in Rich view; surrounding prose remains editable, and HTML
view owns edits to those tables. Their preview mounts no imported URLs/images/
CSS. Basic authored tables retain existing editing tools.

`bun run check`, all 41 frontend tests, Rust formatting and full Rust suite
passed (existing ignored probes unchanged). Coverage includes cleanup persistence,
HTML/Rich switching, Myanmar edits, whitespace/anchor preservation, mixed/list/
table structures, protected table assets and save/export/reopen/source immutability.
Existing bundle warning remains. The running desktop app was inspected and still
showed its old conversion failure; it has not been rebuilt/restarted with this
change. Updated native light/dark/minimum-size, keyboard/IME and Windows/Linux
verification remain open. Next unfinished task: rebuild/restart and verify Rich
conversion and cleanup in the desktop app. No milestone checkboxes advanced.

### Empty subtitle formatting in Rich text — 2026-10-14

The supplied chapter failed HTML → Rich because Tiptap marks dropped the empty
`em` inside its subtitle heading. Empty em/strong/u/s/code elements now have
inline node representations that preserve their tags and book attributes;
nonempty formatting still uses normal marks. The exact pasted chapter now
passes the preservation comparison. A synthetic subtitle fixture tests parsing,
Myanmar typing, saving/reloading and empty formatting with IDs/classes/titles;
mounted proposal tests cover immediate insertion and HTML → Rich switching.
All 24 affected frontend tests and `bun run check` passed. Existing bundle-size
warning remains. No Rust code changed; native IME, visual themes/minimum size
and Windows/Linux remain unverified. Next unfinished task: rebuild/restart and
select Rich text on the retained chapter; another translation is unnecessary.

### Paragraph-boundary instructions and recoverable structure feedback — 2026-10-14

Translation prompts now include computed element counts from the supplied
fragment and require in-place text translation, unchanged paragraph boundaries,
no splitting/merging/repetition/added ending, and preserved header/footer/empty
element order and nesting. Counts supplement the full structure comparison;
they do not replace it. Structure failures now explain the issue in plain
language, retain returned text and explain Retry's matching-prefix behavior
instead of showing raw indexed markup/attribute diffs. A synthetic chapter
regression covers an extra paragraph before the footer, the prompt contract,
recovery and a corrected Retry; equal-count nesting changes remain rejected.
`cargo fmt --check` and the full Rust suite passed, with existing ignored
probes unchanged. No live model or native interaction check made. Prompts do
not guarantee model compliance; automatic deletion/merging of translated
paragraphs remains disallowed to prevent content loss. Next unfinished task:
rebuild/restart and Retry the retained proposal with the strengthened prompt.

### Book-configured language names in prompts — 2026-10-14

Supersedes the Myanmar-specific prompt rule below. Translation instructions
now resolve both saved Book Settings language tags into names and codes using
the same shared JSON catalog as the toolbar. Every language pair uses identical
instructions for translation and Unicode writing; there is no language-specific
branch. Regional tags use primary-language names while retaining the full tag;
unknown tags remain visible verbatim. Tests cover reversed and unrelated pairs,
regional/case variants, unknown tags, catalog consistency and the HTTP payload.
`bun run check`, all 34 frontend tests, Rust formatting and full Rust suite passed
(four existing tests ignored). Existing bundle warning remains. No live provider
or native visual check made. Next unfinished task: rebuild/restart and generate
a fresh proposal with the desired Book Settings language direction.

### Explicit Myanmar translation direction — 2026-10-14

Investigation confirmed the saved source/target language tags reach the prompt
and custom API user message; the old prompt identified Myanmar only as `my`.
The supplied English output does not establish why the provider ignored it.
Myanmar targets (including regional/case variants) now explicitly require
Myanmar/Burmese Unicode prose and headings, forbid English paraphrasing and
Latin transliteration, and prioritize the saved target over conflicting book/
run/content instructions. Link labels may translate while hrefs remain fixed.
The existing mock HTTP regression now sends a real book-translation prompt and
verifies the complete direction/instructions in the received request. Generic
targets retain their requested language. Formatting and full Rust suite passed
(four existing tests ignored). No live provider request or native check made;
language correctness is not guaranteed by XHTML/structure validation. Next
unfinished task: rebuild/restart and generate a fresh proposal; continuation
Retry may retain the previously returned English prefix.

### Translated title attributes — 2026-10-14

Chapter translation now permits existing unnamespaced XHTML `title` values
to be translated as tooltip text. The structure check still requires the
attribute to exist on the same element, rejects added titles and protects
other attribute values (including IDs, classes and URLs). Prompt instructions
explicitly describe this exception. Regression covers the reported legal-notice
title, proposal validation, structure comparison and Retry recovery, plus
rejected structural changes and namespaced title changes. Formatting and the
full Rust suite passed (four existing tests ignored). No frontend code changed;
native execution remains unverified. Next unfinished task: rebuild/restart and
Retry the retained proposal with its translated legal-notice title.

### Explicit XHTML and anchor-target output instructions — 2026-10-14

Translation and HTML repair prompts now explicitly require well-formed EPUB
body XHTML and removal of every anchor `target`, preserving href and link text.
The translation rule explicitly overrides the general attribute-preservation
instruction. Existing validation still removes targets if a tool echoes them;
structure comparison still accepts that normalization. Formatting and all 30
AI translation service tests passed. No UI changed or live provider run made.
Next unfinished task: rebuild/restart and verify the next proposal uses these
instructions. No milestone checkboxes changed.

### Proposal structure comparison after target normalization — 2026-10-14

The prior target-removal change missed the subsequent proposal structure check:
it compared immutable source attributes against already-normalized output and
rejected a missing anchor `target`. The shared structure skeleton now ignores
only unnamespaced `target` on XHTML anchors on both sides. Regression exercises
validation → structure comparison → completed Retry recovery, with both echoed
and omitted targets, and confirms changed URLs, IDs, classes and non-anchor
targets remain rejected. `cargo fmt --check` and the full Rust suite passed;
four existing tests were ignored. No frontend code changed in this follow-up.
Native execution remains unverified. Next unfinished task: rebuild/restart Sagar
and use Retry on the retained proposal. No milestone checkboxes changed.

### Translated titlepage Rich text conversion — 2026-10-14

Supersedes target preservation below: the user authorized removing link targets
from translated XHTML. Chapter validation now accepts but omits anchor `target`
attributes, including older drafts; URLs and text remain intact. The Rich schema
also preserves standalone links beside paragraphs in book containers, avoiding
the extra paragraph that previously failed the preservation check. Its editor
node view omits the URL so editing cannot navigate away. Unsupported structures
still trigger the existing loss-aware check rather than being broadly stripped.
Regression covers the synthetic Myanmar titlepage, editing/reloading, immediate
proposal insertion and HTML → Rich switching. `bun run check`, all 34 frontend
tests, `cargo fmt --check` and the full Rust suite passed (two existing probes
ignored). Existing bundle-size warning remains. Native visual/IME and Windows/
Linux checks remain unverified. Next unfinished task: rebuild/restart Sagar and
verify the affected saved chapter switches to Rich text. No checkboxes advanced.

### Preserved link targets — 2026-10-14

Milestone 2 validation follow-up: chapter documents now preserve `target` on
XHTML anchors, fixing rejected translation proposals containing `_top` links.
A synthetic titlepage fixture checks Myanmar text, target preservation,
repeat validation and inert preview links; active URLs/event handlers and
misplaced target attributes remain rejected. `cargo fmt --check` and the full
Rust test suite passed (two existing import probes ignored). No UI changed;
native and cross-platform interaction remain unverified. Next unfinished task:
rebuild/restart Sagar and retry the affected proposal. No checkboxes advanced.

### Reasoning controls for every custom model — 2026-10-14

Supersedes the Z.ai/model-specific restriction below. All custom API models now
expose the same user-selectable reasoning overrides, including GLM-5.3-Flash,
arbitrary catalog entries and manually entered IDs. No hostname/model checks
control availability. Both live/saved model catalogs expose these overrides;
the frontend also supplies them when metadata/catalog lookup is absent.
Rust validates the provider exists and the effort token is allowed, persists the
selection and forwards it through the existing API transport. Default omits the
parameter. UI text explains these are overrides whose acceptance is determined
by the endpoint, rather than detected model capabilities. CLI behavior unchanged.
Validation: Rust formatting/full suite passed, frontend lint/build and all 32
frontend tests passed. Regression covers arbitrary/local/Z.ai endpoints, model
IDs, effort persistence/execution validation, flash/catalog model switching and
Default saving; existing HTTP transport test verifies effort forwarding.
Native rendered themes/minimum-size/keyboard and cross-platform behavior remain
unverified. Existing bundle-size warning remains. Next unfinished task: rebuild/
restart and verify the selected custom model/effort against its API endpoint.
No milestone checkboxes changed.


### Custom API stall tracing and Z.ai effort — 2026-10-14

Follow-up to custom-provider streaming: the generic no-output notice previously
could not distinguish waiting for HTTP headers, active reasoning and answer text.
Both translation and HTML repair now retain a bounded, expandable API request
trace with response timing/status and stream/event/answer/reasoning byte counts.
Only fixed messages and numeric counters are emitted: no keys, endpoints,
book text, raw response bodies or reasoning text are logged or included.
Status events are non-terminal, do not reset the answer-silence timer or alter
the output, and obey existing job/chapter ownership and early-event buffering.
The disclosure participates in dialog keyboard focus trapping.
Z.ai's documented GLM-5.3 reasoning choices (low/high/max) now appear in model
selection for the exact official standard/coding endpoints. Save and execution
validate those choices locally, and custom calls send explicit reasoning_effort
only when chosen. Default remains the provider default; arbitrary compatible
providers do not gain inferred effort support. Existing CLI paths remain intact.
Official source: https://docs.z.ai/guides/llm/glm-5.3 documents always-enabled
reasoning and max as default. This is a plausible explanation for the user's
240s no-answer report, not an authenticated reproduction or latency diagnosis.
Validation: full Rust suite and formatting passed; frontend lint/build and all
31 frontend tests passed, with the affected dialog regression rerun after final
focus/label changes. Coverage includes effort validation/forwarding, endpoint
scoping, reasoning-only counters, private-data exclusion, early/stale status
routing and continued Cancel behavior. Existing bundle-size warning remains.
Native rendered layout/themes/minimum-size/IME and Windows/Linux remain unverified.
Next unfinished task: rebuild/restart, select Low for Z.ai GLM-5.3 and retry;
inspect API request details if it stalls. No live request with the user's key was
made and no milestone checkboxes changed.


### OpenAI-compatible custom provider streaming — 2026-10-14

Explicitly requested extension to the existing Milestone 5 AI workflow: saved
custom providers now execute chapter translation, partial Retry and AI Fix HTML
through POST {base}/chat/completions with stream=true. The Rust transport uses
the saved endpoint/key and selected model; assistant content alone feeds existing
progress events and proposal validation. Existing book/run instructions,
continuation structure checks and immutable source/saved-draft boundaries remain.
No CLI installation is needed for custom providers. Custom effort remains Default;
provider-specific reasoning controls are outside this slice.
SSE decoding handles split UTF-8, CRLF, multiline data and bounded events/output.
Successful completion requires stop plus DONE; truncated/filter/tool/error output
is rejected with recoverable partial text. HTTP failures omit raw provider bodies
and secrets; redirects are disabled. Cancellation/deadline drop the async request,
including during silent streams. Consent text in both dialogs distinguishes the
configured API endpoint from an installed tool. No schema change.
Validation: Rust formatting and full suite passed; seven custom-provider tests
passed after adding the final chapter/Retry regression. Local HTTP tests cover
endpoint/auth/model/stream payload, Unicode deltas, errors, redirects, cancellation,
timeouts, saved-provider HTML repair, chapter retry, snapshot immutability and no
automatic draft persistence. Frontend lint/build and 31 frontend tests passed;
existing bundle-size warning remains. No live GLM request or native rendered UI,
minimum-window/theme/IME or Windows/Linux verification was performed.
Next unfinished task: rebuild/restart Sagar and verify the saved GLM endpoint with
a real chapter, cancellation, Retry and HTML repair. No milestone checkboxes changed.


### Existing EPUB 2 volume navigation — 2026-10-14

The user's Joy of Life snapshot is EPUB 2 with seven nested NCX parents.
Workspace reopening now resolves the NCX referenced by OPF spine@toc when
EPUB 3 navigation is absent, and maps navPoint ancestry to the same sidebar
accordion without changing saved chapter IDs, progress or snapshots. The header
picker shows indented chapter options with volume context and searches ancestor
labels. This is limited NCX workspace navigation compatibility; EPUB 2 export
conformance and per-anchor navigation remain separate unfinished work.
NCX parsing is namespace-aware, reads only navMap and permits the standard
inert NCX doctype without resolving DTDs; internal entities remain rejected.
Validation: frontend lint/build and 30 frontend tests passed, plus the new picker
interaction regression passed separately. Rust formatting and suite passed
(138 unit, 20 export, 20 import; four ignored probes/sweeps). The local snapshot
probe passed explicitly: 758 chapters, 746 grouped; no reimport needed.
Native current-build layout/theme/minimum-window checks and Windows/Linux remain
unverified. Existing bundle-size warning remains. Next unfinished task: rebuild
and restart the desktop app, then verify both chapter controls with this book.
No broader EPUB 2 milestone checkbox was marked complete.


### Expandable volume navigation — 2026-10-14

Milestone 2 chapter navigation now displays nested EPUB 3 TOC parents as
expandable groups, including unlinked span labels and multiple nesting levels.
Linked volume opening pages remain selectable in the group header. Chapters
retain spine order, saved IDs and progress. Search includes ancestor labels;
filter changes and active-chapter changes reveal the relevant groups.
Hierarchy is derived from the immutable snapshot when opening a project, so
existing projects need no reimport or database migration. Missing/unreadable
navigation retains the existing flat fallback. EPUB 2/NCX and navigation to
multiple anchors within one chapter remain unfinished compatibility work.
Validation: frontend lint/build and 30 frontend tests passed; Rust suite passed
(137 unit tests, 20 export tests, 18 import tests, 3 ignored), plus the new
snapshot reopening integration test passed separately. Rust formatting passed.
Synthetic coverage checks nested linked/unlinked groups, spine order, stable
chapter IDs, immutable snapshot bytes, selection, collapse/expand, active
chapter reveal, keyboard movement and Myanmar labels. Existing bundle-size
warning remains. Native rendered layout, minimum window, light/dark appearance,
IME and Windows/Linux interactions remain unverified; the running release has
not been rebuilt. Next unfinished task: rebuild and smoke-test the native
sidebar with the user's volume book, then address EPUB 2/NCX if needed.
No milestone checkboxes changed.


### Windows npm CLI launcher selection regression — 2026-10-14

Fixed a Milestone 5 Windows launch failure: discovery previously selected npm's
extensionless POSIX shim before its Windows launcher. Windows now searches
.exe, .cmd and .bat only, preserving directory precedence. Shared discovery
applies to provider Check, model listing and translation for Codex, OpenCode,
Claude and agy. Unix bare-name lookup is unchanged. Regression coverage stages
all four tools with coexisting shims and verifies launcher preference on any OS;
a Windows-only test probes a .cmd launcher in a directory containing spaces.
Existing staged tests now use platform executable suffixes.
Formatting and the Rust suite passed on macOS (134 unit, 20 export, 18 import;
3 ignored). No frontend changes; frontend checks were not run.
Native Windows Check/translation remains unverified on this macOS host.
Next unfinished task: rebuild Windows Sagar and verify Codex Check and a
translation with the actual install. No milestone checkboxes changed.

### Windows agy install detection regression — 2026-10-14

Fixed the Milestone 5 cross-platform provider lookup to include
LOCALAPPDATA/agy/bin and the normal AppData/Local/agy/bin profile fallback.
Relocated local app data takes priority; relative environment paths are ignored.
The existing shared discovery supplies provider listing, Check and CLI execution.
Regression coverage stages installs without PATH, including spaces, relocation,
missing/relative environment values and duplicate locations. cargo fmt --check
and cargo test passed (133 unit, 20 export, 18 import; 3 ignored).
No frontend or EPUB behavior changed. Native Windows detection and execution
remain unverified on this macOS host. Next unfinished task: rebuild the Windows
app and verify Providers → Antigravity → Check with the installed agy.exe.
No milestone checkboxes changed.

### Release HTML → Rich mode regression — 2026-10-14

Fixed the Milestone 2 editor-readiness race: a parent chapter-reset effect
cleared the editor after the child reported readiness on immediate mount.
Development Strict Mode effect replay hid the issue. Readiness is now scoped
to chapter identity without clearing the current child registration.
The browser-initialized regression harness reproduces the failure before the
fix (its previous SSR/deferred mount concealed it). Coverage now verifies
HTML → Rich after initial mount and chapter navigation without Strict Mode,
including Myanmar text and inline emphasis; validation failures and external
draft updates remain covered. All 26 frontend tests and bun run check passed;
the existing bundle-size warning remains. No Rust code or persistence changed;
Rust checks were not run. Native release, minimum-window, themes, keyboard/IME
and Windows/Linux verification remain open. Next unfinished task: rebuild the
native release and verify view switching with an existing chapter. No milestone
checkboxes changed.

### Muse Spark free-model server regression — 2026-10-14

Reproduced the saved opencode/muse-spark-1.3-contributor-free selection with
synthetic text: the regular CLI succeeded while Sagar's server adapter returned
APIError / HTTP 403 before any output. Blanket deny permissions removed the
normal tool definitions and triggered OpenCode's free-tier access gate, matching
upstream issue #50627. Temporary sessions now require approval (ask), retaining
the normal schema; any actual permission/question request still aborts without
approval and cleans up the session/server. Sharing remains disabled and provider,
model and variant selections are unchanged. No header impersonation or model
fallback introduced.
The exact Muse Spark model now succeeds through the Rust SSE adapter: two text
deltas, first at 2.91s and finished at 2.98s for synthetic short text. This is a
compatibility/streaming check, not a chapter timing benchmark. Provider errors
now classify access/authentication/rate-limit/service/output-limit failures
without echoing raw response bodies, prompts, paths or credentials. Regression
coverage includes an actual tool-approval request being rejected and cleaned
up, plus sanitized provider errors. Rust formatting, 132 unit, 20 export and
18 import tests passed; normally ignored live probe passed explicitly, and the
performance sweep remains ignored. Frontend lint/build and 18 tests passed;
existing bundle-size warning remains. Temporary event diagnostics were removed.
Next unfinished task: rebuild/restart Sagar and retry the actual Muse Spark
chapter; native UI, chapter timing and Windows/Linux remain unverified. No
milestone checkboxes changed.

### OpenCode Server text streaming — 2026-10-14

OpenCode translation and HTML repair now use a per-job Rust adapter for
opencode serve instead of run --format json. The server binds 127.0.0.1 on
an assigned port with a generated Basic-auth password; clients bypass proxies
and redirects. Subscribe to SSE before prompt_async, route only the current
session's assistant text parts/deltas into the existing single text box,
deduplicate final snapshots, and require completed assistant output before
validating the proposal. Preserve provider/model and variant selections.
Server/session cleanup runs on success, cancellation, timeout and errors;
only this adapter's temporary session is aborted/deleted. Runtime configuration
disables sharing while preserving inline settings and leaving user files alone;
managed settings that prevent this are rejected before sending book text.
Temporary session permissions deny tools. No new JS runtime/dependencies or
changes to Codex, Claude or agy transports.
Installed OpenAPI inspection and authenticated synthetic probe verified live
text deltas using zai-coding-plan/glm-5.3: final probe emitted two deltas, first
at 4.94s and finished at 4.97s. Earlier probe emitted five deltas. These are
streaming checks, not chapter speed benchmarks. Rust formatting and 131 unit,
20 export and 18 import tests passed; both live probes are normally ignored
and were run explicitly in their respective tasks. Performance sweep ignored.
Frontend lint/build and all 18 tests passed; existing bundle-size warning remains.
Initial live-probe setup failed because the full provider catalog exceeded the
bounded response limit; the probe now takes an explicitly selected installed
model and the actual translation adapter does not fetch that catalog.
Next unfinished task: rebuild/restart and verify an actual OpenCode chapter
and HTML repair with its selected model/variant in the native app. Native
window/themes/IME and Windows/Linux verification remain open. No checkboxes
changed.

### Codex App Server text streaming — 2026-10-14

Supersedes the stderr display approach, which the user confirmed only showed
startup text until completion. Codex translation and HTML repair now use a
Rust stdio client for codex app-server: initialize, ephemeral read-only thread,
selected model/effort, and a single turn. item/agentMessage/delta feeds translated
text into the existing single output box; final messages are deduplicated and
successful turn completion is required before the existing XHTML/skeleton checks.
Removed the separate diagnostics box. Other providers retain their adapters.
No Node/Python runtime, TCP listener, source mutation or user config changes.
The process is reaped on success, cancellation, timeout and protocol errors;
messages/output are bounded, stale turns/commentary are ignored, and unexpected
interactive actions fail explicitly rather than approving tools.
Verified the installed protocol schema and an authenticated Rust adapter probe
using gpt-6.1-sol / low: six text deltas, first at 4.17s, completion at 4.42s
for synthetic short text. This is evidence of live deltas, not a chapter speed
benchmark. Rust formatting, 125 unit tests, 20 export and 18 import tests passed;
the live probe passed separately (normally ignored), and the performance sweep
remains ignored. Frontend lint/build and 18 tests passed, including early/stale
stream routing, single textarea, Myanmar text, Cancel/Escape and focus restore.
Existing bundle-size warning remains. Next unfinished task: rebuild/restart and
verify the same chapter/model/effort in the native app, with a timing comparison.
Native themes/minimum-size/IME and Windows/Linux remain unverified. No checkboxes
changed.

### Codex live stderr progress — 2026-10-14

Preserved Codex's restored plain exec arguments and streamed its stderr into a
separate Codex progress box in both translation and HTML repair dialogs. Stdout
alone supplies the proposal, validation and retry content. Progress activity
resets the silence timer; stale jobs are ignored and early HTML repair events
are queued until the start response returns. Diagnostic pipes and retained
history are bounded (128 KiB tail); progress updates are throttled and flushed
at completion. No saved draft/source mutation or JSON mode reintroduction.
Rust formatting and all 31 translation-service tests passed. Regression coverage
verifies progress before final stdout, proposal separation, stderr flooding,
cancellation and continued structured output for other providers. Frontend
lint/build and all 18 tests passed; the dialog regression covers early/stale
progress, Myanmar text, readonly output, Cancel/Escape and focus restoration.
Existing bundle-size warning remains. No running Sagar window was available;
native light/dark, minimum-window, IME and Windows/Linux behavior remain
unverified. Next unfinished task: rebuild/restart and verify live progress with
the same chapter/model/effort, including a timing comparison. No checkboxes changed.

### Codex streaming rollback — 2026-10-14

User reported a chapter run increasing from about 140 seconds to over 200
seconds after enabling JSON mode. Restored Codex's previous exec arguments
without --json and its plain stdout execution path for translation and HTML
repair. Other providers retain structured streaming. The timing cause is not
confirmed; no same-chapter performance claim is made. Rust formatting and all
28 translation-service tests passed, including a plain Codex process regression.
Next unfinished task: rebuild/restart Sagar and time the same chapter with the
same model/effort; an already-running job is unaffected by source changes.
Native and cross-platform verification remain unverified. No checkboxes changed.

### Structured streaming for existing CLI providers — 2026-10-14

Codex now uses exec --json, OpenCode uses run --format json, and Claude Code
uses stream-json with verbose and include-partial-messages. Provider decoders
feed response text into the existing translation/HTML repair progress box,
ignore reasoning/tools, deduplicate text snapshots, and require successful
completion before validating proposals. Codex uses the last completed agent
message; OpenCode retains the final assistant message's text parts.
Live synthetic probes verified all three installed CLIs. Claude emitted text
deltas; OpenCode emitted a completed text part; Codex emitted only a completed
agent message. Codex exec therefore does not promise token-by-token output.
Rust formatter and translation service tests, plus frontend lint/build passed;
the existing bundle-size warning remains. Native dialog, keyboard/IME and
Windows/Linux verification remain unverified. Next unfinished task: native
verification and, if requested, a Codex transport with token delta support.
No milestone checkboxes changed.

### Antigravity CLI provider — 2026-10-14

Added Antigravity (agy) to built-in provider discovery/checks, live `agy models`
listing and saved translation selection, with low/medium/high effort controls.
Translation and HTML repair run headless with stream-json; only agent response
deltas appear in progress and the successful terminal response is validated.
Malformed, failed or missing results are rejected. Existing cancellation,
timeouts, isolated working directories and proposal review remain in use.
Verified installed agy 1.1.23 version/model probes. Frontend lint/build and
17 frontend tests passed; Rust unit/export/import tests passed (performance
sweep ignored). Build retains the existing bundle-size warning.
Next unfinished task: native provider selection and authenticated translation/
repair verification at normal/minimum sizes in both themes; Windows/Linux and
native keyboard/IME behavior remain unverified. No milestone checkboxes changed.

### Blockquote and code authoring — 2026-10-14

Enabled inline code and standard pre/code blocks in the chapter editor while
retaining plain pre elements and imported code attributes. Formatting toolbar
now has Blockquote, Inline code and Code block actions with icons, tooltips,
selected states and capability-aware disabled states. Slash menu supports
/blockquote (also /quote), /code and /inline code; applying a command removes
its query and formats in one undoable transaction. Added theme-token styling
for quotes/code, wrapping code lines without losing saved indentation.
All 17 frontend tests, frontend lint/build, and 13 Rust rich-text validator tests
passed. The build retains the existing bundle-size warning. Regression checks
cover Myanmar text, HTML escaping, pre/code round trips, slash-query removal,
undo, inline-code input, formatting toggles and disabled editors.
Native light/dark, minimum-window, toolbar/slash keyboard interaction and IME
checks remain unverified because no current running Sagar build was available.
Next unfinished task: native verification of these controls; broader XHTML
preservation work remains tracked below. No milestone checkboxes changed.


### Rich XHTML preservation expansion — 2026-10-14

The chapter Rich editor now retains structural containers (including imprint
section/header IDs and classes), text-only container variants, definition lists,
preformatted text, inline spans/b/i/code/sup/sub/ruby/rt/rp, inline images,
EPUB semantics, xml:lang, image srcset/sizes and layout attributes. SVG and picture
subtrees use protected nodes with inert editor placeholders and retained markup;
they remain editable in HTML. SVG namespaces are kept during serialization.
Disabled automatic trailing paragraphs and retained significant preformatted
whitespace so changing views does not rewrite these structures.
Regression coverage includes imprint markup, Myanmar text and links, inline
semantics/images, definition lists/pre, SVG namespaces and responsive images,
reload/edit cycles, and inert SVG node views. Frontend lint/build and all 12
frontend tests passed; build retains the existing bundle-size warning. Rust tests
passed (107 unit, 20 export, 18 import; performance sweep ignored).
No running Sagar window was available: native IME, keyboard, minimum-size,
light/dark and cross-platform verification remain unverified.
This is an expansion, not a claim of universal EPUB XHTML support. Next unfinished
task: deliberate models for complex table sections and mixed block/inline
containers, followed by safe MathML/media/form preservation and native verification.
The Rust safety policy and loss-aware view-switch guard remain in force. No
milestone checkboxes changed and no existing unrelated edits were reverted.


### Imprint markup comparison regression — 2026-10-14

Chapter proposal comparison, continuation, normalization and image embedding now
use the same XHTML/EPUB/XLink namespace context as the restricted editor.
This fixes inherited epub:type declarations being lost when extracting a body.
A synthetic imprint regression covers initial runs, partial retries, Myanmar text,
image srcset/link preservation, changed attributes and unknown-prefix rejection.
Rust formatting and 107 unit, 20 export and 18 import tests passed; one performance
sweep remains ignored. Frontend build passed with the existing bundle-size warning.
Combined frontend checks stopped on existing richText.ts formatting differences.
Next unfinished task: verify the imprint chapter in a rebuilt native app;
full proposal acceptance and Windows/Linux behavior remain unverified.
No milestone checkboxes changed.


### XHTML 1.1 import compatibility — 2026-10-14

The shared XML parser accepts the standard inert XHTML 1.1 PUBLIC declaration
without fetching a DTD; internal entity declarations and arbitrary external
DTDs remain rejected. Synthetic coverage checks editable Myanmar content,
preview, draft export, byte preservation and source immutability. Rust formatting
and 106 unit, 20 export and 18 import tests passed (performance sweep ignored).
The supplied book additionally uses named nbsp/copy entities in its verso page,
which remain unsupported in the strict parser. A separate compatibility copy
removes legacy declarations and replaces those references with numeric equivalents
using XML events; all 24 chapters load, 1706 blocks are editable, and draft
export validates. Original source files remain unchanged. Next unfinished task:
native import/edit verification of the compatibility copy; general named-entity
and EPUB 2/NCX compatibility remain open. No milestone checkboxes changed.

### Unpacked EPUB folder import feedback — 2026-10-14

Import now explains that an EPUB folder must be packaged as a .epub archive.
A regression confirms rejection preserves the folder and creates no project
storage. Rust formatting and 18 import tests passed (one performance sweep
ignored). A separate archive of the supplied local folder passed Sagar's
parser with 24 chapters and two book warnings; the source folder was unchanged.
Next unfinished verification: import that archive in the native app and review
its content warnings. Native/cross-platform UI and EPUBCheck remain unverified;
no milestone checkboxes changed.

### SVG cover preview and DOCTYPE export regression — 2026-10-14

Source XML parsing accepts the inert HTML5 doctype while rejecting internal
entity declarations and external DTDs. Chapter loading, segmentation, navigation,
patching and export validation share this behavior. Preview supports a restricted
passive SVG subset, rewrites image href/xlink:href to local snapshot URLs, and
drops scripts, foreignObject, animations, CSS and URL paints. Original SVG bytes
remain preserved in exports; arbitrary SVG authoring is still unsupported.
Synthetic regressions cover cover loading, sanitization, export validation and
resource/source byte preservation. Rust checks passed: 93 unit, 19 export and
17 import tests (large-book sweep ignored). A temporary diagnostic also imported,
loaded the cover and exported the supplied pg11-images-3.epub successfully.
Next unfinished verification: rebuilt native preview/export in both themes and
an external EPUB reader/EPUBCheck; cross-platform rendering remains unverified.
No milestone checkboxes changed.

### AI italic/bold formatting instructions — 2026-10-14

Translation instructions identify em/strong as editable italic/bold formatting
and prohibit introducing unsupported tags while retaining source structure.
AI Fix HTML explicitly converts i/b to em/strong without losing text, valid
attributes or formatting. All five prompt tests passed. Next unfinished check:
verify a native AI Fix HTML run on a draft containing i tags; no milestone
checkboxes changed.

### Reviewed chapter indicator — 2026-10-14

The chapter review button stays green with a tinted background when the draft
status is reviewed, including hover. Its pressed state and accessible label/
tooltip identify Chapter reviewed. Editing resets status to draft through the
existing save service and clears the green state. Lint/build and all four
frontend tests passed. Next verification: native reviewed/edit/reload states
in both themes; no milestone checkboxes changed.

### Running-action button — 2026-10-14

Both AI dialogs use the shared red/white Cancel button with the close icon.
Frontend lint/build and four tests passed. Native rendered verification remains
pending on a current build; no milestone checkboxes changed.

### ZIP directory export regression — 2026-10-14

Export copies validated filenames rather than treating the filtered file list
as raw ZIP indices. Explicit directory entries previously truncated the copy
loop and dropped trailing resources. The supplied local project reproduced a
missing toc.ncx; export from an isolated database/snapshot copy now succeeds,
retains every source resource and preserves navigation/CSS bytes. A synthetic
integration regression covers multiple directory entries and source immutability.
All 18 export tests passed. Next unfinished verification: restart the native
app, export and reopen in a reader; EPUBCheck/cross-platform checks remain open.

### HTML view simplification — 2026-10-14

The user's latest preference supersedes displaying the read-only document
wrapper: HTML view shows only editable chapter content, with no wrapper
explanation. Original head, stylesheet links and body attributes remain
preserved by export. Native verification remains pending on a current build.

### Button consistency — 2026-10-14

All visible text buttons pair their labels with shared icons; icon-only actions
remain supported. Language and export dialog header dismissal uses an accessible
X button, and language saving uses Save/Saving…. Shared tab, heading and language
control spacing accommodates icon/text groups. AGENTS.md records this convention.
Frontend lint/build and all four tests passed; a source audit found no remaining
text-only buttons. The running native app still shows an older build, so current
native light/dark, minimum-size and keyboard verification remains unfinished.
Next task: verify the updated buttons in a rebuilt desktop app. No milestones
are advanced.

### Full XHTML authoring context — 2026-10-14

Translation HTML view shows the original XML declaration, document head,
stylesheet links and opening body above the editable chapter body, followed
by the original closing wrapper. The wrapper is read-only and comes from
namespace-aware source parsing in Rust, with original bytes retained for
ordinary bodies. Empty bodies are displayed with expanded opening/closing
tags. Saved drafts remain body content; export retains the source wrapper.
Regression tests verify stylesheet/head/body-class preservation and prefixed
and empty bodies. Frontend lint/build and four tests, Rust formatting and
the Rust suite passed (large-book sweep remains ignored). Next unfinished task:
restart the desktop development build and verify this view at normal/minimum
sizes in both themes, with keyboard and Myanmar IME checks. Milestones remain
unchanged.

### EPUB anchor preservation — 2026-10-14

Rich chapter editing now represents empty `a[id]` navigation targets as inline
nodes and preserves paragraph whitespace when loading or converting XHTML.
HTML-to-Rich applies the exact parsed document that passed the existing loss
check. A synthetic Myanmar regression fixture covers anchors, links, trailing
spaces, edits and reloads; the supplied chapter also passed a local round-trip
check. Native IME, minimum-window, light/dark and cross-OS checks remain open.
No milestone checkboxes are advanced. Next unfinished task: verify this
conversion and Myanmar composition in the running desktop build.

**Status:** Milestones 0–4 implemented and verified on macOS (import → translate → autosave → export). Milestone 5 is partially done: polish/docs verified on macOS; installers and cross-OS checks remain open (see docs/release-checklist.md).  
**Target:** Windows, macOS and Linux  
**Phase 1:** Manual side-by-side EPUB translation and EPUB export  
**Chosen scaffold:** Tauri 2 + React + TypeScript + **Bun**  
**Phase 2 (deferred):** AI-assisted translation and review

## 1. Product overview

Desktop application for manually translating a book chapter by chapter, without losing the original EPUB formatting or file structure.

**Primary user journey**

```text
Open EPUB
  -> Validate / import source snapshot
  -> Read metadata, OPF spine, TOC and resources
  -> Create local translation project
  -> Choose chapter
  -> View original | edit translated text
  -> Autosave progress and review missing blocks
  -> Repeat for all chapters
  -> Preview output / update language and title
  -> Export translated EPUB (separate file)
  -> Validate and reopen exported book
```

### MVP goals

- Open a **DRM-free, text-heavy, reflowable EPUB 3** book from disk.
- Provide a split editor: **original (left, read-only)** and **translation (right, editable)**.
- Support a chapter list and navigation based on the book's reading order.
- Save each chapter's translation automatically; restore a project after restarting the app.
- Preserve cover, image assets, chapter order, CSS where possible, link targets and footnotes.
- Track status for blocks and chapters; show incomplete content before export.
- Export a standalone translated EPUB file with updated book title/language metadata.
- Work entirely offline with no AI service or login.

### MVP exclusions

AI translation and provider keys, EPUB 2/NCX (until compatibility milestone), fixed-layout, DRM bypass, PDF/MOBI, collaborative editing, cloud storage, embedded authoring/source-code IDE, arbitrary CSS editing and full EPUB accessibility authoring tools.

## 2. UX / screen architecture

Detailed visual and interaction requirements: [Desktop UI/UX design](ui-ux-design.md). This specification supplements the screen architecture below and maps delivery to Milestones 0–5.

### A. Project Library

- **Open EPUB** button and native file picker.
- Recent projects, title/cover/author, source/target language, last edited, completion count.
- Resume an existing project. Re-importing the same source warns about duplicates by content hash.
- Clearly identify unsupported EPUBs and specific reasons.

### B. Translation Workspace

```text
+---------------------------------------------------------------------+
| Book title           Save: Saved           Preview       Export EPUB |
+----------------------+-------------------------+--------------------+
| Chapters             | ORIGINAL                | TRANSLATION        |
|  01  Completed       | read-only EPUB content  | editable blocks    |
|  02  In progress     | source heading/image    | target heading     |
| >03  In progress     | paragraph A             | translation A      |
|  04  Not started     | paragraph B             | translation B      |
|                      | paragraph C             | translation C      |
|                      |                         |                    |
| Book progress        | synchronized block      | status per block   |
+----------------------+-------------------------+--------------------+
| Chapter 3 of 20      | 8 / 12 blocks reviewed   | Autosaved 12:30 PM|
+---------------------------------------------------------------------+
```

- Sidebar: chapters, status (`not_started`, `in_progress`, `completed`), progress.
- Resizable left/right panes. Comfortable Myanmar font and line-height; independent scrolling initially. Block-linked scrolling later, after editor stability.
- Original pane renders protected/read-only content, including images, headings and links.
- Target pane presents a chapter document with restricted rich text, a shared formatting toolbar, and local/current-chapter image insertion. Original pane offers rendered Reader and read-only HTML modes.
- Target pane edits supported content blocks. Users can copy original text into target, save, undo, and mark blocks reviewed.
- Search inside the selected chapter (post-core milestone if necessary).
- Never claim an empty block is complete merely because its chapter was visited.
- Chapter changes wait for or flush pending writes; warn on persistent save failures.

### C. Export / Review

- Set translated book title and target language (`my`, `en`, etc.; store BCP 47 tags).
- Show completed/pending/unsupported block and chapter counts.
- Prevent silently exporting an unfinished book; allow explicit **Export draft** with warning and defined fallback behavior.
- Open native Save dialog, write to a new path, validate ZIP/OPF/reference integrity, and show success/error.
- Never replace the original imported file by accident.

## 3. Component architecture

```text
React + TypeScript (Bun/Vite)                    Tauri 2 / Rust
+---------------------------------+            +------------------------+
| ProjectLibrary                   |            | Tauri commands         |
| TranslationWorkspace            | <--------> | Application services   |
|  - ChapterSidebar               | typed      |  - ProjectService      |
|  - SourceChapterView            | commands   |  - ImportService       |
|  - TargetBlockEditor            |            |  - TranslationService  |
|  - SaveState / Progress         |            |  - ExportService       |
| ExportReviewDialog              |            +----------+-------------+
+---------------------------------+                       |
                                                          v
                                      +---------------------------------+
                                      | EPUB engine                     |
                                      | ZIP/OCF + OPF + XHTML + nav    |
                                      | source resource map / rewrite  |
                                      +----------------+----------------+
                                                       |
                                      +----------------v----------------+
                                      | Local project storage          |
                                      | SQLite + immutable source EPUB |
                                      | drafts / original resources    |
                                      +---------------------------------+
```

### Responsibilities

| Layer | Owns | Must not own |
| --- | --- | --- |
| React UI | display, navigation, editing UX | ZIP parsing, SQLite queries |
| Typed Tauri bridge | DTOs and command calls | business rules |
| Rust services | import, save, status, export orchestration | JSX/UI state |
| EPUB engine | archive, OPF/spine/nav, XHTML transforms | frontend behavior |
| SQLite repository | transactions and migrations | EPUB rendering |

**Initial Rust modules**: `commands`, `domain`, `services`, `epub`, `storage`, `security` (create as implemented, not as empty placeholders).

## 4. EPUB import design

**Import steps**

1. Native open dialog (`.epub` filter).
2. Rust validates file accessibility, ZIP structure, archive sizes/counts, path normalization, duplicate/symlink entries, and `mimetype`.
3. Read `META-INF/container.xml` to locate the OPF package, respecting relative paths.
4. Parse OPF metadata, manifest, spine and EPUB 3 navigation document with namespace-aware XML processing.
5. Resolve spine item IDs to XHTML resource paths. A TOC entry may point to an anchor within a chapter; do not assume one nav entry = one file.
6. Create immutable source snapshot under app project storage and compute `source_sha256`.
7. Extract an ordered block map for editable headings, paragraphs, list items, block quotes etc. Retain the original resource path and stable structural locator for each block.
8. Create project/chapter/block rows and a translation draft transactionally.
9. Display project overview; surface unsupported content with actionable warnings.

**Security:** never extract ZIP paths unchecked or allow embedded scripts to run in a privileged WebView. Restrict local asset serving to app-owned imported resources; prevent network fetches from book content.

**EPUB.js decision spike:** verify chapter preview, inline images, CSS, relative links, Burmese rendering and Tauri WebView security on all three desktop OSes. Prefer a safe document renderer if the preview library cannot satisfy these constraints. Reader rendering must never become the authoritative export source.

## 5. Translation data model (proposed SQLite v1)

The persisted block model is authoritative for manual edits. This schema is a starting point; migrations may refine it after the first import/export spike.

```sql
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  source_sha256 TEXT NOT NULL,
  source_snapshot_path TEXT NOT NULL,
  title TEXT NOT NULL,
  author TEXT,
  source_language TEXT,
  target_language TEXT NOT NULL,
  translated_title TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_chapter_id TEXT,
  schema_version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE chapters (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  spine_index INTEGER NOT NULL,
  manifest_item_id TEXT NOT NULL,
  resource_href TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'not_started',
  UNIQUE(project_id, spine_index)
);

CREATE TABLE blocks (
  id TEXT PRIMARY KEY,
  chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
  block_index INTEGER NOT NULL,
  structural_locator TEXT NOT NULL,
  element_name TEXT NOT NULL,
  source_xhtml_fragment TEXT NOT NULL,
  source_plain_text TEXT NOT NULL,
  is_translatable INTEGER NOT NULL DEFAULT 1,
  support_status TEXT NOT NULL DEFAULT 'supported',
  UNIQUE(chapter_id, block_index)
);

CREATE TABLE translations (
  block_id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE,
  target_xhtml_fragment TEXT,
  target_plain_text TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  revision INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT,
  reviewed_at TEXT
);

CREATE INDEX idx_chapters_project ON chapters(project_id, spine_index);
CREATE INDEX idx_blocks_chapter ON blocks(chapter_id, block_index);
```

**Implementation notes**

- Enable SQLite foreign keys, WAL mode and migrations; keep SQLite in app-owned storage.
- Use UUIDs or equivalent stable IDs assigned **once at import**, never frontend array indexes.
- `structural_locator` is a stable-in-snapshot DOM locator, not a fragile displayed paragraph number. Store resource-relative URI and IDs necessary to remap references.
- `source_xhtml_fragment` represents the original content; keep original EPUB resource bytes as source of truth for export.
- `target_xhtml_fragment` should be a sanitized **inner fragment for a supported block**, not arbitrary XML from the browser. Validate element/attribute allowlists and namespaces before serialization.
- Manage changes with optimistic `revision` checking so a delayed autosave cannot replace a newer edit.
- Do not duplicate full book text in analytics or logs. SQLite stores translation working data only.

### Block/chapter status rules

- Translation block: `pending` (no reviewed content), `draft` (saved but unreviewed), `reviewed` (user confirmed), `skipped` (user intentionally retains original).
- Chapter: `not_started`, `in_progress`, `completed` (derived from required block states; optional cached status must be kept consistent).
- Unsupported content: tracked separately; cannot be auto-marked complete.
- Completion percentage: `reviewed + intentional skipped` / total supported required blocks, with unsupported warnings shown separately.

**Important editorial trade-off:** paragraph-to-paragraph alignment makes progress/scrolling reliable, but some translations need to merge or split sentences. Keep alignment at **block level**, allow free editing *within* a target block; defer cross-block merge/split to a deliberate later milestone. Nontrivial structures (tables, ruby, MathML, complex footnotes) must be detected rather than corrupted.

## 6. Tauri command contract (initial proposal)

TypeScript wrappers should return typed DTOs and map domain errors to user-readable states. Command names/DTOs may evolve, but the responsibilities should remain stable.

```ts
type ProjectId = string;
type ChapterId = string;
type BlockId = string;

type TranslationStatus = 'pending' | 'draft' | 'reviewed' | 'skipped';

type BlockDTO = {
  id: BlockId;
  sourceHtml: string;      // sanitized for display, never executable
  sourceText: string;
  targetHtml: string | null;
  status: TranslationStatus;
  revision: number;
  supportStatus: 'supported' | 'unsupported';
};

type ExportOptions = {
  title: string;
  targetLanguage: string;
  mode: 'completed' | 'draft';
};
```

| Command | Input | Output |
| --- | --- | --- |
| `import_epub` | selected source path | `ProjectSummary`, import warnings |
| `list_projects` | none | project summaries |
| `get_project` | `projectId` | project metadata/progress |
| `list_chapters` | `projectId` | ordered chapter summaries |
| `get_chapter` | `chapterId` | blocks + read-only resource locator |
| `save_translation` | `blockId`, target fragment, status, expected revision | saved revision + timestamp |
| `update_project_metadata` | `projectId`, translated title, language | project |
| `review_export` | `projectId` | missing/unsupported/invalid content report |
| `export_epub` | `projectId`, destination path, options | output path + validation report |

**Do not trust paths or HTML passed from React.** Rust validates file selection permissions, canonicalized paths, and output safety independently. Keep long imports/exports off the UI thread and report progress or cancellation when appropriate.

## 7. Autosave and recovery flow

```text
User edits a target block
  -> local UI draft updates immediately
  -> debounce (e.g. 400-800 ms; tune after testing)
  -> save_translation(blockId, content, status, expectedRevision)
  -> SQLite transaction updates content + revision + timestamp
  -> UI confirms Saved / shows failure and retry

Before chapter change / export / app close:
  -> flush pending edits where possible
  -> wait for acknowledged writes
  -> abort export and warn if save failed

On reopen:
  -> load project, selected chapter, saved translations
  -> resume from latest committed revisions
```

Avoid relying solely on an app-close callback (crashes and forced closes skip it). Consider a small in-memory unsaved-draft queue for transient failures, but don't represent uncommitted drafts as safely stored. Add recovery tests for killed/restarted processes once core persistence exists.

## 8. Export pipeline — most important correctness requirement

```text
User clicks Export
  -> flush saves
  -> review missing, skipped and unsupported blocks
  -> collect metadata and draft/complete decision
  -> copy immutable source EPUB into a temporary work archive
  -> map each translated block to its original XHTML resource
  -> replace only supported, intended content fragments
  -> update translated title/language + TOC labels where editable
  -> update nav/NCX as applicable; preserve spine/IDs/hrefs/assets
  -> produce OCF-correct ZIP
  -> validate internal references, XML, resource files
  -> EPUBCheck (optional developer/integration step)
  -> reopen output as a reader smoke test
  -> finalize destination file atomically where supported
  -> report export result and any warnings
```

### Export rules

- For Phase 1, **do not convert** the entire source book to Tiptap JSON and back. Patch only approved translated blocks into a copy of the original XHTML tree.
- Maintain original outer tags and stable resource IDs where possible; preserve embedded anchors/footnotes and reject edits that would break them.
- Rewriting a block must be XML-safe: escape text, serialize valid XHTML, maintain namespaces, and preserve required semantic attributes.
- Resource-level originals (covers, images, fonts, CSS, package data) stay unchanged unless deliberately edited.
- Package metadata: update `dc:language`, translated `dc:title` if provided; retain authors, publication data, identifiers by default; choose edition/identifier policy explicitly before publicly distributing translated editions.
- Translate visible TOC labels as a defined field or from reviewed headings; maintain each target `href`. Never blindly translate nav XML text without verifying its intended mapping.
- Write OCF `mimetype` entry **first, uncompressed**. Preserve `META-INF/container.xml`, manifest entries and spine order.
- For **draft export**, untranslated supported blocks retain original text with a clear warning/report; unsupported content is preserved unchanged. For **completed export**, block if required untranslated/unsupported items remain unless the user has explicitly reviewed a skip policy.
- Save to a different path than the source file or its immutable snapshot; confirm before replacing any unrelated existing output.
- Use temporary output and safe rename/copy fallback, avoiding half-written destination files.
- Export failures must not modify saved project translations.

## 9. Implementation roadmap

Checkboxes are checked only after the functionality exists and its acceptance criteria are verified.

### Phase files

The files below break down the existing roadmap without changing its scope or completion status. Numbered phase files follow Milestones 0–5 within product Phase 1; compatibility and AI remain deferred tracks.

- [x] [Milestone 0 — Bootstrap & architecture foundation](phase-0-bootstrap.md)
- [x] [Milestone 1 — EPUB import and chapter listing](phase-1-epub-import.md)
- [x] [Milestone 2 — Side-by-side reading/translation UI](phase-2-translation-workspace.md) (independent chapter rich text documents and image insertion; native IME checks pending)
- [x] [Milestone 3 — Local autosave, progress and resume](phase-3-autosave-and-resume.md)
- [x] [Milestone 4 — EPUB export (vertical-slice first)](phase-4-epub-export.md) (EPUBCheck pending)
- [ ] [Milestone 5 — Polish & desktop release](phase-5-desktop-release.md) (macOS polish done; installers/cross-OS open)
- [Milestone 2 — Side-by-side reading/translation UI](phase-2-translation-workspace.md)
- [Milestone 3 — Local autosave, progress and resume](phase-3-autosave-and-resume.md)
- [Milestone 4 — EPUB export (vertical-slice first)](phase-4-epub-export.md)
- [Milestone 5 — Polish & desktop release](phase-5-desktop-release.md)
- [Later — EPUB compatibility improvements](later-epub-compatibility.md)
- [Later — AI translation (explicitly NOT Phase 1)](later-ai-translation.md)

### Milestone 0 — Bootstrap & architecture foundation

- [x] Verify generated Tauri 2 + React + TS project runs with **Bun** on the current development OS.
- [x] Remove demo UI; create minimal app shell and navigation.
- [x] Add strict TS checks, format/lint conventions and relevant scripts.
- [x] Add Rust `domain`, `services`, `epub`, `storage` structure as functionality is introduced.
- [x] Implement typed Tauri command bridge and reusable error reporting.
- [x] Add SQLite schema/migration initialization with an automated persistence test.
- [x] Add native dialog plugin and narrowly scoped capabilities for import/export; avoid broad filesystem permissions.

**Done when:** `bun tauri dev` launches, the frontend invokes a typed Rust test command, and a local SQLite migration test passes.

### Milestone 1 — EPUB import and chapter listing

- [x] Implement archive and untrusted-input safety limits.
- [x] Parse container.xml + OPF manifest/spine/metadata for EPUB 3.
- [x] Parse EPUB navigation and resolve ordered chapter titles/anchors.
- [x] Copy immutable source snapshot, hash it, and create project rows.
- [x] Build stable editable block map for supported XHTML elements.
- [x] Surface unsupported EPUB feature warnings instead of failing silently.
- [x] Show imported project in library and chapter sidebar.
- [x] Add fixtures: simple multi-chapter, nested paths, images, links, missing OPF, malformed ZIP.

**Done when:** importing a fixture shows correct title, author, source language, cover if present, and spine-ordered chapters. Imported original is not modified.

### Milestone 2 — Side-by-side reading/translation UI

- [x] Design resizable 3-panel workspace (chapters, source, target).
- [x] Render source chapter safely with images and applicable book CSS.
- [x] Display target editor bound to stable source-block IDs.
- [ ] Support basic editing: Unicode text, paragraphs/headings, emphasis and inline links where safely mapped.
- [x] Add keyboard navigation, selection/focus, and clear save/error state.
- [ ] Test Myanmar Unicode line wrapping, copy/paste and multi-line content.
- [x] Validate EPUB.js/preview adapter compatibility with Tauri WebViews.

**Done when:** user can read one imported chapter and edit corresponding target blocks without altering source or breaking referenced images/links.

### Milestone 3 — Local autosave, progress and resume

- [x] Debounced autosave with revision conflict protection and retries.
- [x] Persist `pending`, `draft`, `reviewed`, `skipped` states.
- [x] Derive chapter status from required block review status.
- [x] Restore project and last chapter after a restart.
- [x] Flush saves before navigation and export.
- [x] Add recoverability and save-failure tests (including process interruption where feasible).

**Done when:** edited translations survive app restart and chapter switching without data loss; failures are reported honestly.

### Milestone 4 — EPUB export (vertical-slice first)

- [x] Implement a 1-chapter translated EPUB round trip on an intentionally simple fixture.
- [x] Patch translated XHTML fragments into source resource tree without flattening assets.
- [x] Repackage EPUB with correct OCF entry ordering/compression.
- [x] Update title, language and navigation labels without breaking hrefs.
- [x] Add native Save dialog, explicit draft mode and unfinished-content review.
- [x] Validate output ZIP, XHTML parsing, manifest targets and links.
- [x] Reimport the exported file and compare chapter/resource counts and spine order.
- [x] Add regression fixtures for formatting, internal links, images and footnotes.

**Done when:** a fully manually translated fixture exports, passes the project's integrity checks, reopens in a reading app, and retains its structural assets. EPUBCheck should also pass on supported compliant fixtures in CI/dev test environments.

### Milestone 5 — Polish & desktop release

- [x] UX review: dark mode, pane resizing, text zoom, shortcuts, error dialogs.
- [ ] Accessibility and Burmese text rendering cross-OS tests.
- [x] Performance checks with large books (bounded RAM/use lazy loading; 5,001 chapters / 56.9 MB macOS sweep, see Milestone 5 evidence).
- [ ] Package and smoke-test installers for Windows/macOS/Linux.
      (macOS .app/.dmg built and smoke-tested; Windows/Linux pending.)
- [x] Document backups, privacy, supported EPUB limits, and recovery.
- [x] Release checklist covering data-loss cases and export validation.

**Done when:** build artifacts install/open/export reliably on all supported desktop systems and known limitations are documented.

### Later — EPUB compatibility improvements

- [ ] EPUB 2 + NCX, and complex navigation structures.
- [ ] Advanced tables, ruby, semantic footnotes, SVG/MathML, media overlays as deliberate supported cases.
- [ ] Multi-block translation restructuring (join/split) with stable export mappings.
- [ ] Glossary/terminology, find/replace, side-by-side block scroll synchronization.
- [ ] Project import/export/backup files and manual revision history.

### Later — AI translation (explicitly NOT Phase 1)

- [ ] Provider-independent `TranslationProvider` interface with cancellable jobs.
- [ ] Prompt/context builder with glossary and per-block identity.
- [ ] API key security and opt-in network/privacy disclosures.
- [ ] Proposals stored as separate drafts, not automatic overwrites of reviewed manual translations.
- [ ] Batch progress, retry, costs, rate limits and user review.

## 10. Quality gates / test matrix

| Test case | Expected result |
| --- | --- |
| EPUB 3 book with non-alphabetical filenames | Correct OPF spine order |
| Nested OPF/XHTML/assets paths | References resolve correctly |
| Cover + images + CSS | Visible in source, retained in export |
| Bold/italic + internal link + footnote | Original semantics/targets survive export |
| Very long chapter | Smooth navigation, bounded memory |
| Burmese/Myanmar combining characters | No text corruption; UTF-8 intact |
| App closed during autosave | Last committed translation loads; unsaved state not falsely marked saved |
| Chapter changed quickly after typing | Buffered edits are flushed |
| ZIP traversal / huge decompressed payload | Safe rejection with meaningful error |
| Fixed-layout, DRM or unsupported block | Explicit warning; no silent corruption |
| Export with empty translations | Clear draft warning; never mislabel as complete |
| Export onto original input path | Rejected |
| Completed translated EPUB | ZIP valid, navigation and content readable, EPUBCheck passes for supported fixtures |
| Windows/macOS/Linux reader variations | Fonts, links and layout remain usable |

### Suggested command checks

```bash
bun install
bun tauri dev
bun run build
cd src-tauri && cargo fmt --check && cargo test
# Add these once configured in package.json/CI:
# bun run lint
# bun run test
# cd src-tauri && cargo clippy -- -D warnings
# java -jar <path-to-epubcheck.jar> <exported.epub>
```

## 11. Known technical risks / mitigations

| Risk | Mitigation |
| --- | --- |
| WYSIWYG serialization drops EPUB-specific inline markup | Restricted editor schema, original DOM preservation, explicit compatibility warnings and round-trip fixtures |
| Nav entries don't map 1:1 to XHTML files | Maintain spine item/resource/anchor models separately |
| WebView differences across OSes | Cross-OS EPUB.js spike and automated smoke fixtures |
| Tauri privilege exposure to imported HTML | Sandboxed/sanitized preview, restrictive CSP/capabilities, no network access by default |
| Crashes or delayed autosave | Transactional revisions, save state, flush on navigation, periodic persistence |
| Corrupt or unsafe EPUB ZIP | Input size/path/resource limits, XML validation |
| Valid-looking ZIP fails EPUB standard | OCF checks plus independent EPUBCheck fixture validation |
| Myanmar font shaping/line breaking differs by OS | Font fallback strategy, cross-platform visual tests, no string indexing by code unit |
| Huge books cause memory spikes | Lazy chapter load, storage-backed resource map, bounded parsing and previews |

## 12. Next action — first coding slice

**Start with Milestone 0** in the already-created React/TypeScript/Bun scaffold. Do **not** recreate the scaffold.

1. Confirm `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` and run `bun install` / `bun tauri dev`.
2. Create minimal Project Library and Translation Workspace shell (mock data permitted for UI only, clearly marked).
3. Add a typed Tauri command (e.g., `health_check`) and Rust service wiring.
4. Add local SQLite initialization and a migration test.
5. Implement a small EPUB **import/export proof of concept** on a synthetic EPUB 3 fixture before committing to a rich text schema. Verify lossless preservation of links, inline formatting and images.

**Do not start AI integration until the user explicitly changes the phase.**

## Technical references

- Tauri 2 create project & Bun commands: https://v2.tauri.app/start/create-project/
- Tauri file dialog: https://v2.tauri.app/plugin/dialog/
- Tauri permissions: https://v2.tauri.app/learn/security/using-plugin-permissions/
- EPUB overview / package, spine and navigation: https://www.w3.org/TR/epub-overview-34/
- EPUB specification: https://www.w3.org/publishing/epub/
- EPUBCheck validator: https://github.com/w3c/epubcheck

### Workspace revision — 2026-10-13

Milestone 2 follow-up replaces plain text fields with restricted Tiptap passage editors presented as a document, adds Reader/HTML source views and image insertion with EPUB manifest packaging. See [Milestone 2](phase-2-translation-workspace.md) for verified behavior and limitations. Native Myanmar IME/copy-paste and Windows/Linux remain unchecked; no release milestones were advanced.

### Chapter authoring correction — 2026-10-13

The user's clarified workflow supersedes the block-aligned editor/merge-split
restriction in this plan: each chapter has one independent, initially empty
rich text document. Paragraph selection is not required. Chapter IDs own saved
drafts, review and export; immutable source blocks remain for import and legacy
recovery. Export replaces the corresponding XHTML body with the authored
document. See Milestone 2 for preservation limits and verification.

Translation HTML authoring is now explicitly requested: the target pane has
an editable HTML body view sharing chapter autosave with Rich text, with
validation and a loss-aware check before converting back. This supersedes
the earlier HTML-authoring exclusion for the supported chapter schema; the
source HTML view stays read-only.

### Writer shortcuts — 2026-10-13

The editor has a `/` block menu for Text, H1–H3, Table and Image, with keyboard
filtering/selection and pointer selection at the caret. Basic authored tables
are now an explicitly requested feature, using the existing chapter save and
EPUB export services. See Milestone 2 for validation and remaining native checks.

### Per-book settings follow-up — 2026-10-13

Explicitly requested book metadata and AI instruction settings are implemented
as an extension of the export/workspace slice and the authorized AI assist
work. See [AI follow-ups](later-ai-translation.md) for fields, prompt behavior,
preservation tests and remaining native/cross-platform checks. Release and
future AI milestone checkboxes are unchanged.

### Custom cover follow-up — 2026-10-13

The explicitly requested custom cover feature is implemented in Book Settings
and EPUB export using local SQLite storage and an export-only asset override.
See Milestone 4 for behavior, regression tests and native verification limits.
Release checkboxes remain unchanged.

### Library views follow-up — 2026-10-13

The Home library supports the existing table and a responsive cover grid through
accessible view buttons beside Search. The view preference is saved locally;
search, open, progress, Book Settings and removal share the same project data
and actions in both layouts. This is a Milestone 5 UI polish follow-up.
`bun run check` passed (lint, TypeScript and production build); Vite reports a
large bundle warning. No running development build was available for rendered
light/dark, minimum-window, keyboard or native interaction verification.
Next unfinished task: verify both layouts in the desktop app at 1280×800 and
960×640 in both themes; Windows/Linux release checks remain open.

### Selected-content translation — 2026-10-15 (explicitly authorized)

The AI dialog retains Whole chapter as its default and adds Selected paragraphs
with durable text/table/figure/image selections, bounded sequential requests,
preview, and explicit append into the independent chapter draft. Migration 10
stores stable source identities, configuration snapshots, checkpoints and atomic,
idempotent append receipts. Images and source markup are reconstructed locally;
image bytes and paths are excluded from provider requests. Unsupported image
structures remain visible with reasons. See [AI follow-ups](later-ai-translation.md)
for implementation and verification details. No release milestone is advanced.

Verification: 47 frontend tests and 206 Rust tests passed (four existing ignored
tests); lint, TypeScript, production build and Rust formatting passed. Rendered
synthetic-fixture checks covered both themes at 1280×800 and 960×640, preview
images, Myanmar text, keyboard focus and Escape. Native IME, live providers,
Windows/Linux and EPUBCheck remain unverified. Next unfinished task: rebuild and
restart the native app, then verify consented generation, cancellation, retry,
append and export/reopen with Myanmar IME and real book assets.

### Selected translation usability — 2026-10-15

Selected paragraphs now uses Select → Translate → Review & Add stages. One
scrolling content area sits between stationary workflow controls and contextual
footer actions. Review pairs each original and validated proposal with actual
language labels, while source selection, history, recovery and append safeguards
remain distinct. The backend and saved-data formats are unchanged.

`bun run check` and all 56 frontend tests passed, including new pagination,
image-only, recovery, already-added and idempotent retry coverage. Browser checks
with synthetic data covered light/dark at 1280×800 and 960×640, plus stacked
review at 780px. Native desktop, Myanmar IME and Windows/Linux remain unverified.
Next unfinished task: verify this workflow in a rebuilt native Sagar with a real
consented provider run and real book assets.

### Review selection clarification — 2026-10-15

Review’s misleading “Clear results” action is now “Clear selection” and disabled
when nothing is checked. Help text explains that clearing does not delete saved
proposals or draft writing. Already-added results remain protected by default;
Review now exposes an explicit re-add confirmation directly beside the results,
with duplicate-anchor and manual-deletion limitations. Turning it off clears
selections. Saved proposals and draft content are retained.

Verification: `bun run check` and 57 frontend tests passed, including the exact
already-added/clear-selection/re-enable-checkbox case. The production bundle
warning remains. This small control change has not been rechecked in the native
app; next unfinished task is native interaction verification. Deleting proposals
or previously authored draft text is not part of this selection-only fix.

### Delete saved AI proposals — 2026-10-15

History now offers “Delete this saved run…” with an explicit irreversible-delete
confirmation. Rust rejects active jobs and mismatched chapters. Migration 11 adds
a deletion marker; one transaction removes proposals, part outputs and saved
configuration, hides the run, and preserves only identity mappings needed by
append receipts. Draft writing and already-added history remain unchanged.
Deleting the last run returns to source selection; another remaining run can
still be reviewed. No provider request is made.

Verification: `bun run check`, 58 frontend tests and the Rust suite passed
(209 passed, four existing ignored). Regression tests cover confirmation/cancel,
draft preservation, persisted insertion history, deleted-run resume rejection,
and idempotent append replay. The build’s bundle-size warning remains. Native
rendered interaction and Windows/Linux checks are still pending; the next task
is checking History deletion in the rebuilt desktop app.

### Restore manually removed content — 2026-10-15

Insertion history no longer disables source or result checkboxes. “Added before”
explicitly describes history rather than claiming text is still present in the
current draft. Users can select previously inserted results and confirm re-add
in the visible footer, or select source items again after deleting proposals.
Bulk selection includes these items; Select untranslated now uses absence of a
saved proposal rather than treating insertion history as current content.
Confirmation resets when the proposal selection/run changes. Duplicate-ID,
revision and append-order validation remain in Rust. This supersedes earlier
UI rules that locked previously added content. No book reset is required.

Frontend checks and 58 tests passed, including manual HTML removal, preserving
other writing, confirmation and restoring saved proposals. A Rust integration
fixture covers removal of an element with an ID followed by safe re-add while
retaining the run. Native rendered verification remains pending; next unfinished
task is testing this recovery flow in the rebuilt desktop app.

### Accumulate part-by-part chapter proposals — 2026-10-15

Review is now chapter-wide rather than restricted to the selected saved run.
Ready proposals from all retained batches are combined in source order; starting
another batch or inspecting history no longer clears prior result selections.
One proposal version per source occurrence is displayed. The newest ready
version is the default unless a previous version was explicitly chosen or
selected for append. A version picker exposes alternatives, with the correct
model, batch and source/target languages for each. Failed newer batches do not
hide older completed proposals. Progress/retry/deletion still target the chosen
run; append reuses existing cross-run Rust validation and atomic persistence.

Verified `bun run check`, 61 frontend tests and nine selected-content Rust
integration tests. Regressions cover translating 1–3 then 4–6 and appending all
six, retaining checked proposals, reopening with a newer failed run, choosing
older versions, and preserving manual text/source order in cross-run append.
Rendered synthetic fixtures showed six accumulated results and visible footer
at 960×640 light and 1280×800 dark. Native/provider and cross-platform checks
remain open; next unfinished task is that same two-batch workflow in rebuilt
native Sagar. The existing bundle-size warning remains.

### Visible append confirmation and save feedback — 2026-10-15

Append placement confirmation now lives in the stationary footer beside Add,
combined with re-add permission when both apply. Add is disabled until required
confirmation is checked; consent resets when the selection/run changes. A Rust
order warning also reveals this footer confirmation without clearing checked
results or changing the append request identity. Save failures offer “Retry
adding results,” which retries the append rather than merely reloading history.
Successful appends show an immediate saved confirmation in that same footer.

Rust no longer requests out-of-order permission solely from insertion history
when the saved current draft is empty. Existing nonempty-draft order validation,
duplicate IDs and transactional saves remain protected. Tests cover proactive
confirmation, server-requested confirmation, actual append retry, and restoring
an emptied draft with historical inserts. `bun run check` and 65 frontend tests
passed; all ten selected-content Rust integration tests passed. Rendered
synthetic fixtures verified visible confirmation and success at 960×640 light
and confirmation at 1280×800 dark. Native/provider and Windows/Linux checks
remain pending; next unfinished task is checking this append flow in rebuilt
native Sagar. The existing build bundle warning remains.
Full Rust formatting and test verification also passed: 212 tests passed, four
existing tests ignored.

### Consume proposals after adding — 2026-10-15

Review now holds translations waiting to be added. Successful append removes
accepted proposal output and part checkpoints in the same SQLite transaction
as the draft/receipt commit. Older ready versions of those source occurrences
are cleared too, so a previous version does not reappear as pending. Unadded
units stay available across batches. Failed saves retain proposals. Compact
item/receipt identities preserve insertion history and idempotent replay;
source assets and the chapter draft are unchanged. Legacy receipted output is
cleaned on chapter selection-state load. Newly translated proposals for a
previously inserted source occurrence remain eligible.

This supersedes retaining accepted proposals for later restoration: after
manually removing accepted draft content, source items can be selected and
translated again without deleting the chapter/book. “Added” is terminal queue
state, so Resume never regenerates consumed output. Review shows an explicit
empty state when all results were added.

Verified frontend lint/TypeScript/build and 66 frontend tests; Rust suite passed
213 tests with four existing ignored tests. Coverage includes partial acceptance,
older versions/checkpoints cleanup, preserving unadded items, receipt replay,
legacy cleanup and translating previously accepted source again. Native rendered
checks for this updated empty state remain pending; next unfinished task is
checking accept/empty/retranslate in rebuilt native Sagar. The existing bundle
warning remains.

### Replace pending translations for the same paragraph — 2026-10-15

A successful, fully validated translation now atomically replaces the previous
pending proposal for the same stable source occurrence, regardless of model.
Old output and checkpoints are deleted and its item becomes terminal Superseded;
failed/cancelled/incomplete attempts do not discard the previous ready result.
A delayed completion from an older run cannot replace a newer successful run.
Existing multiple ready proposals are reduced to the newest successful one when
selection state loads. Review has no version picker, and replacement clears
selection of the old proposal so the new result can be reviewed before append.
Other paragraphs and authored draft text are unchanged. This supersedes the
earlier saved-version selection design.

Verified frontend checks and tests, plus the full Rust suite (214 passed, four
existing ignored). Regressions cover another model failing then succeeding,
old-output cleanup, unchanged manual draft, late completion ordering, and a
single replacement result in Review. Native rendered checks of this simplified
review remain pending; next unfinished task is native model-change/retranslate
verification. The existing build bundle warning remains.

### App language in Appearance — 2026-10-16

Settings → Appearance now offers English and မြန်မာ (Myanmar). English is the
initial/fallback language; the selected app language is saved locally alongside
presentation preferences, restored on startup, and applied immediately through a
React context with the document language updated for accessibility. Language
changes do not remount the app or change book text, translation language settings,
source snapshots or saved drafts. Shared UI typography includes Myanmar-capable
font fallbacks.

A local Myanmar message catalog and translation hook cover Appearance and the
main library, chapter navigation, reader, authoring, Book Settings, export and
existing AI workflow labels/help text. Missing messages safely retain English.
Some computed/dynamic messages, editor-generated menus, technical diagnostics,
backend/provider errors and native OS dialogs still retain their existing text;
this is an initial localization pass, not a claim of complete string coverage.

Validation: `bun run check` passed (lint, TypeScript, production build; existing
bundle-size warning remains). All 80 frontend tests passed, including new
language default/validation, fallback, immediate switching, focus preservation,
unchanged theme/text size and restoration regressions. Browser rendering at
1207×1256 verified Myanmar in light/dark Appearance, keyboard language/theme
selection, restoration after reload, and Escape/focus restoration. Native
Tauri, minimum-window-size and Windows/Linux rendering remain unverified. No
Rust or EPUB processing changed, so Rust checks were not rerun.

Next unfinished task: finish dynamic/technical message and editor-menu coverage,
then verify localization in native Sagar at normal/minimum window sizes on the
supported platforms and review Myanmar terminology with a native speaker.

Appearance preview correction: the English sample “The next chapter begins here.”
now stays English with `lang="en"` in either app language; the Myanmar sample
stays Myanmar. Removed the sample from the interface catalog. Frontend checks
and all three localization tests passed, including a regression assertion for
the English sample under Myanmar UI. Rendered/native checks were not repeated
for this text-only correction; the next unfinished localization tasks above
remain unchanged.

### Simplified Chinese app language — 2026-10-16

Added 中文（简体） to Settings → Appearance. The `zh` preference persists across
restarts, updates the document language to `zh-Hans`, and uses a Simplified
Chinese catalog for the existing localized interface. English remains the
fallback for messages not yet in the catalog. The English sample in the
Appearance preview remains hard-coded English and is excluded from catalogs.
Regression coverage checks Chinese translations and fallback, all three picker
options, persistence/invalid stored values, immediate Chinese selection, the
`zh-Hans` document language and the English preview. Lint, TypeScript, build and
all three focused localization tests passed; the existing bundle-size warning
remains. A 1280×1280 browser rendering verified the Simplified Chinese picker,
Appearance controls and English-only preview in dark mode. Native Tauri and
minimum-window-size checks remain for the localization follow-up listed above.


### Japanese app language — 2026-10-16

Added 日本語 to Settings → Appearance as the fourth app language. Japanese is
saved and restored with the other language preferences; choosing it applies the
Japanese catalog and sets the document language to `ja`. The English Appearance
preview sentence is absent from all language catalogs.

Lint, TypeScript, production build and the three focused localization tests
passed, including four picker options, Japanese selection and persistence,
`ja` document language, Japanese translation/fallback and the English-only
preview. Browser/native Japanese rendering and minimum-window-size checks remain
part of the outstanding localization follow-up. The existing bundle-size
warning remains.

### Korean app language — 2026-10-16

Added 한국어 as a fifth choice in Settings → Appearance. The app stores the `ko`
language code, restores it on launch, sets the document language to `ko`, and
uses a Korean catalog for the interface strings covered by localization. The
English Appearance sample remains outside the catalogs.

`bun run check` passed (lint, TypeScript and production build; existing bundle
size warning remains), as did all three focused localization tests. Browser
rendering verified the Korean picker and Appearance labels in dark mode. Native
Tauri and minimum-window-size checks remain part of the localization follow-up.

### Thai app language — 2026-10-16

Added ไทย as a sixth choice in Settings → Appearance. The `th` language code is
saved and restored, and selecting Thai applies the Thai catalog and sets the
document language to `th`. The English Appearance sample remains outside all
language catalogs.

`bun run check` passed (lint, TypeScript and production build; existing bundle
size warning remains), as did all three focused localization tests, including
Thai selection/persistence and English fallback. Native Tauri and minimum-window
checks remain in the localization follow-up.

### Simplify provider settings — 2026-10-16

Replaced library-style provider rows with compact settings rows. Coding
assistants show their name, availability and a labeled Check icon; custom
providers show name/model, status and consistent Test, Edit, Details and Delete
icons with tooltips. URLs and API-key state expand on request, and connection
help/privacy explanations are collapsed instead of occupying the top of every
settings tab. The provider form keeps protocol-specific help collapsed and
focuses the first text field when opened. Connection failures stay visible;
delete requires an explicit inline confirmation. Focus trapping now includes
selects and disclosures and skips hidden content. New labels are translated in
all supported interface catalogs.

`bun run check` passed, with the existing bundle-size warning. All 81 frontend
tests passed, including a new regression for details, failed tests and confirmed
versus cancelled deletion; the existing provider-form test uses the shorter
Add provider label. A synthetic browser fixture verified Myanmar in light and
dark, a 1280×720 preview and a component constrained to 960×640, details expansion,
visible failures, keyboard focus on delete cancellation, form focus and retained
inputs after validation. The fixture was removed after verification. Native
Tauri/provider integration and Windows/Linux rendering were not rerun. Next
unfinished task: verify the compact settings in rebuilt native Sagar alongside
the remaining localization coverage work.

### Compact Appearance settings — 2026-10-16

Appearance now has aligned language/theme rows and a grouped text-size slider,
current-size badge and preview. The duplicate visible heading and default help
paragraphs were removed; the same shared Settings help disclosure used by
Providers retains language, autosave and keyboard-shortcut guidance. New short
labels are translated in all supported catalogs. The fixed English and Myanmar
preview samples are preserved. Slider value announcements use the selected
interface language.

Four focused localization/provider regression tests passed. Browser fixtures
verified Myanmar at 1280×720 and within 960×640 component bounds, light/dark
appearance, keyboard theme selection, slider End → 28px with immediate preview,
and help expansion. The temporary fixture was removed. Native Tauri and other
platform rendering remain unverified. Next unfinished task: verify both compact
settings tabs in rebuilt native Sagar with the remaining localization work.
Lint, TypeScript and production build also passed; the existing bundle-size
warning remains.

### Compact Translation Model settings — 2026-10-16

Translation Model now uses compact shared settings rows, searchable model names
with a visible selected state, and a persistent footer for Save and saved/unsaved
feedback. Model descriptions, catalog diagnostics, manual IDs and reasoning
explanations live under More options. Unknown saved/manual IDs expand options
so the value remains editable. Provider-default effort restrictions are retained;
switching agents clears the pending model and disables Save until one is chosen.
Save errors appear in the footer and retain the pending selection for retry.
Appearance and model settings now share the same row/label/select styles.

`bun run check` and eight focused settings/localization/AI-progress tests passed.
Browser fixtures checked Myanmar in light and dark, a 1280×720 preview and
960×640 component bounds, search and selection, custom IDs, saving/failed-save/
retry feedback, expanded options, and disabled Gemini reasoning controls. The
fixture was removed. Native Tauri/provider calls and cross-platform rendering
remain unverified. Next unfinished task: verify all three compact settings tabs
in rebuilt native Sagar and finish remaining localization coverage. The existing
bundle-size warning remains.

### Compact Book Settings — 2026-10-16

Book Settings now separates its short title from a single-line book subtitle,
uses the shared tab style with arrow-key navigation, and groups cover controls
into a small preview/status/chooser row. The native file input is operated by a
labeled Choose cover button. Export title has its own full-width row, with
author/publisher and source/target languages aligned below. Description expands
when populated or requested; longer export and instructions guidance is in help
disclosures. The instructions editor gets a focused writing area. Both tabs
share a sticky Save/Cancel footer with errors and an explicit note that Save
applies to both tabs. The footer pattern is shared with Translation Model.

`bun run check` passed; all 82 frontend tests passed. New coverage checks cover
reset and tab persistence, failed-save retention, retrying both tabs together,
and focus restoration. Browser fixtures verified Myanmar in light/dark at
1280×720 and within 960×640 component bounds, custom-cover/reset display,
keyboard tabs, metadata and Myanmar instruction edits surviving switches and
save failure, plus visible loading/error/disabled actions. Synthetic fixtures
were removed. Native file chooser, Rust/provider integration and Windows/Linux
rendering were not rerun. Next unfinished task: native verification of the
compact global and Book Settings dialogs with remaining localization coverage.
The existing bundle-size warning remains.

### Compact AI translation setup — 2026-10-16

Whole-chapter and selected-paragraph translation now share compact mode controls
and a single provider/model/language summary. Optional instructions guidance
is collapsed; chapter sharing consent remains explicit beside Generate. The
paragraph workflow uses small step controls, a combined batch/count toolbar,
and shorter source rows with icon disclosures for formatted previews. Blocked
items retain visible reasons and disabled checkboxes. Existing job, retry,
review and append behavior is unchanged; source markup is not reprocessed. New
labels are translated in all five non-English catalogs.

`bun run check` passed and the 26 targeted AI selection/progress tests passed.
Browser fixtures checked Myanmar at 1280×720 in light mode and within 960×640
component bounds in dark mode, visible keyboard focus, consent enabling,
disabled source items and preview expansion independent of selection. Synthetic
fixtures were removed. Native provider calls, native window sizing and
Windows/Linux rendering remain unverified. Next unfinished task: verify the
compact dialogs in rebuilt native Sagar and finish remaining localization
coverage. The existing bundle-size warning remains.

### Compact translation languages — 2026-10-16

The workspace language dialog now has equal-width named language choices, an
Unknown source option, and an Other language option that retains custom and
regional tags. The repeated direction line is removed, detailed guidance is
collapsed, and Save/Cancel share the existing settings footer. Fields disable
while saving; selects and disclosures participate in the focus trap, and Enter
submits. The existing metadata service and validation boundaries are preserved.
New display labels are translated in all five non-English catalogs.

`bun run check` passed, and four language-dialog/localization tests passed.
Regression coverage checks regional/custom tags, unknown source, save failure
retention, retry and focus restoration. Browser fixtures checked Myanmar in
light at 1280×720 and dark within 960×640 component bounds, custom tag entry,
invalid-tag feedback, save errors and keyboard focus wrapping. Fixtures were
removed. Native saving and Windows/Linux rendering were not rerun. Next
unfinished task: verify the compact language dialog in rebuilt native Sagar.
No milestone checkboxes advanced; the existing bundle-size warning remains.

### App-language default for new EPUB targets — 2026-10-16

Library import passes the current app language through the typed Tauri bridge.
The Rust import service maps English/Myanmar to Myanmar and Chinese, Japanese,
Korean and Thai to their respective target tags. Missing/unknown preferences
retain the Myanmar default. The target is inserted in the same transaction as
the new project; existing projects and duplicate/resume targets are unchanged.
No migration or source-content conversion is needed.

Validation: lint/TypeScript/build and four frontend import/localization tests
passed. Rust formatting and the full Rust test suite passed, including the import
integration suite (21 tests, two existing ignored). The new synthetic regression covers all supported languages,
unknown preference fallback, persisted targets, existing-project preservation
and unchanged source/snapshot bytes. Native picker/import interaction and
Windows/Linux were not exercised. Next unfinished task: verify changing app
language then importing in rebuilt native Sagar. No milestone checkboxes advanced.
The existing bundle-size warning remains.

### Myanmar ethnic language choices — 2026-10-16

Expanded the shared language names with all 19 requested languages and Myanmar
labels where supplied. Translation-language choices include both primary codes
and explicit Myanmar-script Myanmar-region locales (`rki-Mymr-MM`, etc.).
Existing saved tags remain unchanged; display lookup recognizes full locales
through their primary language. This continues language-dialog compatibility
polish; no milestone checkboxes advanced.

Validation: `bun run check` passed (existing bundle-size warning). The language
dialog regression passed with 88 assertions covering all primary/locale choices
in both fields, a Myanmar label, exact locale submission, save-error retention,
retry and focus restoration. Rust code and EPUB processing were unchanged;
Rust tests were not rerun. Native rendering, theme/minimum-window checks and
Windows/Linux remain unverified. Next unfinished task: verify the expanded
native language picker and Myanmar glyph rendering in rebuilt Sagar.

### Shan and Arakanese app language choices — 2026-10-16

Added Myanmar (Shan), `shn`, and Myanmar (Arakanese), `rki`, to Settings →
Appearance. Both preferences apply immediately, restore after restart, set the
interface language tag, and initialize the matching translation target on new
imports. Existing projects retain their saved languages. Added separate core
interface catalogs with English fallback; these are partial translations and
require native-speaker review. Continues the existing localization compatibility
work; no milestone checkboxes advanced.

Validation: `bun run check` passed with the existing bundle-size warning; five
frontend tests passed (57 assertions), including selection, focus retention,
remount persistence and import command forwarding. `cargo fmt --check` and the
focused import persistence regression passed. Native rendered layout, light/dark
appearance, glyph shaping, minimum window size and Windows/Linux behavior were
not visually verified. Next unfinished task: native-speaker review and completion
of both catalogs, followed by native UI/glyph verification.

### Remove duplicate language choices — 2026-10-16

The picker now offers one full Myanmar-script locale per requested language.
When a project already stores a short code, that field substitutes its saved
code for the equivalent locale choice, preserving metadata without duplicates.
No milestone checkboxes advanced. `bun run check` and the language-dialog
regression passed (70 assertions), including absence of duplicate short codes,
preservation of saved Shan, and locale save/retry. Existing bundle-size warning
remains. Native rendering and cross-platform checks were not run. Next unfinished
task: verify the corrected picker in rebuilt native Sagar.

### Searchable translation language picker — 2026-10-16

Replaced the workspace language selects with searchable comboboxes using existing
picker styles. Search matches English/Myanmar names and locale codes, with
Unicode normalization. Arrow keys and Enter select; Escape dismisses the list
before the dialog; blur restores the saved selection without changing metadata.
Unknown source, custom tags, saved short-code deduplication and save/retry remain.
Search and empty-state labels are localized in the five existing base catalogs.

Validation: `bun run check` and the language-dialog regression passed (57
assertions), covering all requested locale choices, English/Myanmar/code search,
keyboard selection/Escape, persisted short codes, save errors and focus restoration.
Existing bundle-size warning remains. No running Sagar build was available in
the UI inventory; native visual, light/dark, minimum-size, IME and cross-platform
checks remain unverified. Rust and EPUB processing were unchanged and Rust tests
were not rerun. Next unfinished task: inspect and exercise the searchable picker
in rebuilt native Sagar. No milestone checkboxes advanced.

### Mon app language choice — 2026-10-16

Added Myanmar (Mon), `mnw`, under Settings → Appearance, with a separate partial
core interface catalog and English fallback for untranslated messages. Selection
applies immediately and restores on remount; new imports default their target to
Mon. Existing book language choices already include Mon and remain unchanged.
Continues localization compatibility work; no milestone checkboxes advanced.

Validation: five frontend tests passed (66 assertions), including Mon selection,
focus retention, remount persistence, fallback and import forwarding. Build,
changed-source Biome checks, Rust formatting and the focused import persistence
regression passed. Full `bun run check` stops on existing formatting differences
in the Shan/Arakanese catalogs; build separately passed with the existing bundle
size warning. Native UI, light/dark, minimum-size layout, Mon glyph rendering and
Windows/Linux behavior were not verified. Next unfinished task: native-speaker
review and completion of the Mon catalog, then native visual verification.

### System display fonts — 2026-10-16

Added Settings → Appearance selectors for UI and translated book fonts, populated
from local system font discovery through a Rust service (`fontdb`). Each has an
English/Myanmar preview, immediate application, local persistence and a Default
font option. Loading, discovery failure/retry, empty discovery and missing saved
fonts retain recoverable preferences. Per the confirmed scope, original EPUB
rendering and CSS remain unchanged. These are display preferences; exported EPUB
styles and editor content are unchanged. No milestone checkboxes advanced.

Validation: `bun run lint`, `bun run build` and five focused frontend tests
(71 assertions) passed. Build retains the existing bundle-size warning. Native
layout, keyboard interaction, light/dark previews, minimum size, Myanmar shaping
and Windows/Linux discovery remain unverified; no running Sagar window was
available. Next unfinished task: verify the font pickers in rebuilt native Sagar.
Rust formatting passed; Rust library tests passed (180 passed, 2 ignored),
including a local font discovery smoke test. Font discovery uses the cross-platform
system-directory scanning supported by fontdb; OS-disabled or nonstandard font
installations may not appear in the list.
