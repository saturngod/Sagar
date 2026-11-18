import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";
import type { PaneSearch } from "./usePaneSearch";

/** Header button that shows or hides a pane's search bar. */
export function PaneSearchButton({
  search,
  label,
  controls,
  disabled,
}: {
  search: PaneSearch;
  label: string;
  controls: string;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      className="icon-button"
      aria-label={label}
      title={`${label} (Cmd/Ctrl + F)`}
      aria-expanded={search.open}
      aria-controls={controls}
      disabled={disabled}
      onClick={search.toggle}
    >
      <Icon name="search" />
    </button>
  );
}

/** Inline search row shown under a pane header; never a dialog. */
export function PaneSearchBar({
  id,
  search,
  label,
}: {
  id: string;
  search: PaneSearch;
  label: string;
}) {
  const { t } = useTranslation();
  if (!search.open) return null;
  const empty = search.count === 0;
  return (
    <search id={id} className="pane-search" aria-label={label}>
      <input
        ref={search.inputRef}
        type="text"
        className="pane-search__input"
        aria-label={label}
        placeholder={t("Search…")}
        value={search.query}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        autoComplete="off"
        onChange={(event) => search.setQuery(event.target.value)}
        onKeyDown={(event) => {
          // Enter may be confirming an IME candidate, not asking for a match.
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Enter") {
            event.preventDefault();
            if (event.shiftKey) search.previous();
            else search.next();
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            search.close();
          }
        }}
      />
      <output
        className={`pane-search__count${search.query && empty ? " pane-search__count--empty" : ""}`}
        aria-live="polite"
      >
        {search.query
          ? empty
            ? t("No results")
            : `${search.index + 1} / ${search.count}`
          : ""}
      </output>
      <button
        type="button"
        className="icon-button"
        aria-label={t("Previous match")}
        title={`${t("Previous match")} (Shift + Enter)`}
        disabled={empty}
        onClick={search.previous}
      >
        <Icon name="chevronUp" />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("Next match")}
        title={`${t("Next match")} (Enter)`}
        disabled={empty}
        onClick={search.next}
      >
        <Icon name="chevronDown" />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("Close search")}
        title={`${t("Close search")} (Esc)`}
        onClick={search.close}
      >
        <Icon name="close" />
      </button>
    </search>
  );
}
