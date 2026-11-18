import { useCallback, useEffect, useRef, useState } from "react";
import {
  findTextMatches,
  firstVisibleMatch,
  paintMatches,
  scrollMatchIntoView,
} from "../../lib/search/textSearch";

export type SearchPane = "source" | "target";

/** Cmd/Ctrl+F opens search in the pane the user last worked in. */
let lastActivePane: SearchPane = "target";

function isVisible(element: Element | null): boolean {
  return !!element && element.getClientRects().length > 0;
}

/** A short single-line selection becomes the initial query, like browser find. */
function selectedText(pane: HTMLElement): string {
  const active = document.activeElement;
  let text = "";
  if (active instanceof HTMLTextAreaElement && pane.contains(active)) {
    text = active.value.slice(active.selectionStart, active.selectionEnd);
  } else {
    const selection = window.getSelection();
    if (selection?.anchorNode && pane.contains(selection.anchorNode)) {
      text = selection.toString();
    }
  }
  return text.length > 0 && text.length <= 200 && !/[\r\n]/.test(text) ? text : "";
}

/**
 * Search state for one workspace pane. `getRoot` picks, inside the pane, the
 * element whose rendered text is searched for the current view; `revision` changes
 * whenever that text may have changed, so matches are recomputed.
 */
export function usePaneSearch({
  pane,
  available,
  getRoot,
  revision,
}: {
  pane: SearchPane;
  available: boolean;
  getRoot: (pane: HTMLElement) => HTMLElement | null;
  revision: unknown;
}) {
  const paneRef = useRef<HTMLElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  /** Set by user actions only, so edits while searching never move the view. */
  const pendingScroll = useRef(false);
  const pendingReset = useRef(false);
  const pendingFocus = useRef(false);
  const [open, setOpen] = useState(false);
  const [query, setQueryState] = useState("");
  const [matches, setMatches] = useState<Range[]>([]);
  const [index, setIndex] = useState(0);
  const active = open && available;

  // biome-ignore lint/correctness/useExhaustiveDependencies: `revision` signals that the searched text changed.
  useEffect(() => {
    const element = paneRef.current;
    const root = active && query && element ? getRoot(element) : null;
    const found = root ? findTextMatches(root, query) : [];
    setMatches(found);
    if (pendingReset.current) {
      pendingReset.current = false;
      setIndex(firstVisibleMatch(found));
    } else {
      setIndex((current) => Math.min(current, Math.max(found.length - 1, 0)));
    }
  }, [active, query, getRoot, revision]);

  useEffect(() => {
    const current = matches[index] ?? null;
    const clear = paintMatches(matches, current);
    if (current && pendingScroll.current) scrollMatchIntoView(current);
    pendingScroll.current = false;
    return clear;
  }, [matches, index]);

  const setQuery = useCallback((value: string) => {
    pendingReset.current = true;
    pendingScroll.current = true;
    setQueryState(value);
  }, []);

  const step = useCallback(
    (delta: 1 | -1) => {
      if (matches.length === 0) return;
      pendingScroll.current = true;
      setIndex((current) => (current + delta + matches.length) % matches.length);
    },
    [matches.length],
  );
  const next = useCallback(() => step(1), [step]);
  const previous = useCallback(() => step(-1), [step]);

  const openSearch = useCallback(() => {
    const element = paneRef.current;
    const focused = document.activeElement;
    if (
      focused instanceof HTMLElement &&
      focused !== inputRef.current &&
      element?.contains(focused)
    ) {
      restoreFocus.current = focused;
    }
    const selected = element ? selectedText(element) : "";
    if (selected) setQuery(selected);
    else if (!open) pendingScroll.current = true;
    setOpen(true);
    if (inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    } else {
      // The bar mounts with the next render; the effect below focuses it.
      pendingFocus.current = true;
    }
  }, [open, setQuery]);

  useEffect(() => {
    if (!active || !pendingFocus.current) return;
    pendingFocus.current = false;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [active]);

  const close = useCallback(() => {
    setOpen(false);
    const target = restoreFocus.current;
    restoreFocus.current = null;
    if (target?.isConnected) target.focus({ preventScroll: true });
  }, []);

  const toggle = useCallback(() => {
    if (active) close();
    else openSearch();
  }, [active, close, openSearch]);

  useEffect(() => {
    const element = paneRef.current;
    if (!element) return;
    const mark = () => {
      lastActivePane = pane;
    };
    element.addEventListener("pointerdown", mark, true);
    element.addEventListener("focusin", mark);
    return () => {
      element.removeEventListener("pointerdown", mark, true);
      element.removeEventListener("focusin", mark);
    };
  }, [pane]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.defaultPrevented) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key !== "f" && key !== "g") return;
      if (!available || !isVisible(paneRef.current)) return;
      if (document.querySelector(".dialog-backdrop")) return;
      const other = document.querySelector(
        pane === "source" ? ".pane--target" : ".pane--source",
      );
      // Fall back to this pane when the last-used one is hidden.
      if (lastActivePane !== pane && isVisible(other)) return;
      if (key === "f") {
        if (event.shiftKey) return;
        event.preventDefault();
        openSearch();
      } else if (open) {
        event.preventDefault();
        step(event.shiftKey ? -1 : 1);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [available, open, openSearch, pane, step]);

  return {
    paneRef,
    inputRef,
    open: active,
    query,
    setQuery,
    count: matches.length,
    index,
    next,
    previous,
    toggle,
    close,
  };
}

export type PaneSearch = ReturnType<typeof usePaneSearch>;
