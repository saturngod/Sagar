import { useCallback, useEffect, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  installSkill,
  listSkillInstalls,
  uninstallSkill,
} from "../../lib/tauri/commands";
import type { SkillInstall, SkillInstallStatus } from "../../types/project";

const STATUS_LABEL: Record<SkillInstallStatus, string> = {
  notInstalled: "Not installed",
  installed: "Installed",
  updateAvailable: "Update available",
  modified: "Changed outside Sagar",
  conflict: "Not Sagar's folder",
};
const STATUS_TONE: Record<SkillInstallStatus, string> = {
  notInstalled: "original",
  installed: "reviewed",
  updateAvailable: "available",
  modified: "available",
  conflict: "original",
};

/**
 * Installs one translation skill into each coding CLI's skills folder.
 * `revision` changes when the skill's text changes, to refresh the statuses.
 */
export function SkillInstalls({
  skillId,
  revision,
}: {
  skillId: string;
  revision: string;
}) {
  const { t } = useTranslation();
  const [installs, setInstalls] = useState<SkillInstall[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{
    providerId: string;
    action: "install" | "uninstall";
  } | null>(null);

  const load = useCallback(() => {
    setLoadError(null);
    listSkillInstalls(skillId)
      .then(setInstalls)
      .catch((failure: unknown) =>
        setLoadError(
          failure instanceof Error
            ? failure.message
            : "Install status could not be read.",
        ),
      );
  }, [skillId]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: revision explicitly refreshes after the skill text changes.
  useEffect(() => {
    load();
  }, [load, revision]);

  async function run(
    install: SkillInstall,
    action: "install" | "uninstall",
    force: boolean,
  ): Promise<boolean> {
    try {
      if (action === "install") await installSkill(skillId, install.providerId, force);
      else await uninstallSkill(skillId, install.providerId, force);
      return true;
    } catch (failure) {
      setError(
        `${install.label}: ${failure instanceof Error ? failure.message : "The skill could not be changed."}`,
      );
      return false;
    }
  }

  async function act(
    install: SkillInstall,
    action: "install" | "uninstall",
    force: boolean,
  ) {
    setBusy(install.providerId);
    setError(null);
    setConfirm(null);
    await run(install, action, force);
    setBusy(null);
    load();
  }

  async function installAll() {
    if (!installs) return;
    setBusy("all");
    setError(null);
    setConfirm(null);
    for (const install of installs) {
      if (
        install.detected &&
        (install.status === "notInstalled" || install.status === "updateAvailable")
      ) {
        if (!(await run(install, "install", false))) break;
      }
    }
    setBusy(null);
    load();
  }

  if (loadError)
    return (
      <StatusNotice kind="error" onRetry={load}>
        {loadError}
      </StatusNotice>
    );
  if (!installs)
    return <StatusNotice kind="loading">{t("Checking coding CLIs…")}</StatusNotice>;

  const installed = (id: string) =>
    installs.some(
      (i) =>
        i.providerId === id && i.status !== "notInstalled" && i.status !== "conflict",
    );
  const opencodeTwice =
    installed("opencode") && (installed("claude_code") || installed("codex"));
  const canInstallAll = installs.some(
    (i) => i.detected && (i.status === "notInstalled" || i.status === "updateAvailable"),
  );

  return (
    <div className="skill-installs">
      <div className="skill-installs__header">
        <h4 className="skill-installs__title">{t("Install in coding CLIs")}</h4>
        <button
          type="button"
          className="btn btn--small"
          disabled={busy !== null || !canInstallAll}
          onClick={() => void installAll()}
        >
          <Icon name="download" />
          {busy === "all" ? t("Installing…") : t("Install in all found CLIs")}
        </button>
      </div>
      <p className="dialog__muted skill-installs__help">
        {t(
          "Copies this skill into each CLI's own skills folder, so you can use it outside Sagar too. Sagar's own translations don't need this. Sagar only changes folders it installed.",
        )}
      </p>
      {error ? <StatusNotice kind="error">{error}</StatusNotice> : null}
      <ul className="skill-installs__list">
        {installs.map((install) => {
          const rowBusy = busy === install.providerId || busy === "all";
          const asking = confirm?.providerId === install.providerId ? confirm : null;
          const canInstall =
            install.detected &&
            (install.status === "notInstalled" || install.status === "updateAvailable");
          const canRemove =
            install.status === "installed" || install.status === "updateAvailable";
          return (
            <li key={install.providerId} className="skill-installs__row">
              <div className="skill-installs__info">
                <strong>{install.label}</strong>
                <span className="dialog__muted skill-installs__folder">
                  {install.folder}
                </span>
              </div>
              <span
                className={`provider-status provider-status--${STATUS_TONE[install.status]}`}
              >
                {!install.detected && install.status === "notInstalled"
                  ? t("Not found")
                  : t(STATUS_LABEL[install.status])}
              </span>
              <div className="skill-installs__actions">
                {canInstall ? (
                  <button
                    type="button"
                    className="icon-button provider-action"
                    disabled={busy !== null}
                    aria-label={`${install.status === "updateAvailable" ? t("Update") : t("Install")} · ${install.label}`}
                    title={`${install.status === "updateAvailable" ? t("Update") : t("Install")} · ${install.label}`}
                    onClick={() => void act(install, "install", false)}
                  >
                    <Icon
                      name={install.status === "updateAvailable" ? "refresh" : "download"}
                      spin={rowBusy && busy !== "all"}
                    />
                  </button>
                ) : null}
                {install.status === "modified" ? (
                  <>
                    <button
                      type="button"
                      className="icon-button provider-action"
                      disabled={busy !== null}
                      aria-label={`${t("Replace with Sagar's version")} · ${install.label}`}
                      title={`${t("Replace with Sagar's version")} · ${install.label}`}
                      onClick={() =>
                        setConfirm({ providerId: install.providerId, action: "install" })
                      }
                    >
                      <Icon name="refresh" />
                    </button>
                    <button
                      type="button"
                      className="icon-button provider-action"
                      disabled={busy !== null}
                      aria-label={`${t("Uninstall")} · ${install.label}`}
                      title={`${t("Uninstall")} · ${install.label}`}
                      onClick={() =>
                        setConfirm({
                          providerId: install.providerId,
                          action: "uninstall",
                        })
                      }
                    >
                      <Icon name="trash" />
                    </button>
                  </>
                ) : null}
                {canRemove ? (
                  <button
                    type="button"
                    className="icon-button provider-action"
                    disabled={busy !== null}
                    aria-label={`${t("Uninstall")} · ${install.label}`}
                    title={`${t("Uninstall")} · ${install.label}`}
                    onClick={() => void act(install, "uninstall", false)}
                  >
                    <Icon name="trash" />
                  </button>
                ) : null}
              </div>
              {install.status === "conflict" ? (
                <p className="dialog__muted skill-installs__note">
                  {t(
                    "A folder with this name is already there and wasn't installed by Sagar, so Sagar won't change it.",
                  )}
                </p>
              ) : null}
              {asking ? (
                <div className="provider-row__confirm skill-installs__note">
                  <p className="provider-row__confirm-label">
                    {asking.action === "install"
                      ? t(
                          "Replace the changed copy with Sagar's version? Changes made outside Sagar will be lost.",
                        )
                      : t(
                          "Uninstall the changed copy? Changes made outside Sagar will be lost.",
                        )}
                  </p>
                  <div className="button-row">
                    <button
                      type="button"
                      className="btn btn--small"
                      onClick={() => setConfirm(null)}
                    >
                      <Icon name="close" />
                      {t("Cancel")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--small btn--danger"
                      onClick={() => void act(install, asking.action, true)}
                    >
                      <Icon name={asking.action === "install" ? "refresh" : "trash"} />
                      {asking.action === "install" ? t("Replace") : t("Uninstall")}
                    </button>
                  </div>
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
      {opencodeTwice ? (
        <p className="dialog__muted skill-installs__help">
          {t(
            "OpenCode also reads the Claude Code and Codex skill folders, so this skill may appear twice in OpenCode.",
          )}
        </p>
      ) : null}
    </div>
  );
}
