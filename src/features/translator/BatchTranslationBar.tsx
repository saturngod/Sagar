import { Icon } from "../../components/Icon";
import { formatDuration } from "../../lib/duration";
import { useTranslation } from "../../lib/i18n";
import { type BatchState, useAiBatch, useNow } from "./aiBatch";

/** Progress line for a batch run, e.g. "3 of 12 · Chapter 3 · 1:05". */
export function batchProgress(
  batch: BatchState,
  now: number,
  t: (message: string) => string,
): string {
  const position = `${Math.min(batch.processed + 1, batch.total)} ${t("of")} ${batch.total}`;
  if (batch.phase === "waiting" && batch.waitUntil) {
    const left = Math.max(0, Math.ceil((batch.waitUntil - now) / 1000));
    return `${batch.processed} ${t("of")} ${batch.total} · ${t("Next chapter in")} ${formatDuration(left)}`;
  }
  if (batch.phase === "running") {
    const elapsed = batch.chapterStartedAt
      ? formatDuration(Math.max(0, Math.floor((now - batch.chapterStartedAt) / 1000)))
      : "";
    return [position, batch.currentTitle, elapsed].filter(Boolean).join(" · ");
  }
  const parts = [`${batch.translated} ${t("translated")}`];
  if (batch.skipped) parts.push(`${batch.skipped} ${t("skipped")}`);
  if (batch.failures.length) parts.push(`${batch.failures.length} ${t("failed")}`);
  return parts.join(" · ");
}

export function batchStateLabel(batch: BatchState): string {
  if (batch.phase === "waiting") return "Pausing";
  if (batch.phase === "running") return "Translating all chapters";
  if (batch.phase === "stopped") return "Stopped";
  if (batch.phase === "failed") return "Stopped after errors";
  return "Finished translating all chapters";
}

/**
 * Persistent strip for "translate all untranslated chapters", shown in the
 * library and the workspace so a long-running batch is never invisible.
 */
export function BatchTranslationBar({ projectId }: { projectId?: string }) {
  const { t } = useTranslation();
  const { batch, stop, dismiss } = useAiBatch();
  const active = batch?.phase === "running" || batch?.phase === "waiting";
  const now = useNow(active);
  if (!batch || (projectId && batch.projectId !== projectId)) return null;
  const failed = batch.phase === "failed" || batch.failures.length > 0;
  return (
    <section className="ai-background" aria-label={t("Translate all chapters")}>
      <div
        className={`ai-background__job ai-background__job--${
          active ? "running" : failed ? "failed" : "done"
        }`}
      >
        <span className="ai-background__icon">
          <Icon name="sparkles" spin={batch.phase === "running"} />
        </span>
        <span className="ai-background__text" role="status" aria-live="polite">
          <strong>{batch.projectTitle}</strong>
          <span className="ai-background__meta">
            {t(batchStateLabel(batch))} · {batchProgress(batch, now, t)}
          </span>
          {batch.message ? (
            <span className="ai-background__message">{t(batch.message)}</span>
          ) : null}
          {!active && batch.failures.length ? (
            <span className="ai-background__message">
              {batch.failures
                .slice(0, 3)
                .map((failure) => `${failure.title}: ${failure.message}`)
                .join(" · ")}
            </span>
          ) : null}
        </span>
        {active ? (
          <button
            type="button"
            className="icon-button"
            aria-label={t("Stop translating all chapters")}
            title={t("Stop translating all chapters")}
            onClick={stop}
          >
            <Icon name="stop" />
          </button>
        ) : (
          <button
            type="button"
            className="icon-button"
            aria-label={t("Dismiss")}
            title={t("Dismiss")}
            onClick={dismiss}
          >
            <Icon name="close" />
          </button>
        )}
      </div>
    </section>
  );
}
