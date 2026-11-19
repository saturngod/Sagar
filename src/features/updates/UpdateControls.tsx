import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import { formatBytes, useAppUpdate } from "./UpdateProvider";

/** Progress bar + primary action for the current update state. */
function UpdateActions({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const { state, checkNow, install, restart, openReleasePage } = useAppUpdate();

  if (state.kind === "downloading") {
    const percent =
      state.total && state.total > 0
        ? Math.min(100, Math.round((state.received / state.total) * 100))
        : null;
    return (
      <div className="update-progress">
        <progress
          className="update-progress__bar"
          max={100}
          value={percent ?? undefined}
          aria-label={t("Download progress")}
        />
        <span className="dialog__muted" aria-live="polite">
          {t("Downloading…")}{" "}
          {state.total
            ? `${formatBytes(state.received)} / ${formatBytes(state.total)}`
            : formatBytes(state.received)}
        </span>
      </div>
    );
  }

  if (state.kind === "ready") {
    return (
      <div className="update-actions">
        <button
          type="button"
          className="btn btn--primary update-btn"
          onClick={() => void restart()}
        >
          <Icon name="refresh" />
          <span>{t("Restart now")}</span>
        </button>
      </div>
    );
  }

  if (state.kind === "available") {
    return (
      <div className="update-actions">
        <button
          type="button"
          className="btn btn--primary update-btn"
          onClick={() => void install()}
        >
          <Icon name="download" />
          <span>{t("Update now")}</span>
        </button>
        {compact ? null : (
          <button type="button" className="btn update-btn" onClick={openReleasePage}>
            <Icon name="globe" />
            <span>{t("Release notes")}</span>
          </button>
        )}
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div className="update-actions">
        <button
          type="button"
          className="btn update-btn"
          onClick={() => void (state.phase === "install" ? install() : checkNow())}
        >
          <Icon name="refresh" />
          <span>{t("Retry")}</span>
        </button>
        <button type="button" className="btn update-btn" onClick={openReleasePage}>
          <Icon name="globe" />
          <span>{t("Open release page")}</span>
        </button>
      </div>
    );
  }
  return null;
}

/** Settings → Updates tab. */
export function UpdateSettings() {
  const { t } = useTranslation();
  const { state, currentVersion, autoCheck, setAutoCheck, checkNow } = useAppUpdate();
  const checking = state.kind === "checking";
  const working = checking || state.kind === "downloading";

  return (
    <section
      className="provider-section update-settings"
      aria-labelledby="update-settings-heading"
      aria-busy={working}
    >
      <div className="provider-section__header">
        <h3 className="panel__heading" id="update-settings-heading">
          {t("Updates")}
        </h3>
        <button
          type="button"
          className="btn update-btn"
          disabled={working || state.kind === "ready"}
          onClick={() => void checkNow()}
        >
          <Icon name="refresh" spin={checking} />
          <span>{t(checking ? "Checking…" : "Check for updates")}</span>
        </button>
      </div>
      <p className="update-settings__text">
        {t("Current version")}: <strong>{currentVersion ?? "—"}</strong>
      </p>

      {state.kind === "upToDate" ? (
        <StatusNotice kind="success">{t("Sagar is up to date.")}</StatusNotice>
      ) : null}
      {state.kind === "available" ? (
        <StatusNotice kind="info">
          {t("Version {version} is available.").replace("{version}", state.version)}
        </StatusNotice>
      ) : null}
      {state.kind === "ready" ? (
        <StatusNotice kind="success">
          {t("Version {version} is installed. Restart Sagar to use it.").replace(
            "{version}",
            state.version,
          )}
        </StatusNotice>
      ) : null}
      {state.kind === "error" ? (
        <StatusNotice kind="error">{state.message}</StatusNotice>
      ) : null}
      {state.kind === "available" && state.notes ? (
        <details className="update-settings__notes">
          <summary>{t("What’s new")}</summary>
          <p>{state.notes}</p>
        </details>
      ) : null}

      <UpdateActions />

      <label className="export-ack">
        <input
          type="checkbox"
          checked={autoCheck}
          onChange={(event) => setAutoCheck(event.target.checked)}
        />
        <span>{t("Check for updates when Sagar opens")}</span>
      </label>
      <p className="update-settings__text dialog__muted">
        {t(
          "Sagar only contacts GitHub to look for a new version. No books or translations are sent. Updates are signature-checked before they install.",
        )}
      </p>
    </section>
  );
}

/** Library banner shown when a launch check finds a new version. */
export function UpdateBanner() {
  const { t } = useTranslation();
  const { state, dismissed, dismiss } = useAppUpdate();
  const visible =
    !dismissed &&
    (state.kind === "available" ||
      state.kind === "downloading" ||
      state.kind === "ready" ||
      (state.kind === "error" && state.phase === "install"));
  if (!visible) return null;

  const message =
    state.kind === "ready"
      ? t("Version {version} is installed. Restart Sagar to use it.").replace(
          "{version}",
          state.version,
        )
      : state.kind === "error"
        ? t(state.message)
        : t("Sagar {version} is available.").replace("{version}", state.version);

  return (
    <section
      className={`update-banner${state.kind === "error" ? " update-banner--error" : ""}`}
      aria-label={t("App update")}
    >
      <span className="update-banner__icon">
        <Icon name={state.kind === "error" ? "close" : "download"} />
      </span>
      <p
        className="update-banner__text"
        role={state.kind === "error" ? "alert" : "status"}
      >
        {message}
      </p>
      <UpdateActions compact />
      {state.kind === "downloading" ? null : (
        <button
          type="button"
          className="icon-button"
          aria-label={t("Hide update notice")}
          title={t("Later")}
          onClick={dismiss}
        >
          <Icon name="close" />
        </button>
      )}
    </section>
  );
}
