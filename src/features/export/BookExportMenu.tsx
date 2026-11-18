import {
  type CSSProperties,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Icon, type IconName } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  type ExportFormat,
  exportOriginal,
  pickExportDestination,
} from "../../lib/tauri/commands";
import type { ExportOutcome } from "../../types/chapter";
import type { ProjectSummary } from "../../types/project";
import { ExportDialog } from "./ExportDialog";
import { ExportOutcomePanel } from "./ExportOutcomePanel";

type Variant = "original" | "translated";

const GROUPS: { format: ExportFormat; label: string }[] = [
  { format: "epub", label: "EPUB" },
  { format: "kfx", label: "KFX (Kindle)" },
];

const ITEMS: { variant: Variant; label: string; icon: IconName }[] = [
  { variant: "original", label: "Original", icon: "book" },
  { variant: "translated", label: "Translated", icon: "languages" },
];

const ITEM_NAMES: Record<ExportFormat, Record<Variant, string>> = {
  epub: { original: "Export original EPUB", translated: "Export translated EPUB" },
  kfx: { original: "Export original KFX", translated: "Export translated KFX" },
};

/**
 * Export menu: EPUB / KFX, each as the original book or the translation.
 * Translated exports open the review dialog; original exports go straight
 * to the Save dialog.
 */
