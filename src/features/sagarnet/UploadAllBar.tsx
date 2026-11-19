import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";
import { uploadProgress, uploadStateLabel, useUploadAll } from "./uploadAll";

/**
 * Persistent strip for "upload all chapters to SagarNet", shown in the library
 * and the workspace so a long upload is never invisible.
 */
export function UploadAllBar({ projectId }: { projectId?: string }) {
  const { t } = useTranslation();
  const { upload, stop, dismiss } = useUploadAll();
  if (!upload || (projectId && upload.projectId !== projectId)) return null;
  const active = upload.phase === "running";
  const failed = upload.phase === "failed" || upload.failures.length > 0;
  return (
    <section className="ai-background" aria-label={t("Upload all chapters to SagarNet")}>
      <div
        className={`ai-background__job ai-background__job--${
          active ? "running" : failed ? "failed" : "done"
        }`}
      >
        <span className="ai-background__icon">
          <Icon name={active ? "refresh" : "cloudUpload"} spin={active} />
        </span>
        <span className="ai-background__text" role="status" aria-live="polite">
          <strong>{upload.projectTitle}</strong>
          <span className="ai-background__meta">
            {t(uploadStateLabel(upload))} · {uploadProgress(upload, t)}
          </span>
          {upload.message ? (
            <span className="ai-background__message">{t(upload.message)}</span>
          ) : null}
          {!active && upload.failures.length ? (
            <span className="ai-background__message">
              {upload.failures
                .slice(0, 3)
                .map((failure) => `${failure.title}: ${failure.message}`)
                .join(" · ")}
              {upload.failures.length > 3
                ? ` · +${upload.failures.length - 3} ${t("more")}`
                : ""}
            </span>
          ) : null}
        </span>
        {active ? (
          <button
            type="button"
            className="icon-button"
            aria-label={t("Stop uploading to SagarNet")}
            title={t("Stop after the current chapter")}
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
