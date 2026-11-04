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
  return (
    <section
      className="provider-section content-server-settings"
      aria-labelledby="content-server-heading"
      aria-busy={busy}
    >
      <h3 className="panel__heading" id="content-server-heading">
        {t("Content server")}
      </h3>
      <p className="dialog__muted">
        {t(
          "Share your Sagar library, covers and saved translations with Sagarwar on the same network. Existing translations are never replaced.",
        )}
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
      {busy ? (
        <StatusNotice kind="loading">
          {t(
            action === "start"
              ? "Starting content server…"
              : action === "stop"
                ? "Stopping content server…"
                : "Checking content server…",
          )}
        </StatusNotice>
      ) : null}
      {status && !busy ? (
        <StatusNotice kind={status.running ? "success" : "info"}>
          {t(
            status.running ? "Content server is running." : "Content server is stopped.",
          )}
        </StatusNotice>
      ) : null}
      {status?.running ? (
        <div className="content-server-settings__connection">
          <p>
            {t(status.authenticationRequired ? "Login required." : "No login required.")}
          </p>
          <p>
            {t(
              "In Sagarwar, connect using an address below. Both devices must be on the same network; firewall settings may need to allow Sagar.",
            )}
          </p>
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
              {t(
                "No local-network address was found. Use this computer’s IPv4 address with port",
              )}{" "}
              <strong>{status.port}</strong>.
            </p>
          )}
          <p className="dialog__muted">
            {t("On this computer only:")} <code>http://127.0.0.1:{status.port}</code>
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
            <span className="settings-label">{t("Port")}</span>
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
                setValidation(null);
                setInvalidField(null);
              }}
            />
          </label>
          <p className="dialog__muted" id="content-server-port-help">
            {t(
              "Use 0 to choose an available port. Stop the server before changing its settings.",
            )}
          </p>
          <label className="form-field settings-row" htmlFor="content-server-username">
            <span className="settings-label">{t("Username (optional)")}</span>
            <input
              id="content-server-username"
              aria-invalid={invalidField === "credentials"}
              aria-describedby={
                invalidField === "credentials" ? "content-server-validation" : undefined
              }
              autoComplete="off"
              maxLength={256}
              value={username}
              disabled={locked}
              onChange={(event) => {
                setUsername(event.target.value);
                setValidation(null);
                setInvalidField(null);
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
                invalidField === "credentials" ? "content-server-validation" : undefined
              }
              autoComplete="new-password"
              maxLength={1024}
              value={password}
              disabled={locked}
              onChange={(event) => {
                setPassword(event.target.value);
                setValidation(null);
                setInvalidField(null);
              }}
            />
          </label>
          <p className="dialog__muted">
            {t(
              "Leave both empty for access without a login. Anyone who can reach this server can then download books and fill missing translations. Use a trusted local network: HTTP does not encrypt passwords or book content.",
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
              {t(busy && action === "stop" ? "Stopping…" : "Stop server")}
            </button>
          ) : (
            <button
              ref={actionButton}
              type="submit"
              className="btn btn--primary"
              disabled={locked}
            >
              <Icon name="play" />
              {t(busy && action === "start" ? "Starting…" : "Start server")}
            </button>
          )}
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void run("refresh", contentServerStatus)}
          >
            <Icon name="refresh" />
            {t("Check status")}
          </button>
        </div>
        {status?.running ? (
          <p className="dialog__muted">
            {t(
              "Stop the server to change its settings. HTTP does not encrypt passwords or book content; use a trusted local network.",
            )}
          </p>
        ) : null}
        <p className="dialog__muted">
          {t(
            "The server stays on when you close Settings and stops when Sagar quits. Credentials are not saved for the next launch.",
          )}
        </p>
      </form>
    </section>
  );
}
