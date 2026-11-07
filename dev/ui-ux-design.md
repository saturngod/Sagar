# Desktop UI/UX design — EPUB Translation Studio

**Status:** Design specification — not implemented or visually verified  
**Scope:** Manual translation MVP on Windows, macOS and Linux  
**Related:** [Master plan](plan.md), [repository rules](../AGENTS.md), [how to build UI](../Design.md)

## 1. Design direction

Build a calm, professional editorial workspace for long reading and translation sessions. The book and translation occupy most of the window; navigation and controls stay compact and predictable. Use neutral surfaces, clear typography, subtle borders and one restrained accent color. Reserve stronger colors for meaningful warnings and failures.

Use consistent desktop patterns: persistent toolbars, selectable lists, native file dialogs, contextual menus, keyboard shortcuts and adjustable panes. Avoid decorative hero sections, oversized cards, gradients, glass effects and animations that distract from editing. Use existing CSS with shared design tokens; choose a component library only if implementation demonstrates a concrete reduction in complexity.

The interface must distinguish source content, editable translation, review progress and storage status. Every important state uses text as well as an icon or color. All features described here are planned requirements; defer optional enhancements until their roadmap milestone.

## 2. Visual system

These are initial implementation targets. Validate readability and contrast in the actual WebViews before treating them as final.

| Element | Design target |
| --- | --- |
| Spacing | 4, 8, 12, 16, 24 and 32 px scale; 16–24 px pane padding |
| App typography | Platform system UI font; 14 px base, 12–13 px secondary labels, 18–20 px screen headings |
| Reading/editor typography | 18 px default; user-adjustable 14–28 px; generous line height, initially 1.75 |
| Text width | Approximately 60–75 Latin characters per line; validate Myanmar visually rather than applying a character limit |
| Controls | 32–36 px tall; consistent labels, padding and disabled states |
| List rows | At least 40 px; enlarge for long titles and additional status text |
| Corners | 6–8 px for controls and dialogs; simple rectangular reading panes |
| Borders/shadows | Subtle 1 px separators; shadows mainly for floating menus and dialogs |
| Accent | Muted blue for primary actions, selection and focus; separate success, warning and error tokens |
| Contrast | Verify at least 4.5:1 for normal UI text and 3:1 for large text and meaningful control boundaries |
| Focus | Clearly visible focus ring in both themes; never globally remove outlines |

Define semantic CSS variables for background, surface, text, muted text, border, accent, focus, success, warning and error. Support system/light/dark appearance during release polish. Book CSS stays contained within the source preview; it must not style app controls. Theme adaptation must preserve readable source images and text; do not blindly invert an entire book.

Use platform system fonts for chrome and a tested Myanmar-capable fallback stack for book/editor content. If bundling a font is needed, verify its redistribution license and offline availability. Check combining marks, tall glyphs, cursor movement, selection and copy/paste using real Myanmar samples. Avoid fixed-height text containers that clip marks, forced uppercase, excessive letter spacing and truncation within editable content.

## 3. Window and layout

Use normal platform window controls and resize behavior. Prefer the existing Tauri window decoration unless a concrete usability need justifies custom chrome. Keep interactive controls out of draggable regions.

Target an initial 1280 × 800 window, with a usable 960 × 640 minimum subject to WebView and font checks. At a smaller usable width, collapse the chapter sidebar with an explicit toggle and preserve the two reading/editor panes. Do not silently switch to a mobile interface. At larger text zoom, allow scrolling and reachable controls instead of clipping content.

```text
┌─────────────────────────────────────────────────────────────────────────┐
│ Library   Book title       Saved       Text size    Preview   Export EPUB│
├─────────────────┬───────────────────────────┬───────────────────────────┤
│ Chapters        │ Original                  │ Translation · Myanmar     │
│                 │ Chapter title             │ Undo / Redo · Formatting  │
│ 01 Introduction │                           │                           │
│    Completed    │ Source heading            │ Target heading            │
│ 02 Chapter two  │ Source paragraph          │ Editable target block     │
│    In progress  │                           │ Draft · Mark reviewed     │
│ 03 Chapter three│ Source image / caption    │ Preserved image / caption │
│    Not started  │                           │                           │
├─────────────────┴───────────────────────────┴───────────────────────────┤
│ Chapter 2 of 20       8 / 12 blocks reviewed       Last saved 12:30 PM    │
└─────────────────────────────────────────────────────────────────────────┘
```

Start with a 240 px chapter sidebar, adjustable approximately 200–360 px. Divide remaining space equally between source and translation, with approximately 320 px minimum widths where possible. Splitter handles must be discoverable, keyboard operable and labeled for assistive technology. Persist pane sizes as presentation preferences separately from translation content.

Keep the toolbar, pane headings and bottom status bar stable while content scrolls. Source and translation scroll independently for the MVP; synchronized scrolling is deferred. Keep active-block highlighting subtle and do not unexpectedly scroll the editor while the user types.

