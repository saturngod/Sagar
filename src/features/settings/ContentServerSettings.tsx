import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  type ContentServerStatus,
  contentServerStatus,
  startContentServer,
  stopContentServer,
} from "../../lib/tauri/contentServer";
import { CommandError } from "../../lib/tauri/errors";

export function ContentServerSettings({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ContentServerStatus | null>(null);
  const [port, setPort] = useState("8080");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState("refresh");
  const [error, setError] = useState<string | null>(null);
  const [validation, setValidation] = useState<string | null>(null);
  const [invalidField, setInvalidField] = useState<"port" | "credentials" | null>(null);
  const mounted = useRef(false);
  const actionButton = useRef<HTMLButtonElement>(null);
  const focusAfterAction = useRef(false);
  const pending = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(
    async (kind: string, operation: () => Promise<ContentServerStatus>) => {
      if (pending.current) return;
      pending.current = true;
      setBusy(true);
      setAction(kind);
      setError(null);
      try {
        const next = await operation();
        if (mounted.current) {
          setStatus(next);
          focusAfterAction.current = kind === "start" || kind === "stop";
          if (next.running && next.port !== null) setPort(String(next.port));
        }
      } catch (cause) {
        if (mounted.current)
          setError(
            cause instanceof CommandError
              ? cause.message
              : "Could not update the content server. Check its status and retry.",
          );
      } finally {
        pending.current = false;
        if (mounted.current) setBusy(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (enabled) void run("refresh", contentServerStatus);
  }, [enabled, run]);

  useEffect(() => {
    if (status && !busy && focusAfterAction.current) {
      focusAfterAction.current = false;
      if (enabled) actionButton.current?.focus();
    }
  }, [busy, status, enabled]);

  function start() {
    if (!/^\d+$/.test(port) || Number(port) > 65535) {
      setValidation("Enter a whole-number port from 0 to 65535.");
      setInvalidField("port");
      document.getElementById("content-server-port")?.focus();
      return;
    }
    if (
      (username.length === 0) !== (password.length === 0) ||
      username.includes(":") ||
      Array.from(username + password).some((character) => {
        const code = character.charCodeAt(0);
        return code <= 31 || (code >= 127 && code <= 159);
      })
    ) {
      setInvalidField("credentials");
      document
        .getElementById(
          username.length === 0 || username.includes(":")
            ? "content-server-username"
            : "content-server-password",
        )
        ?.focus();
      setValidation(
        "Enter both username and password, or leave both empty. The username cannot contain a colon, and credentials cannot contain control characters.",
      );
      return;
    }
    setValidation(null);
    setInvalidField(null);
    void run("start", () =>
      startContentServer({ port: Number(port), username, password }),
    );
  }

  const locked = busy || status === null || status.running;
  const state = busy ? "busy" : status === null ? "unknown" : status.running ? "running" : "stopped";
  const stateLabel = busy
    ? action === "start"
      ? "Starting…"
      : action === "stop"
        ? "Stopping…"
        : "Checking…"
    : status === null
      ? "Unknown"
      : status.running
        ? "Running"
        : "Stopped";
  const clearValidation = () => {
    setValidation(null);
    setInvalidField(null);
  };
  return (
    <section
      className="provider-section content-server-settings"
      aria-labelledby="content-server-heading"
      aria-busy={busy}
    >
      <div className="provider-section__header">
        <h3 className="panel__heading" id="content-server-heading">
          {t("Content server")}
        </h3>
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
            aria-label={t("Check status")}
            title={t("Check status")}
            disabled={busy}
            onClick={() => void run("refresh", contentServerStatus)}
          >
            <Icon name="refresh" spin={busy && action === "refresh"} />
          </button>
        </div>
      </div>
      <p className="dialog__muted">
        {t("Share your library and saved translations with Sagarwar on the same network.")}
      </p>
      {error ? (
        <StatusNotice
          kind="error"
          onRetry={() => void run("refresh", contentServerStatus)}
          retryLabel="Check status"
        >
          {error}
        </StatusNotice>
      ) : null}
      {status?.running ? (
        <div className="content-server-settings__connection">
          <p className="content-server-settings__label">{t("Connect from Sagarwar")}</p>
          {status.urls.length > 0 ? (
            <ul>
              {status.urls.map((url) => (
                <li key={url}>
                  <code>{url}</code>
                </li>
              ))}
            </ul>
          ) : (
            <p>
              {t("No network address found. Use this computer’s IP address with port")}{" "}
              <strong>{status.port}</strong>.
            </p>
          )}
          <p className="dialog__muted">
            {t("This computer:")} <code>http://127.0.0.1:{status.port}</code>
          </p>
          <p className="dialog__muted">
            {t(status.authenticationRequired ? "Login required." : "No login required.")}{" "}
            {t("If Sagarwar can’t connect, allow Sagar through the firewall.")}
          </p>
        </div>
      ) : null}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!locked) start();
        }}
      >
        <div hidden={status?.running}>
          <label className="form-field settings-row" htmlFor="content-server-port">
            <span className="settings-label content-server-settings__field-label">
              <span>{t("Port")}</span>
              <small className="dialog__muted" id="content-server-port-help">
                {t("0 picks a free port")}
              </small>
            </span>
            <input
              id="content-server-port"
              inputMode="numeric"
              value={port}
              disabled={locked}
              aria-invalid={invalidField === "port"}
              aria-describedby={
                invalidField === "port"
                  ? "content-server-port-help content-server-validation"
                  : "content-server-port-help"
              }
              onChange={(event) => {
                setPort(event.target.value);
                clearValidation();
              }}
            />
          </label>
          <label className="form-field settings-row" htmlFor="content-server-username">
            <span className="settings-label">{t("Username (optional)")}</span>
            <input
              id="content-server-username"
              aria-invalid={invalidField === "credentials"}
              aria-describedby={
                invalidField === "credentials"
                  ? "content-server-login-help content-server-validation"
                  : "content-server-login-help"
              }
              autoComplete="off"
              maxLength={256}
              value={username}
              disabled={locked}
              onChange={(event) => {
                setUsername(event.target.value);
                clearValidation();
              }}
            />
          </label>
          <label className="form-field settings-row" htmlFor="content-server-password">
            <span className="settings-label">{t("Password (optional)")}</span>
            <input
              id="content-server-password"
              type="password"
              aria-invalid={invalidField === "credentials"}
              aria-describedby={
                invalidField === "credentials"
                  ? "content-server-login-help content-server-validation"
                  : "content-server-login-help"
              }
              autoComplete="new-password"
              maxLength={1024}
              value={password}
              disabled={locked}
              onChange={(event) => {
                setPassword(event.target.value);
                clearValidation();
              }}
            />
          </label>
          <p className="dialog__muted" id="content-server-login-help">
            {t(
              "Without a login, anyone on this network can access your books. Connections aren’t encrypted, so use a trusted network.",
            )}
          </p>
        </div>
        {validation ? (
          <div id="content-server-validation">
            <StatusNotice kind="error">{validation}</StatusNotice>
          </div>
        ) : null}
        <div className="content-server-settings__actions">
          {status?.running ? (
            <button
              type="button"
              className="btn"
              disabled={busy}
              ref={actionButton}
              onClick={(event) => {
                // The same DOM button becomes a submit button after stopping.
                // Cancel its original click's default action before that update.
                event.preventDefault();
                void run("stop", stopContentServer);
              }}
            >
              <Icon name="stop" />
              <span>{t(busy && action === "stop" ? "Stopping…" : "Stop server")}</span>
            </button>
          ) : (
            <button
              ref={actionButton}
              type="submit"
              className="btn btn--primary"
              disabled={locked}
            >
              <Icon name="play" />
              <span>{t(busy && action === "start" ? "Starting…" : "Start server")}</span>
            </button>
          )}
          <p className="dialog__muted">
            {t(
              status?.running
                ? "Stop the server to change settings."
                : "Runs until Sagar quits. Login isn’t saved.",
            )}
          </p>
        </div>
      </form>
    </section>
  );
}