export function BookExportMenu({
  project,
  onFlushSaves,
  iconSize,
  disabled = false,
}: {
  project: ProjectSummary;
  /** Flushes pending chapter saves before a translated export. */
  onFlushSaves: () => Promise<boolean>;
  iconSize?: number;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [dialogFormat, setDialogFormat] = useState<ExportFormat | null>(null);
  const [busy, setBusy] = useState<ExportFormat | null>(null);
  const [outcome, setOutcome] = useState<ExportOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLFieldSetElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const initialItem = useRef(0);
  const menuId = useId();
  // The popover is fixed to the viewport so library rows and panes can't clip it.
  const [placement, setPlacement] = useState<CSSProperties | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPlacement(null);
      return;
    }
    const button = trigger.current?.getBoundingClientRect();
    const height = menu.current?.offsetHeight ?? 0;
    if (!button) return;
    const gap = 6;
    const margin = 8;
    const right = Math.max(margin, window.innerWidth - button.right);
    const fitsBelow = button.bottom + gap + height <= window.innerHeight - margin;
    const fitsAbove = button.top - gap - height >= margin;
    setPlacement(
      !fitsBelow && fitsAbove
        ? { bottom: window.innerHeight - button.top + gap, right }
        : {
            top: Math.max(
              margin,
              Math.min(button.bottom + gap, window.innerHeight - height - margin),
            ),
            right,
          },
    );
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target))
        setOpen(false);
    };
    // A fixed popover would drift from its button; close instead.
    const dismiss = (event: Event) => {
      if (!(event.target instanceof Node && menu.current?.contains(event.target)))
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open || !placement) return;
    const items = menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    items?.[initialItem.current < 0 ? items.length - 1 : initialItem.current]?.focus({
      preventScroll: true,
    });
  }, [open, placement]);

  function close() {
    setOpen(false);
    trigger.current?.focus();
  }

  async function exportOriginalBook(format: ExportFormat) {
    setError(null);
    const fileBase = project.title.replace(/[\\/:*?"<>|]/g, "_");
    let destination: string | null;
    try {
      destination = await pickExportDestination(`${fileBase}-original.${format}`, format);
    } catch (pickError) {
      setError(
        pickError instanceof Error
          ? pickError.message
          : "The Save dialog could not open.",
      );
      return;
    }
    if (destination === null) return; // Save dialog cancelled.
    setBusy(format);
    try {
      setOutcome(await exportOriginal(project.id, destination, format));
    } catch (exportError) {
      setError(
        exportError instanceof Error
          ? exportError.message
          : "The book could not be exported.",
      );
    } finally {
      setBusy(null);
    }
  }

  function choose(format: ExportFormat, variant: Variant) {
    close();
    if (variant === "translated") setDialogFormat(format);
    else void exportOriginalBook(format);
  }

  const label = busy ? t("Exporting…") : t("Export");

  return (
    <>
      <fieldset
        aria-label={t("Export")}
        className="export-menu"
        ref={root}
        onBlur={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null))
            setOpen(false);
        }}
      >
        <button
          ref={trigger}
          type="button"
          className="icon-button"
          aria-label={busy ? label : `${label}: ${project.title}`}
          title={busy ? label : t("Export as EPUB or KFX")}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? menuId : undefined}
          aria-busy={busy !== null}
          disabled={disabled || busy !== null}
          onClick={() => {
            initialItem.current = 0;
            setOpen(!open);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              initialItem.current = event.key === "ArrowUp" ? -1 : 0;
              setOpen(true);
            }
          }}
        >
          <Icon name="download" size={iconSize} />
        </button>
        {open ? (
          <div
            ref={menu}
            id={menuId}
            className="export-menu__popover"
            style={placement ?? { top: 0, right: 0, visibility: "hidden" }}
            role="menu"
            aria-label={t("Export")}
            onKeyDown={(event) => {
              const items = Array.from(
                event.currentTarget.querySelectorAll<HTMLButtonElement>(
                  '[role="menuitem"]',
                ),
              );
              const index = items.indexOf(document.activeElement as HTMLButtonElement);
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                close();
              } else if (event.key === "Tab") {
                setOpen(false);
              } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
                event.preventDefault();
                const next =
                  event.key === "Home"
                    ? 0
                    : event.key === "End"
                      ? items.length - 1
                      : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
                        items.length;
                items[next]?.focus({ preventScroll: true });
              }
            }}
          >
            {GROUPS.map((group) => (
              <fieldset
                key={group.format}
                aria-labelledby={`${menuId}-${group.format}`}
                className="export-menu__group"
              >
                <div id={`${menuId}-${group.format}`} className="export-menu__heading">
                  {group.label}
                </div>
                {ITEMS.map((item) => (
                  <button
                    key={item.variant}
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    className="export-menu__item"
                    aria-label={t(ITEM_NAMES[group.format][item.variant])}
                    title={t(ITEM_NAMES[group.format][item.variant])}
                    onClick={() => choose(group.format, item.variant)}
                  >
                    <Icon name={item.icon} size={16} />
                    <span>{t(item.label)}</span>
                  </button>
                ))}
              </fieldset>
            ))}
          </div>
        ) : null}
      </fieldset>
      {busy ? (
        <div className="export-menu__progress">
          <StatusNotice kind="loading">
            {busy === "kfx" ? t("Converting the original book to KFX…") : t("Exporting…")}
          </StatusNotice>
        </div>
      ) : null}
      {dialogFormat ? (
        <ExportDialog
          project={project}
          format={dialogFormat}
          onClose={() => setDialogFormat(null)}
          onFlushSaves={onFlushSaves}
          onOutcome={(result) => {
            setOutcome(result);
            setDialogFormat(null);
          }}
        />
      ) : null}
      {outcome ? (
        <ExportOutcomePanel
          outcome={outcome}
          onDismiss={() => {
            setOutcome(null);
            trigger.current?.focus();
          }}
        />
      ) : null}
      {error ? (
        <div className="dialog-backdrop">
          <div className="dialog" role="alertdialog" aria-labelledby={`${menuId}-error`}>
            <div className="panel__header">
              <h2 className="panel__heading" id={`${menuId}-error`}>
                {t("Export failed")}
              </h2>
              <button
                type="button"
                className="icon-button"
                aria-label={t("Close")}
                title={t("Close")}
                // biome-ignore lint/a11y/noAutofocus: the error dialog takes focus so it can be dismissed.
                autoFocus
                onClick={() => {
                  setError(null);
                  trigger.current?.focus();
                }}
              >
                <Icon name="close" />
              </button>
            </div>
            <StatusNotice kind="error">{error}</StatusNotice>
          </div>
        </div>
      ) : null}
    </>
  );
}
