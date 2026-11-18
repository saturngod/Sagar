import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import type { ExportOutcome } from "../../types/chapter";

const HEADINGS: Record<string, string> = {
  original: "Original book exported",
  completed: "Completed translation exported",
  draft: "Draft translation exported",
};

/** Result of an export: output path, counts and any conversion warnings. */
export function ExportOutcomePanel({
  outcome,
  onDismiss,
}: {
  outcome: ExportOutcome;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const isOriginal = outcome.mode === "original";
  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="alertdialog" aria-labelledby="export-outcome-title">
        <div className="panel__header">
          <h2 className="panel__heading" id="export-outcome-title">
            {t(HEADINGS[outcome.mode] ?? HEADINGS.draft)}
          </h2>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Close")}
            title={t("Close")}
            // biome-ignore lint/a11y/noAutofocus: the result dialog takes focus so Escape/Enter work.
            autoFocus
            onClick={onDismiss}
          >
            <Icon name="close" />
          </button>
        </div>
        <StatusNotice kind="success">
          {isOriginal
            ? `${t("The original book was exported without translations.")} ${outcome.chaptersExported} ${t(outcome.chaptersExported === 1 ? "chapter" : "chapters")}.`
            : `${outcome.blocksTranslated} ${t(outcome.blocksTranslated === 1 ? "translated chapter" : "translated chapters")} · ${outcome.chaptersExported} ${t(outcome.chaptersExported === 1 ? "chapter" : "chapters")}.`}
        </StatusNotice>
        <p className="dialog__path">
          {t("Output file:")}
          <code>{outcome.outputPath}</code>
        </p>
        {outcome.warnings.length > 0 ? (
          <ul className="warning-list">
            {outcome.warnings.map((warning, index) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: static per-export list
              <li key={`${warning.code}-${index}`} className="warning-list__item">
                {warning.location ? (
                  <strong className="warning-list__location">{warning.location}</strong>
                ) : null}
                <span>{warning.message}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="dialog__muted">{t("No validation warnings.")}</p>
        )}
      </div>
    </div>
  );
}
