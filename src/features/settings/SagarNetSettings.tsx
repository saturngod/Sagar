import { invoke } from "@tauri-apps/api/core";
import { type CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import logo from "../../assets/sagarnet-logo.png";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import { sagarnetRequest } from "../../lib/sagarnet/link";
import { toCommandError } from "../../lib/tauri/errors";

type Availability = { available: boolean; host: string | null };
type Account = { id: number; email: string };
type Action = "check" | "signIn" | "signOut";

/** Settings → SagarNet: shows the signed-in account and signs in or out. */
export function SagarNetSettings({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  const [availability, setAvailability] = useState<Availability | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState<Action>("check");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const mounted = useRef(true);
  const pending = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(async (kind: Action, operation: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setAction(kind);
    setError(null);
    setNotice(null);
    try {
      await operation();
    } catch (cause) {
      if (mounted.current) {
        const problem = toCommandError(cause);
        if (problem.code === "sharing_unauthorized") setAccount(null);
        setError(problem.message);
      }
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  }, []);

  const check = useCallback(
    () =>
      run("check", async () => {
        const current = await invoke<Availability>("sharing_availability");
        if (!mounted.current) return;
        setAvailability(current);
        if (!current.available) {
          setAccount(null);
          setChecked(true);
          return;
        }
        const signedIn = await sagarnetRequest<Account | null>("account");
        if (!mounted.current) return;
        setAccount(signedIn);
        setChecked(true);
      }),
    [run],
  );

  useEffect(() => {
    if (enabled) void check();
  }, [enabled, check]);

  const signIn = () =>
    run("signIn", async () => {
      const signedIn = await invoke<Account>("sharing_sign_in");
      if (!mounted.current) return;
      setAccount(signedIn);
      setNotice(t("Signed in to SagarNet."));
    });

  const signOut = () =>
    run("signOut", async () => {
      await sagarnetRequest("disconnect");
      if (!mounted.current) return;
      setAccount(null);
      setNotice(t("Signed out of SagarNet."));
    });

  const cancelSignIn = () => {
    void invoke("sharing_cancel_sign_in").catch((cause: unknown) => {
      if (mounted.current) setError(toCommandError(cause).message);
    });
  };

  const unavailable = availability?.available === false;
  const state = busy ? "busy" : account ? "running" : "stopped";
  const stateLabel = busy
    ? action === "signIn"
      ? "Signing in…"
      : action === "signOut"
        ? "Signing out…"
        : "Checking…"
    : unavailable
      ? "Unavailable"
      : account
        ? "Signed in"
        : checked
          ? "Not signed in"
          : "Unknown";

  return (
    <section
      className="provider-section content-server-settings sagarnet-settings"
      aria-labelledby="sagarnet-settings-heading"
      aria-busy={busy}
    >
      <div className="provider-section__header">
        <div className="sagarnet-settings__brand">
          <span
            className="sagarnet-settings__logo"
            aria-hidden="true"
            style={{ "--sagarnet-logo": `url(${logo})` } as CSSProperties}
          />
          <h3 className="panel__heading" id="sagarnet-settings-heading">
            SagarNet
          </h3>
        </div>
        <div className="content-server-settings__state">
          <span
            className={`content-server-badge content-server-badge--${state}`}
            role="status"
          >
            <span className="content-server-badge__dot" aria-hidden="true" />
            {t(stateLabel)}
          </span>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Check account")}
            title={t("Check account")}
            disabled={busy}
            onClick={() => void check()}
          >
            <Icon name="refresh" spin={busy && action === "check"} />
          </button>
        </div>
      </div>
      <p className="content-server-settings__text dialog__muted">
        {t(
          "Sign in to share translated chapters with SagarNet. Sagar keeps your sign-in on this computer.",
        )}
      </p>

      {unavailable ? (
        <StatusNotice kind="info">
          {t("SagarNet isn’t configured in this build of Sagar.")}
        </StatusNotice>
      ) : (
        <div className="sharing-account">
          <Icon name="user" size={16} />
          <span className="sharing-account__text">
            {account ? (
              <>
                {t("Signed in as")} <strong>{account.email}</strong>
              </>
            ) : checked ? (
              t("Not signed in")
            ) : (
              t("Checking account…")
            )}
            {availability?.host ? (
              <span className="dialog__muted"> · {availability.host}</span>
            ) : null}
          </span>
        </div>
      )}

      {busy && action === "signIn" ? (
        <StatusNotice kind="loading">
          {t("Continue in your browser to sign in to SagarNet…")}
        </StatusNotice>
      ) : null}
      {error ? (
        <StatusNotice
          kind="error"
          onRetry={() => void check()}
          retryLabel="Check account"
        >
          {error}
        </StatusNotice>
      ) : null}
      {notice && !busy ? <StatusNotice kind="success">{notice}</StatusNotice> : null}

      {!unavailable ? (
        <div className="content-server-settings__actions">
          {busy && action === "signIn" ? (
            <button type="button" className="btn" onClick={cancelSignIn}>
              <Icon name="stop" />
              <span>{t("Cancel sign-in")}</span>
            </button>
          ) : account ? (
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={() => void signOut()}
            >
              <Icon name="unlink" />
              <span>{t(busy && action === "signOut" ? "Signing out…" : "Sign out")}</span>
            </button>
          ) : (
            <button
              type="button"
              className="btn btn--primary"
              disabled={busy || !checked}
              onClick={() => void signIn()}
            >
              <Icon name="link" />
              <span>{t("Sign in with browser")}</span>
            </button>
          )}
          <p className="content-server-settings__text dialog__muted">
            {t(
              account
                ? "Signing out removes the saved sign-in from this computer."
                : "Your browser opens so you can sign in or create an account.",
            )}
          </p>
        </div>
      ) : null}
    </section>
  );
}