## 4. Project Library and import

Use a clear heading, a primary **Import EPUB** action and a searchable, compact book list. A row shows a small cover thumbnail or neutral placeholder, title, author, language direction, reviewed progress and last edited time. Titles take priority over metadata; show the full title in an accessible detail view when truncation is necessary. Use a compact reviewed/total chapter count and progress bar, with the complete metric in its accessible label and tooltip.

For an empty library, show a book icon, “Add your first book” and a single short import hint. Keep one Import EPUB action. Avoid account setup or online onboarding.

After native file selection, show import stages such as “Checking book,” “Reading chapters” and “Creating project.” Use indeterminate progress unless measurable totals are available. Offer cancellation only when the service supports safe cancellation. Cancelling the picker returns to the previous screen without an error notification.

On success, open the imported project and show a concise warning summary if needed. Explain unsupported content in ordinary language and provide chapter/block locations. For errors, identify the problem and a useful next action without dumping internal logs. Detect duplicate content and offer to resume the existing project or create a separate project through the service’s supported workflow.

## 5. Chapter navigation and source reader

List chapters in OPF spine order. Each row includes the chapter title, a textual status and a compact reviewed count. Distinguish selection from keyboard focus. Long titles may wrap to two lines. Show unsupported-content counts separately from review completion. TOC anchors may navigate within a resource; never imply every TOC entry is a separate chapter file.

Selecting a chapter flushes pending saves before leaving the current one. Keep the current chapter visible if saving fails, with Retry and an explanation. During loading, retain the workspace frame and show loading content; avoid presenting the previous chapter under the new chapter title.

The original pane is labeled **Original · read-only**. Preserve headings, paragraphs, images, captions and supported links with comfortable margins. Internal links and footnotes navigate safely and provide a return route. External URLs never fetch automatically; if opening is supported, require an explicit user action through a controlled application route. Unsupported resources use a localized placeholder and explanation rather than disappearing.

## 6. Translation editor and review

Render editable blocks in reading order with source-block identity managed internally. Display matching source and target blocks clearly without exposing IDs, XML locators or implementation terminology to the user. A selected block highlights its source counterpart without interrupting selection or typing.

Keep the editor toolbar small: Undo, Redo and only the formatting supported by the validated schema. Bold, italic and links appear only after their preservation behavior is verified. Disable unavailable operations with an accessible explanation. Do not offer arbitrary HTML, unsupported tables or formatting that export cannot preserve.

An empty block shows a subdued “Enter translation…” hint and accessible label. Provide **Copy original** as a deliberate action; copying or typing creates a draft and never automatically marks it reviewed. If copying would replace existing translation, request confirmation. Keep block actions near the active block instead of repeating a full toolbar for every paragraph.

| Block state | Presentation and behavior |
| --- | --- |
| Pending | “Not translated”; empty target or unreviewed required content |
| Draft | “Draft”; saved content requiring user review |
| Reviewed | “Reviewed”; explicit user confirmation |
| Skipped | “Keep original”; explicit decision with original-content fallback explained |
| Unsupported | “Preserved original · unsupported”; separate warning and review policy |

Editing reviewed content returns it to Draft and clears its review confirmation through the translation service. **Mark reviewed** validates the current content and acknowledges the save before showing reviewed status. Empty required blocks cannot be reviewed as complete; use **Keep original** when intentional. Undo/redo applies to editing; status transitions follow service validation and revision rules.

Composition with an input method must finish before serializing a new draft. Preserve selections during normal autosave and avoid replacing the entire editor document after each acknowledgement. Test paste with multiline text, Unicode, links and unsupported markup; any material formatting loss needs a clear explanation. Deferred block merge/split and glossary features must not appear as working controls.

## 7. Save, recovery and feedback

Separate storage status from review status. Show one persistent workspace save indicator and block-level review labels.

| Save state | User feedback |
| --- | --- |
| Pending edits | “Unsaved changes” immediately after editing |
| Write in progress | “Saving…” while awaiting acknowledgement |
| Durable write acknowledged | “Saved” plus optional last-saved time |
| Write failed | “Couldn’t save · Retry”; persistent actionable message |
| Revision conflict | Explain that a newer saved version exists; preserve local draft and offer a deliberate resolution |

Do not show a toast for every autosave. Use brief, non-disruptive messages for routine actions and persistent inline feedback for failures that affect durability. Announce important changes accessibly without reading every keystroke or repeatedly interrupting the user.

Before navigation, export or normal close, flush and await pending writes. Do not silently discard local drafts; keep them available for retry during the session. If a normal close requires a user decision after repeated failure, offer Retry, Cancel close and an explicitly labeled discard action. Forced shutdown recovery can promise only the latest committed content until a durable recovery mechanism exists.

## 8. Export and output preview

