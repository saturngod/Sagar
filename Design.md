# Design.md — How to build Sagar's UI

This is the practical guide for building and changing screens in Sagar. It
covers the design tokens, the shared components and CSS patterns, and the checks
to run before a UI change is called done.

- **What each screen does** (workflow, states, milestones) is in
  [dev/ui-ux-design.md](dev/ui-ux-design.md).
- **Mandatory rules** for agents and contributors are in
  [AGENTS.md](AGENTS.md) → "Professional UI/UX and consistency". If this file
  and AGENTS.md disagree, AGENTS.md wins; fix this file.

## 1. Principles

1. **Reading and writing come first.** The book and the translation take most
   of the window. Controls stay compact, predictable and out of the way.
2. **Calm and editorial.** Neutral surfaces, subtle 1 px borders, one muted
   blue accent. Stronger color only for success, warning and error.
   No gradients, glass effects, hero sections or decorative animation.
3. **Never color alone.** Every state has text (and usually an icon) as well as
   color: "Saved", "Draft", "Failed", "Not found".
4. **Show what is hidden on demand.** Lists show one line per item. Details,
   secondary actions and help open from a disclosure or a menu. Formatting and
   table tools appear only when relevant.
5. **Reuse before adding.** Extend an existing shared pattern instead of adding
   a one-off style for a single screen.
6. **Myanmar text is a first-class case.** Never clip combining marks, force
   uppercase, add letter spacing, or use fixed-height text containers.

## 2. Design tokens

All values come from [src/styles/tokens.css](src/styles/tokens.css). Do not
hard-code colors, spacing, radii or font stacks in components or CSS. If a value
you need is missing, add a token (with light **and** dark values) instead of a
literal.

| Group | Tokens | Use |
| --- | --- | --- |
| Spacing | `--space-1` 4 · `--space-2` 8 · `--space-3` 12 · `--space-4` 16 · `--space-6` 24 · `--space-8` 32 | Gaps, padding, margins. Pane padding 16–24 px. |
| UI text | `--text-xs` 12 · `--text-sm` 13 · `--text-base` 14 · `--text-md` 16 · `--text-lg` 18 · `--text-xl` 20 | 14 px base; 12–13 px secondary labels; 18–20 px headings. |
| Fonts | `--font-ui`, `--font-book`, `--leading-book` (1.75) | `--font-ui` for app chrome; `--font-book` for source and translation content. Both include Myanmar fallbacks. |
| Surfaces | `--color-bg`, `--color-surface`, `--color-surface-hover`, `--color-surface-selected` | Page background, cards/dialogs, hover, selected rows. |
| Text | `--color-text`, `--color-text-muted` | Muted text must still meet 4.5:1 contrast. |
| Lines | `--color-border`, `--border-subtle` | Use `border: var(--border-subtle)` for separators and cards. |
| Accent | `--color-accent`, `--color-accent-hover`, `--color-accent-text`, `--color-focus` | Primary action, selection, focus ring. |
| Status | `--color-success`/`-bg`/`-border`, `--color-warning…`, `--color-error…`, `--color-info…` | Status text, notices, badges. |
| Destructive | `--color-cancel`, `--color-cancel-hover`, `--color-cancel-text` | Stopping a running job (`.btn--cancel`). |
| Shape | `--radius-control` 6 px, `--radius-dialog` 8 px, `--control-height` 32 px | Controls use 6 px; dialogs, cards and grouped lists use 8 px. |
| Depth | `--shadow-overlay` | Floating menus and dialogs only. |

### Themes

Theme is set by `data-theme="light" | "dark"` on the root, or follows the OS
when neither is set ("System"). `tokens.css` defines the dark values twice: once
under `[data-theme="dark"]` and once under `prefers-color-scheme: dark` for
System. **When you add or change a dark token, update both blocks.**

Book content keeps its own styling inside the source preview. Book CSS must
never style app controls, and theme changes must never invert book images.

## 3. Layout

- **Window:** default 1280 × 800, minimum **960 × 640** (set in
  `src-tauri/tauri.conf.json`). Every screen must work at the minimum size.
- **Workspace:** two persistent control rows — the book/chapter header, then
  the pane headers. Do not stack extra toolbars above the panes.
- **Panes:** source and translation share the space; the chapter sidebar
  collapses with an explicit toggle when space is tight. Use the shared
  `Splitter` component for resizable panes; it is keyboard operable and
  labeled.
