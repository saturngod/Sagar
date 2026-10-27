import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";

export function ReadingControls({
  workspaceRef,
  scrollRef,
  canGoPrevious,
  canGoNext,
  navigationBusy,
  onPrevious,
  onNext,
  onClose,
}: {
  workspaceRef: RefObject<HTMLDivElement | null>;
  scrollRef: RefObject<HTMLDivElement | null>;
  canGoPrevious: boolean;
  canGoNext: boolean;
  navigationBusy: boolean;
  onPrevious: () => void;
  onNext: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(true);
  const controlsRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const focusCloseAfterReveal = useRef(false);
  const navigating = useRef(false);
  const navigationBusyRef = useRef(navigationBusy);
  navigationBusyRef.current = navigationBusy;

  const startNavigation = (navigate: () => void) => {
    navigating.current = true;
    navigate();
    requestAnimationFrame(() => {
      if (!navigationBusyRef.current) navigating.current = false;
    });
  };

  useEffect(() => {
    if (navigationBusy || !navigating.current) return;
    const frame = requestAnimationFrame(() => {
      navigating.current = false;
    });
    return () => cancelAnimationFrame(frame);
  }, [navigationBusy]);

  useEffect(() => {
    if (visible && focusCloseAfterReveal.current) {
      focusCloseAfterReveal.current = false;
      closeRef.current?.focus();
    }
  }, [visible]);

  const hideControls = useCallback(() => {
    const active = document.activeElement;
    if (active && controlsRef.current?.contains(active)) {
      const content = workspaceRef.current?.querySelector<HTMLElement>(
        "[data-reading-content]",
      );
      if (content) content.focus({ preventScroll: true });
      else if (active instanceof HTMLElement) active.blur();
    }
    setVisible(false);
  }, [workspaceRef]);

  useEffect(() => {
    const workspace = workspaceRef.current;
    const body = workspace?.querySelector<HTMLElement>(".workspace__body");
    const scroller = scrollRef.current;
    if (!workspace || !body || !scroller) return;

    const onClick = (event: MouseEvent) => {
      const target = event.target;
      const selection = window.getSelection();
      if (
        event.button !== 0 ||
        (target instanceof Element &&
          target.closest(
            "a, button, input, select, textarea, [contenteditable='true']",
          )) ||
        (selection !== null && !selection.isCollapsed)
      )
        return;
      if (visible) hideControls();
      else setVisible(true);
    };
    const onScroll = () => {
      // A chapter change resets the scroll position; that is not a reader scroll.
      if (!navigating.current) hideControls();
    };
    const onUserScroll = () => {
      navigating.current = false;
      hideControls();
    };
    body.addEventListener("click", onClick);
    scroller.addEventListener("scroll", onScroll, { passive: true });
    // Controls are siblings of the scroller; scrolling over them must hide UI too.
    workspace.addEventListener("wheel", onUserScroll, { passive: true, capture: true });
    workspace.addEventListener("touchmove", onUserScroll, {
      passive: true,
      capture: true,
    });
    return () => {
      body.removeEventListener("click", onClick);
      scroller.removeEventListener("scroll", onScroll);
      workspace.removeEventListener("wheel", onUserScroll, true);
      workspace.removeEventListener("touchmove", onUserScroll, true);
    };
  }, [hideControls, scrollRef, visible, workspaceRef]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      } else if (event.key === "Tab" && !visible) {
        event.preventDefault();
        focusCloseAfterReveal.current = true;
        setVisible(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, visible]);

  return (
    <div
      ref={controlsRef}
      className={`workspace__reading-controls${visible ? " workspace__reading-controls--visible" : ""}`}
      aria-hidden={!visible}
      inert={!visible}
    >
      <button
        ref={closeRef}
        type="button"
        className="workspace__reading-control workspace__reading-control--close"
        aria-label={t("Close reading view (Esc)")}
        title={t("Close reading view (Esc)")}
        onClick={onClose}
      >
        <Icon name="close" />
      </button>
      <button
        type="button"
        className="workspace__reading-control workspace__reading-control--previous"
        aria-label={t("Previous chapter")}
        title={t("Previous chapter")}
        disabled={!canGoPrevious}
        onClick={() => startNavigation(onPrevious)}
      >
        <Icon name="left" />
      </button>
      <button
        type="button"
        className="workspace__reading-control workspace__reading-control--next"
        aria-label={t("Next chapter")}
        title={t("Next chapter")}
        disabled={!canGoNext}
        onClick={() => startNavigation(onNext)}
      >
        <Icon name="right" />
      </button>
    </div>
  );
}
