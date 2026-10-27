import { useEffect, useId, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";

type Pane = "original" | "translation";

export function PaneVisibilityMenu({
  visible,
  onToggle,
}: {
  visible: Record<Pane, boolean>;
  onToggle: (pane: Pane) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLFieldSetElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const initialItem = useRef(0);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    menu.current
      ?.querySelectorAll<HTMLButtonElement>("button")
      [initialItem.current]?.focus();
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target))
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  function close() {
    setOpen(false);
    trigger.current?.focus();
  }

  return (
    <fieldset
      aria-label={t("Show or hide panes")}
      className="pane-visibility-menu"
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
        aria-label={t("Show or hide panes")}
        title={t("Show or hide panes")}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => {
          initialItem.current = 0;
          setOpen(!open);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            initialItem.current = event.key === "ArrowUp" ? 1 : 0;
            setOpen(true);
          }
        }}
      >
        <Icon name="moreVertical" />
      </button>
      {open ? (
        <div
          ref={menu}
          id={menuId}
          className="pane-visibility-menu__popover"
          role="menu"
          aria-label={t("Show or hide panes")}
          onKeyDown={(event) => {
            const items = Array.from(
              event.currentTarget.querySelectorAll<HTMLButtonElement>("button"),
            );
            const index = items.indexOf(document.activeElement as HTMLButtonElement);
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              close();
            } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
              event.preventDefault();
              const next =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? items.length - 1
                    : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
                      items.length;
              items[next]?.focus();
            }
          }}
        >
          {(["original", "translation"] as const).map((pane) => {
            const other = pane === "original" ? "translation" : "original";
            const disabled = visible[pane] && !visible[other];
            return (
              <button
                key={pane}
                type="button"
                role="menuitemcheckbox"
                tabIndex={-1}
                aria-checked={visible[pane]}
                aria-disabled={disabled}
                title={disabled ? t("Keep at least one pane visible.") : undefined}
                className="pane-visibility-menu__item"
                onClick={() => {
                  if (!disabled) onToggle(pane);
                }}
              >
                <span className="pane-visibility-menu__check" aria-hidden="true">
                  {visible[pane] ? <Icon name="check" /> : null}
                </span>
                <span>
                  {t(pane === "original" ? "Show Original" : "Show Translation")}
                </span>
              </button>
            );
          })}
        </div>
      ) : null}
    </fieldset>
  );
}