- **Dialogs:** `.dialog-backdrop` > `.dialog` (`.dialog--narrow` for small
  forms). Header is `.panel__header` with an `.panel__heading` and an X
  icon-button. Footer actions use `.button-row.dialog__actions`
  (right-aligned, Cancel then primary).
- **Breakpoints** already in use: 500, 600, 760, 850, 1000, 1100 and 1600 px
  wide, plus `max-height: 700px`. Reuse one of these rather than inventing a new
  value. Inside dialogs, 600 px is the usual "narrow" switch.
- Large text sizes must scroll, never clip or overlap controls.

## 4. Shared components

| Component | File | Use it for |
| --- | --- | --- |
| `Icon` | [src/components/Icon.tsx](src/components/Icon.tsx) | Every icon. 24 px viewBox, stroke-based, `aria-hidden`. Default size 18; 16 for inline/list icons. Add new icons to `paths` there; do not import another icon set. |
| `StatusNotice` | [src/components/StatusNotice.tsx](src/components/StatusNotice.tsx) | Loading, success, error and info feedback. Errors get `role="alert"` and an optional Retry. |
| `Splitter` | [src/components/Splitter.tsx](src/components/Splitter.tsx) | Resizable panes. |
| `HtmlCode` | [src/components/HtmlCode.tsx](src/components/HtmlCode.tsx) | Read-only highlighted HTML. |

Keep components small. Business logic belongs in hooks and Rust services, not in
JSX (see AGENTS.md §7).

## 5. Controls

### Buttons

| Pattern | Class | When |
| --- | --- | --- |
| Primary | `btn btn--primary` | The one main action of a dialog or area (Save, Export). |
| Secondary | `btn` | Cancel, Edit, other text actions. |
| Small | `btn btn--small` | Actions inside lists, rows and section headers. |
| Danger | `btn btn--danger` | Confirming a delete. Always behind a confirm step. |
| Stop a job | `btn btn--cancel` | Cancelling a running AI request. |
| Icon only | `icon-button` | Compact toolbar and row actions. 34 × 34 px with 18 px icon. |

Rules:

- **Every text button has an icon** from `Icon`, placed before the label.
  Text-only buttons are not allowed.
- **Every icon-only button** has an action-specific `aria-label` and `title`,
  including the item name when it acts on one item: `Test DeepSeek`, not `Test`.
- Toggles use `aria-pressed`; disclosures use `aria-expanded` +
  `aria-controls`. The shared CSS already styles both as active.
- Labels: "Save" / "Saving…", "Cancel", "Delete" → "Confirm delete" /
  "Deleting…". Use `…` (one character), not `...`.
- Disabled controls stay readable; never combine a faded background with
  low-contrast text.
- Use an X icon-button (`Icon name="close"`, label "Close") to dismiss a dialog.
  If closing cancels work, say so in the label or tooltip.

```tsx
<button type="button" className="btn btn--primary" disabled={saving} onClick={save}>
  <Icon name="save" />
  {saving ? t("Saving…") : t("Save")}
</button>

<button
  type="button"
  className="icon-button"
  aria-label={`${t("Edit")} ${provider.name}`}
  title={`${t("Edit")} ${provider.name}`}
  onClick={() => openEdit(provider)}
>
  <Icon name="edit" />
</button>
```

### Tabs

`.tabs` with `role="tablist"`; each `.tab` has `role="tab"`, an icon and a
label, `aria-selected`, `aria-controls`, roving `tabIndex`, and Left/Right
arrow navigation. Panels use `role="tabpanel"` and `hidden`. See the Settings
dialog for the reference implementation.

### Forms

- Wrap each field in `<label className="form-field">` with a `<span>` label and
  the control. Controls are `--control-height` tall.
- Short hints go in `.form-field__hint` directly under the control (link with
  `aria-describedby`). Do not hide essential guidance in a disclosure.
- Errors go in `.form-field__error` under the field, and the control gets
  `aria-invalid`. Keep the user's values when a save fails.
- Order fields the way the user fills them in. Put pairs of short fields
  side by side; stack them below 600 px.
- Placeholders are examples only; never put instructions or required meaning
  in a placeholder.
- Search inputs reuse `.library-search` (search icon + borderless input).

## 6. Lists and long collections

Design every list for many items, not the three you have while testing.

- **One line per item.** Name first, short metadata after it in muted text,
  status, then at most one or two row actions. Truncate with ellipsis and keep
  the full text available (title, details or accessible name).
- **Details on demand.** Put secondary fields and less frequent actions
  (Edit, Delete) in an expandable row (`aria-expanded` on the row toggle).
