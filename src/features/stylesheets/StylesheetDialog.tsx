import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  getStylesheet,
  listStylesheets,
  resetStylesheet,
  saveStylesheet,
} from "../../lib/tauri/commands";
import { CommandError } from "../../lib/tauri/errors";
import type {
  ProjectSummary,
  StylesheetContent,
  StylesheetSummary,
} from "../../types/project";
import { CssCodeEditor, type CssCodeEditorHandle, findMatches } from "./CssCodeEditor";

type ListState =
  | { status: "loading" }
  | { status: "ready"; sheets: StylesheetSummary[] }
  | { status: "error"; message: string };

type FileState =
  | { status: "idle" }
  | { status: "loading"; href: string }
  | { status: "ready"; content: StylesheetContent }
  | { status: "error"; href: string; message: string };

/** An action held back until the user decides what to do with unsaved edits. */
type Pending = { kind: "switch"; href: string } | { kind: "close" } | { kind: "reset" };

function message(error: unknown, fallback: string): string {
  return error instanceof CommandError || error instanceof Error
    ? error.message
    : fallback;
}

function formatSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

export function StylesheetDialog({
  project,
  onClose,
}: {
  project: ProjectSummary;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDivElement>(null);
  const editor = useRef<CssCodeEditorHandle>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const [list, setList] = useState<ListState>({ status: "loading" });
  const [listRetry, setListRetry] = useState(0);
  const [file, setFile] = useState<FileState>({ status: "idle" });
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState(0);
  const requestId = useRef(0);

  const loaded = file.status === "ready" ? file.content : null;
  const dirty = loaded !== null && draft !== loaded.css;
  const busy = saving;
  const matches = useMemo(
    () => (searchOpen ? findMatches(draft, query) : []),
    [draft, query, searchOpen],
  );
  const currentMatch = matches.length === 0 ? -1 : Math.min(current, matches.length - 1);

  // biome-ignore lint/correctness/useExhaustiveDependencies: listRetry explicitly reloads after a failure.
  useEffect(() => {
    let disposed = false;
    setList({ status: "loading" });
    listStylesheets(project.id)
      .then((sheets) => {
        if (disposed) return;
        setList({ status: "ready", sheets });
        setFile((state) =>
          state.status === "idle" && sheets[0]
            ? { status: "loading", href: sheets[0].href }
            : state,
        );
      })
      .catch((error) => {
        if (!disposed)
          setList({
            status: "error",
            message: message(error, "The book's stylesheets could not be loaded."),
          });
      });
    return () => {
      disposed = true;
    };
  }, [project.id, listRetry]);

  // Load whichever file is requested; ignore stale responses.
  const loadingHref = file.status === "loading" ? file.href : null;
  useEffect(() => {
    if (loadingHref === null) return;
    const id = ++requestId.current;
    getStylesheet(project.id, loadingHref)
      .then((content) => {
        if (id !== requestId.current) return;
        setFile({ status: "ready", content });
        setDraft(content.css);
      })
      .catch((error) => {
        if (id !== requestId.current) return;
        setFile({
          status: "error",
          href: loadingHref,
          message: message(error, "This stylesheet could not be opened."),
        });
      });
  }, [project.id, loadingHref]);

  const updateSummary = useCallback((content: StylesheetContent) => {
    setList((state) =>
      state.status === "ready"
        ? {
            ...state,
            sheets: state.sheets.map((sheet) =>
              sheet.href === content.href
                ? {
                    ...sheet,
                    modified: content.modified,
                    updatedAt: content.updatedAt,
                    size: new TextEncoder().encode(content.css).length,
                  }
                : sheet,
            ),
          }
        : state,
    );
  }, []);

  const save = useCallback(async (): Promise<boolean> => {
    if (!loaded || saving) return false;
    setSaving(true);
    setSaveError(null);
    setSavedNotice(null);
    try {
      const content = await saveStylesheet(project.id, loaded.href, draft);
      setFile({ status: "ready", content });
      updateSummary(content);
      setSavedNotice(
        content.modified
          ? t("Saved. Your next translated export will use this stylesheet.")
          : t("Saved. This stylesheet now matches the original."),
      );
      return true;
    } catch (error) {
      setSaveError(message(error, "The stylesheet could not be saved."));
      return false;
    } finally {
      setSaving(false);
    }
  }, [loaded, saving, project.id, draft, updateSummary, t]);

  async function reset() {
    if (!loaded) return;
    setSaving(true);
    setSaveError(null);
    setSavedNotice(null);
    try {
      const content = await resetStylesheet(project.id, loaded.href);
      setFile({ status: "ready", content });
      setDraft(content.css);
      updateSummary(content);
      setSavedNotice(t("Restored the original stylesheet."));
    } catch (error) {
      setSaveError(message(error, "The original stylesheet could not be restored."));
    } finally {
      setSaving(false);
    }
  }

  function openFile(href: string) {
    setSavedNotice(null);
    setSaveError(null);
    setCurrent(0);
    setFile({ status: "loading", href });
  }

  function requestFile(href: string) {
    if (busy || (loaded?.href === href && file.status === "ready")) return;
    if (dirty) {
      setPending({ kind: "switch", href });
      return;
    }
    openFile(href);
  }

  const requestClose = useCallback(() => {
    if (busy) return;
    if (dirty) {
      setPending({ kind: "close" });
      return;
    }
    onClose();
  }, [busy, dirty, onClose]);

  function finishPending(action: Pending) {
    setPending(null);
    if (action.kind === "switch") openFile(action.href);
    else if (action.kind === "close") onClose();
    else void reset();
  }

  async function saveThenContinue() {
    const action = pending;
    if (!action) return;
    if (await save()) finishPending(action);
    else setPending(null);
  }

  // Bumped on every "open find" so a repeat Ctrl+F refocuses the field.
  const [searchFocusRequest, setSearchFocusRequest] = useState(0);
  const openSearch = useCallback(() => {
    setSearchOpen(true);
    setSearchFocusRequest((n) => n + 1);
  }, []);
  // Focus as soon as the field is committed, before the next keystroke.
  useLayoutEffect(() => {
    if (searchFocusRequest === 0) return;
    searchInput.current?.focus();
    searchInput.current?.select();
  }, [searchFocusRequest]);

  function step(delta: number) {
    if (matches.length === 0) return;
    setCurrent((index) => {
      const base = Math.min(index, matches.length - 1);
      return (base + delta + matches.length) % matches.length;
    });
  }

  function closeSearch() {
    const match = currentMatch >= 0 ? matches[currentMatch] : undefined;
    setSearchOpen(false);
    if (match) editor.current?.select(match);
    else editor.current?.focus();
  }
  // The window key handler below runs first (capture phase); keep it on the
  // latest match list without re-registering on every keystroke.
  const closeSearchRef = useRef(closeSearch);
  closeSearchRef.current = closeSearch;

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    return () => previous?.focus();
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        if (loaded) openSearch();
        return;
      }
      if (mod && event.key.toLowerCase() === "s") {
        event.preventDefault();
        event.stopPropagation();
        if (dirty) void save();
        return;
      }
      if (event.key === "Escape") {
        event.stopPropagation();
        if (event.target === searchInput.current) {
          event.preventDefault();
          closeSearchRef.current();
          return;
        }
        if (pending) setPending(null);
        else requestClose();
        return;
      }
      if (event.key !== "Tab") return;
      const controls = Array.from(
        dialog.current?.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled), textarea:not(:disabled)",
        ) ?? [],
      ).filter((control) => control.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === dialog.current)
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [loaded, dirty, pending, save, requestClose, openSearch]);

  const selectedHref =
    file.status === "ready"
      ? file.content.href
      : file.status === "idle"
        ? null
        : file.href;
  const selectedSheet =
    list.status === "ready"
      ? list.sheets.find((sheet) => sheet.href === selectedHref)
      : undefined;

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog stylesheet-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="stylesheet-dialog-title"
        tabIndex={-1}
        ref={dialog}
      >
        <div className="panel__header stylesheet-dialog__header">
          <div className="book-settings__heading">
            <h2 className="panel__heading" id="stylesheet-dialog-title">
              {t("Book CSS")}
            </h2>
            <p className="dialog__muted book-settings__book-title" title={project.title}>
              {project.title}
            </p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Close book CSS")}
            title={dirty ? t("Close (asks about unsaved changes)") : t("Close")}
            disabled={busy}
            onClick={requestClose}
          >
            <Icon name="close" />
          </button>
        </div>

        {list.status === "loading" ? (
          <StatusNotice kind="loading">{t("Loading stylesheets…")}</StatusNotice>
        ) : null}
        {list.status === "error" ? (
          <StatusNotice kind="error" onRetry={() => setListRetry((n) => n + 1)}>
            {list.message}
          </StatusNotice>
        ) : null}
        {list.status === "ready" && list.sheets.length === 0 ? (
          <div className="empty-state">
            <Icon name="palette" size={36} />
            <h3 className="empty-state__heading">{t("No stylesheets")}</h3>
            <p>{t("This book does not include any CSS files.")}</p>
          </div>
        ) : null}

        {list.status === "ready" && list.sheets.length > 0 ? (
          <div className="stylesheet-dialog__body">
            <nav className="stylesheet-list" aria-label={t("Stylesheets")}>
              <ul>
                {list.sheets.map((sheet) => (
                  <li key={sheet.href}>
                    <button
                      type="button"
                      className="stylesheet-list__item"
                      aria-current={sheet.href === selectedHref ? "true" : undefined}
                      title={sheet.href}
                      disabled={busy}
                      onClick={() => requestFile(sheet.href)}
                    >
                      <Icon name="palette" size={16} />
                      <span className="stylesheet-list__name">{sheet.name}</span>
                      <span className="stylesheet-list__meta">
                        {sheet.modified ? (
                          <span className="stylesheet-badge">{t("Edited")}</span>
                        ) : null}
                        {formatSize(sheet.size)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </nav>

            <section
              className="stylesheet-editor"
              aria-label={selectedSheet?.name ?? t("Stylesheet")}
            >
              <div className="stylesheet-editor__toolbar">
                <span className="stylesheet-editor__title" title={selectedSheet?.href}>
                  {selectedSheet?.name ?? ""}
                  {dirty ? (
                    <span className="stylesheet-editor__state">
                      {t("Unsaved changes")}
                    </span>
                  ) : loaded?.modified ? (
                    <span className="stylesheet-editor__state">{t("Edited")}</span>
                  ) : loaded ? (
                    <span className="stylesheet-editor__state">{t("Original")}</span>
                  ) : null}
                </span>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={t("Find in stylesheet")}
                  title={`${t("Find in stylesheet")} (⌘/Ctrl+F)`}
                  aria-expanded={searchOpen}
                  aria-controls="stylesheet-search"
                  aria-pressed={searchOpen}
                  disabled={!loaded}
                  onClick={() => (searchOpen ? closeSearch() : openSearch())}
                >
                  <Icon name="search" />
                </button>
                <button
                  type="button"
                  className="btn"
                  title={t("Discard your edits and restore the book's original CSS")}
                  disabled={!loaded || busy || (!loaded.modified && !dirty)}
                  onClick={() => setPending({ kind: "reset" })}
                >
                  <Icon name="undo" />
                  {t("Restore original")}
                </button>
                <button
                  type="button"
                  className="btn btn--primary"
                  title={`${t("Save")} (⌘/Ctrl+S)`}
                  disabled={!dirty || busy}
                  onClick={() => void save()}
                >
                  <Icon name="save" />
                  {saving ? t("Saving…") : t("Save")}
                </button>
              </div>

              {searchOpen ? (
                <search className="stylesheet-search" id="stylesheet-search">
                  <label className="library-search stylesheet-search__field">
                    <Icon name="search" size={16} />
                    <input
                      ref={searchInput}
                      type="search"
                      aria-label={t("Find in stylesheet")}
                      placeholder={t("Find")}
                      value={query}
                      onChange={(event) => {
                        setQuery(event.target.value);
                        setCurrent(0);
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          step(event.shiftKey ? -1 : 1);
                        }
                      }}
                    />
                  </label>
                  <span className="stylesheet-search__count" aria-live="polite">
                    {query === ""
                      ? ""
                      : matches.length === 0
                        ? t("No matches")
                        : `${currentMatch + 1} / ${matches.length}`}
                  </span>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("Previous match")}
                    title={`${t("Previous match")} (Shift+Enter)`}
                    disabled={matches.length === 0}
                    onClick={() => step(-1)}
                  >
                    <Icon name="chevronUp" />
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("Next match")}
                    title={`${t("Next match")} (Enter)`}
                    disabled={matches.length === 0}
                    onClick={() => step(1)}
                  >
                    <Icon name="chevronDown" />
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("Close find")}
                    title={`${t("Close find")} (Esc)`}
                    onClick={closeSearch}
                  >
                    <Icon name="close" />
                  </button>
                </search>
              ) : null}

              {pending ? (
                <div
                  className="stylesheet-confirm"
                  role="alertdialog"
                  aria-live="assertive"
                >
                  <p className="stylesheet-confirm__text">
                    {pending.kind === "reset"
                      ? t(
                          "Restore the original stylesheet? Your saved and unsaved edits to this file will be discarded.",
                        )
                      : t("This stylesheet has unsaved changes.")}
                  </p>
                  <div className="button-row">
                    {pending.kind === "reset" ? (
                      <button
                        type="button"
                        className="btn btn--danger"
                        onClick={() => finishPending(pending)}
                      >
                        <Icon name="undo" />
                        {t("Restore original")}
                      </button>
                    ) : (
                      <>
                        <button
                          type="button"
                          className="btn btn--primary"
                          onClick={() => void saveThenContinue()}
                        >
                          <Icon name="save" />
                          {t("Save")}
                        </button>
                        <button
                          type="button"
                          className="btn btn--danger"
                          onClick={() => finishPending(pending)}
                        >
                          <Icon name="trash" size={16} />
                          {t("Discard changes")}
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      className="btn"
                      onClick={() => setPending(null)}
                    >
                      <Icon name="edit" />
                      {t("Keep editing")}
                    </button>
                  </div>
                </div>
              ) : null}

              {saveError ? <StatusNotice kind="error">{saveError}</StatusNotice> : null}
              {savedNotice && !dirty ? (
                <StatusNotice kind="success">{savedNotice}</StatusNotice>
              ) : null}

              <div className="stylesheet-editor__scroll">
                {file.status === "loading" ? (
                  <StatusNotice kind="loading">{t("Opening stylesheet…")}</StatusNotice>
                ) : null}
                {file.status === "error" ? (
                  <StatusNotice kind="error" onRetry={() => openFile(file.href)}>
                    {file.message}
                  </StatusNotice>
                ) : null}
                {file.status === "ready" ? (
                  <CssCodeEditor
                    ref={editor}
                    value={draft}
                    label={`${t("CSS for")} ${selectedSheet?.name ?? ""}`}
                    disabled={saving}
                    matches={matches}
                    currentMatch={currentMatch}
                    onChange={(value) => {
                      setDraft(value);
                      setSavedNotice(null);
                    }}
                  />
                ) : null}
              </div>
            </section>
          </div>
        ) : null}

        <p className="dialog__muted stylesheet-dialog__note">
          {t(
            "Edits are saved with this book and used when you export a translated EPUB or KFX. The original EPUB is never changed.",
          )}
        </p>
      </div>
    </div>
  );
}
