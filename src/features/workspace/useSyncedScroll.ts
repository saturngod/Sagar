import { type RefObject, useEffect } from "react";

/**
 * Proportional scroll position: given the scrolled pane's offset and both
 * scrollable ranges, return the other pane's offset at the same ratio.
 * Returns null when either side cannot scroll or is already close enough.
 */
export function proportionalScrollTop(
  fromScrollTop: number,
  fromMax: number,
  toScrollTop: number,
  toMax: number,
): number | null {
  if (fromMax <= 0 || toMax <= 0) return null;
  const ratio = Math.min(1, Math.max(0, fromScrollTop / fromMax));
  const desired = ratio * toMax;
  if (Math.abs(toScrollTop - desired) < 1) return null;
  return desired;
}

/**
 * Keep the Original and Translation pane scrollers in lock-step.
 *
 * The two chapters are the same logical position but different languages,
 * so their pixel heights differ. Sync proportionally: when one pane scrolls
 * to N% of its scrollable range, the other pane is moved to N% of its own
 * range. A single frame lock prevents the programmatic `scrollTop` write
 * from echoing back and causing a feedback loop.
 */
export function useSyncedScroll(
  sourceRef: RefObject<HTMLDivElement | null>,
  targetRef: RefObject<HTMLDivElement | null>,
  resetKey: string | null | undefined,
  enabled = false,
  linkRequested = enabled,
) {
  // Start each chapter at the top on both sides.
  // biome-ignore lint/correctness/useExhaustiveDependencies: resetKey intentionally retriggers the reset without being read.
  useEffect(() => {
    sourceRef.current?.scrollTo({ top: 0 });
    targetRef.current?.scrollTo({ top: 0 });
  }, [resetKey]);

  // Align only when the user turns linking on. Temporarily hiding a pane or
  // entering reading mode should not move the reader back on return.
  // biome-ignore lint/correctness/useExhaustiveDependencies: linkRequested tracks the user's setting, independently of temporary pauses.
  useEffect(() => {
    if (!linkRequested || !enabled) return;
    const source = sourceRef.current;
    const target = targetRef.current;
    if (!source || !target) return;
    const desired = proportionalScrollTop(
      source.scrollTop,
      source.scrollHeight - source.clientHeight,
      target.scrollTop,
      target.scrollHeight - target.clientHeight,
    );
    if (desired !== null) target.scrollTop = desired;
  }, [linkRequested]);

  useEffect(() => {
    if (!enabled) return;
    const source = sourceRef.current;
    const target = targetRef.current;
    if (!source || !target) return;

    let locked = false;

    const sync = (from: HTMLDivElement, to: HTMLDivElement) => {
      if (locked) return;
      const desired = proportionalScrollTop(
        from.scrollTop,
        from.scrollHeight - from.clientHeight,
        to.scrollTop,
        to.scrollHeight - to.clientHeight,
      );
      if (desired === null) return;
      locked = true;
      to.scrollTop = desired;
      requestAnimationFrame(() => {
        locked = false;
      });
    };

    const onSourceScroll = () => sync(source, target);
    const onTargetScroll = () => sync(target, source);

    source.addEventListener("scroll", onSourceScroll, { passive: true });
    target.addEventListener("scroll", onTargetScroll, { passive: true });
    return () => {
      source.removeEventListener("scroll", onSourceScroll);
      target.removeEventListener("scroll", onTargetScroll);
    };
  }, [sourceRef, targetRef, enabled]);
}