- **Group in a bordered list** (`border: var(--border-subtle)`, 8 px radius,
  separators between rows) rather than loose rows across the page.
- **Show a count** next to the heading, and add a search field once a list
  passes a handful of items (the provider list shows search above 4).
- **Small fixed sets** (for example detected tools) can be a tile grid
  (`repeat(auto-fill, minmax(180px, 1fr))`).
- **Open inline forms where the user clicked**: an "Add" form opens under the
  section header; an "Edit" form opens inside that item's row. Never append a
  form at the bottom of a long list.
- **Empty and no-match states** are separate messages ("No custom providers
  yet." vs "No providers match your search.").

The Providers tab in
[AiSettingsDialog.tsx](src/features/settings/AiSettingsDialog.tsx) is the
reference implementation of these rules.

## 7. Status and feedback

- Status text uses a small colored dot plus a word (`.provider-status`) or the
  `.chapter-export-status--*` classes. Success uses `--color-success`; neutral
  states use `--color-text-muted`.
- Use `StatusNotice` for loading, errors and results that need a message.
  Errors say what happened and what to do next, keep the user's input, and
  offer Retry when retrying can work.
- One persistent save indicator in the workspace: "Unsaved changes" →
  "Saving…" → "Saved", or "Couldn't save · Retry". No toast per autosave.
- Avoid implementation jargon in messages (no IDs, XML paths, error codes as
  the main text).

## 8. Editor rules

- Empty-editor hints sit outside the text flow. The caret starts at the
  beginning, the hint stays on focus and disappears on typing, and hint text is
  never saved.
- External updates (for example an accepted AI result) appear immediately in
  the mounted Rich editor and go through the same validation as typing.
- Never reload editor content during normal typing or IME composition. Keep
  caret, selection, undo history and Myanmar combining marks intact; ignore
  stale async results.

## 9. Accessibility and keyboard

- Global `:focus-visible` ring (2 px `--color-focus`). Never remove outlines.
- Dialogs: focus moves into the dialog on open (to the first useful field, never
  a destructive button), Tab is trapped, Escape closes, and focus returns to the
  trigger on close.
- Every interactive element is reachable by keyboard in a logical order.
- Text contrast at least 4.5:1 in both themes, including disabled and muted
  text; 3:1 for meaningful control borders.
- Respect `prefers-reduced-motion` (handled globally in `app.css`) and
  `forced-colors`.
- Use Cmd on macOS and Ctrl on Windows/Linux in shortcuts and tooltips.

## 10. Language and copy

- Wrap every user-facing string in `t("…")` from `useTranslation()`. The English
  string is the key; missing translations fall back to English.
- When you add a string, add it to the catalogs in `src/lib/i18n/`
  (my, rki, zh, ja, ko, th, shn, mnw) when you can translate it reliably.
  Leave it to fall back otherwise and mention it in your summary.
- Use "Sagar" for the app, "Book Settings" for per-book preferences, and
  chapter-based wording for progress.
- Sentence case for labels and headings ("Add provider", not "Add Provider").
- Keep messages short and concrete.

## 11. CSS conventions

- All app styles live in [src/styles/app.css](src/styles/app.css); tokens in
  `tokens.css`. No CSS-in-JS, no new styling framework.
- BEM-style names: `.block`, `.block__element`, `.block--modifier`
  (`.provider-row__toggle`, `.btn--small`). Prefix new screen styles with the
  feature name and keep them together under a short comment.
- Keep selector specificity low and avoid `!important`. Biome flags
  "descending specificity" — fix it by restructuring (for example add a class
  to a wrapper) rather than suppressing the warning.
- Use logical properties where direction matters (`margin-inline-end`,
  `text-align: start`).
- Inline `style` only for values computed at runtime (pane widths, user text
  size).

## 12. Checklist before calling a UI change done

1. `bun run lint` and `bun run build` pass (Biome also formats CSS and TSX).
2. Every new button has an icon; icon-only buttons have `aria-label` + `title`.
3. Loading, empty, error, disabled and selected states all exist and are
   readable.
4. Light and dark (and System) themes checked.
5. Checked at 960 × 640 and at a normal window size, with a large text size,
   long names and many items.
6. Keyboard only: Tab order, Escape, arrow keys where relevant, focus return.
7. Myanmar text renders without clipped marks in any changed text area or
   editor.
8. Report what you checked visually separately from lint/build. If you only
   checked a static render or could not run `bun tauri dev`, say so.