Use an export review dialog with a clear title, translated-title field, target-language field and summary of reviewed, draft, pending, skipped and unsupported blocks. Language fields show a human-readable name and a validated BCP 47 value. Clicking an issue navigates to its chapter/block after preserving edits.

Present **Export completed EPUB** when completion requirements are satisfied. Otherwise explain remaining issues and offer **Export draft…** with explicit acknowledgement that untranslated or unsupported content stays in the original language. Intentional skips and unsupported-content policy remain visible even when an export is allowed.

After review, use the native Save dialog. Check destination safety in Rust and confirm replacement of unrelated existing outputs. Show packaging and validation stages without a fake percentage. Prevent duplicate submissions. On success, display the output filename, validation summary and warnings; offer a supported action to show the output in its folder or open it. Never claim full EPUB conformance based only on internal checks.

Preview must show saved translations and explain retained original blocks. It uses the same validated content mapping as export, while remaining isolated from privileged APIs. A preview is not evidence that an archive has passed validation. Do not show an enabled Preview action until this behavior exists.

## 9. Desktop interaction and accessibility

Use Cmd on macOS and Ctrl on Windows/Linux for app shortcuts. Display platform-appropriate shortcuts in menus and tooltips. Respect editor and operating-system shortcuts; do not intercept them globally when unrelated controls have focus.

| Shortcut | Planned action |
| --- | --- |
| Cmd/Ctrl + O | Open EPUB through native picker |
| Cmd/Ctrl + S | Flush pending translation saves; show actual result |
| Cmd/Ctrl + Z | Undo in the focused editor |
| Platform redo shortcut | Redo in the focused editor using editor/platform conventions |
| Escape | Close the current menu/dialog and return focus to its trigger |
| Tab / Shift + Tab | Predictable movement through controls; no keyboard trap |

Provide accessible names for icon buttons, editor blocks and splitters. Dialogs trap focus while open and restore it when dismissed. Default initial focus to a safe, relevant control; never to a destructive confirmation. Native chooser cancellation preserves the previous focus where feasible. Chapter-list keyboard navigation must follow the selected accessible list pattern and work without the mouse.

Provide descriptive tooltips for compact controls, visible hover/pressed/disabled states, sufficient click targets and inline field errors. Respect reduced motion. Support text zoom without hiding export, save state or chapter controls. Validate with keyboard and available platform screen readers; document operating systems not yet checked.

## 10. Delivery by milestone

| Milestone | Required UI/UX slice |
| --- | --- |
| 0 — Foundation | Shared visual tokens, basic library/workspace frame, accessible controls and reusable error feedback; mock content clearly labeled |
| 1 — Import | Empty/loading/error library states, native picker cancellation, recent projects, chapter order and import warnings |
| 2 — Workspace | Resizable panes, safe source preview, restricted block editor, typography, focus and Myanmar composition behavior |
| 3 — Durability | Honest save states, review labels, progress counts, save-aware navigation and restored chapter |
| 4 — Export | Metadata fields, actionable review summary, explicit draft decision, native Save dialog and validation feedback |
| 5 — Release | System/light/dark appearance, text zoom, shortcut polish, layout persistence and cross-OS accessibility/visual checks |

## 11. Design acceptance checks

These additional UI checks supplement the master plan’s acceptance criteria; all remain unverified.

- [ ] Library and workspace feel consistent in spacing, typography, controls and error wording.
- [ ] The active chapter, editable target, review status and save state are identifiable without relying on color.
- [ ] At 960 × 640, 1280 × 800 and a larger window, core actions remain reachable; also check high-DPI scaling and increased text size.
- [ ] Pane resizing works with pointer and keyboard and cannot hide required content or controls.
- [ ] A long chapter title, missing cover, many chapters and a long paragraph do not break the layout.
- [ ] Myanmar marks are not clipped; composition, selection, copy/paste, wrapping and undo preserve text.
- [ ] Import cancellation, loading, malformed input and unsupported content have distinct useful states.
- [ ] Save failure and revision conflict preserve local edits and prevent misleading Saved/completed indicators.
- [ ] Export review distinguishes drafts, required missing blocks, intentional skips and unsupported content.
- [ ] Keyboard-only use covers import, chapter selection, editing, review, export and dialog cancellation.
- [ ] Focus, accessible names, contrast and available screen readers are checked in both themes on supported OSes where possible.
- [ ] Screenshots and manual findings record the actual OS, window size and limitations; no untested platform is marked verified.

### Visual polish revision — 2026-10-13

The home page shows books and import/search actions without system diagnostics,
version/schema information or a workflow tutorial. Common actions use one local
SVG icon set: trash, appearance, sidebar, previous/next, review, keep-original,
formatting, images and export. Icon-only buttons have accessible labels,
tooltips and keyboard focus. Major actions retain short text beside their icon.
Library metadata is reduced to author/language, chapter progress and edited date.
Source content no longer has a redundant uppercase caption.
