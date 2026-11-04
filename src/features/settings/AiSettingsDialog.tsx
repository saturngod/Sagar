import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  checkAiCli,
  deleteAiCustomProvider,
  listAiProviders,
  saveAiCustomProvider,
  testAiCustomProvider,
} from "../../lib/tauri/ai";
import { CommandError } from "../../lib/tauri/errors";
import type {
  AiProviderList,
  ApiProtocol,
  CliCheckResult,
  CustomProviderSummary,
  CustomTestResult,
} from "../../types/ai";
import { type AppearancePreferences, AppearanceSettings } from "./AppearanceSettings";
import { ContentServerSettings } from "./ContentServerSettings";
import { TranslationModelSection } from "./TranslationModelSection";

type AiSettingsDialogProps = {
  appearance: AppearancePreferences;
  onClose: () => void;
};

type CustomFormState = {
  protocol: ApiProtocol;
  editingId: string | null;
  name: string;
  baseUrl: string;
  model: string;
  apiKey: string;
  fieldErrors: Record<string, string>;
  saving: boolean;
};

const emptyForm: CustomFormState = {
  protocol: "openai",
  editingId: null,
  name: "",
  baseUrl: "",
  model: "",
  apiKey: "",
  fieldErrors: {},
  saving: false,
};

/**
 * AI provider settings dialog (settings-only slice): CLI availability for
 * Codex / OpenCode / Claude Code plus user-added API-compatible
 * endpoints. Nothing here sends book text anywhere.
 */
export function AiSettingsDialog({ onClose, appearance }: AiSettingsDialogProps) {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<AiProviderList | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [checking, setChecking] = useState<Record<string, boolean>>({});
  const [checkResults, setCheckResults] = useState<Record<string, CliCheckResult>>({});
  const [testing, setTesting] = useState<Record<string, boolean>>({});
  const [testResults, setTestResults] = useState<Record<string, CustomTestResult>>({});
  const [form, setForm] = useState<CustomFormState>(emptyForm);
  const [formOpen, setFormOpen] = useState(false);
  const [tab, setTab] = useState<"appearance" | "providers" | "translation" | "server">(
    "appearance",
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [busyDelete, setBusyDelete] = useState(false);
  const [expandedProvider, setExpandedProvider] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const formRef = useRef<HTMLElement | null>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => {
      previousFocus.current?.focus();
    };
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: switching the edited provider should focus the form even when it is already open.
  useEffect(() => {
    if (formOpen) {
      formRef.current?.scrollIntoView({ block: "nearest" });
      formRef.current?.querySelector<HTMLInputElement>("input")?.focus();
    }
  }, [formOpen, form.editingId]);

  const loadProviders = useCallback(() => {
    setLoadError(null);
    listAiProviders()
      .then(setProviders)
      .catch((error: unknown) => {
        setLoadError(
          error instanceof CommandError
            ? error.message
            : "AI settings could not be loaded.",
        );
      });
  }, []);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
      if (event.key === "Tab") {
        const controls = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]',
          ) ?? [],
        ).filter(
          (control) =>
            !control.closest("[hidden]") && control.getClientRects().length > 0,
        );
        const first = controls[0];
        const last = controls[controls.length - 1];
        if (
          event.shiftKey &&
          (document.activeElement === first ||
            document.activeElement === dialogRef.current)
        ) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  async function handleCheck(providerId: string) {
    setChecking((current) => ({ ...current, [providerId]: true }));
    setActionError(null);
    try {
      const result = await checkAiCli(providerId);
      setCheckResults((current) => ({ ...current, [providerId]: result }));
      loadProviders();
    } catch (error) {
      setActionError(error instanceof CommandError ? error.message : "The check failed.");
    } finally {
      setChecking((current) => ({ ...current, [providerId]: false }));
    }
  }

  function openAddForm() {
    setForm(emptyForm);
    setFormOpen(true);
    setActionError(null);
  }

  function openEditForm(provider: CustomProviderSummary) {
    setForm({
      protocol: provider.protocol ?? "openai",
      editingId: provider.id,
      name: provider.name,
      baseUrl: provider.baseUrl,
      model: provider.model,
      apiKey: "",
      fieldErrors: {},
      saving: false,
    });
    setFormOpen(true);
    setActionError(null);
  }

  function fieldErrorFor(code: string): [string, string] | null {
    switch (code) {
      case "invalid_provider_name":
        return ["name", "Enter a provider name of 1–64 characters."];
      case "invalid_base_url":
        return ["baseUrl", "Use a full http(s) URL such as https://api.example.com/v1."];
      case "invalid_api_key":
        return ["apiKey", "Enter an API key."];
      case "invalid_model":
        return ["model", "Enter a model name of 1–128 characters."];
      default:
        return null;
    }
  }

  async function handleSaveForm() {
    setActionError(null);
    setForm((current) => ({ ...current, fieldErrors: {}, saving: true }));
    try {
      await saveAiCustomProvider({
        id: form.editingId ?? undefined,
        name: form.name,
        protocol: form.protocol,
        baseUrl: form.baseUrl,
        apiKey: form.apiKey === "" ? null : form.apiKey,
        model: form.model,
      });
      setForm(emptyForm);
      setFormOpen(false);
      loadProviders();
    } catch (error) {
      const mapped = error instanceof CommandError ? fieldErrorFor(error.code) : null;
      if (mapped) {
        const [field, message] = mapped;
        setForm((current) => ({
          ...current,
          saving: false,
          fieldErrors: { [field]: message },
        }));
      } else {
        setForm((current) => ({ ...current, saving: false }));
        setActionError(
          error instanceof CommandError
            ? error.message
            : "The provider could not be saved.",
        );
      }
    }
  }

  async function handleDelete(id: string) {
    if (confirmDelete !== id) {
      setConfirmDelete(id);
      return;
    }
    setBusyDelete(true);
    setActionError(null);
    try {
      await deleteAiCustomProvider(id);
      setConfirmDelete(null);
      setTestResults((current) => {
        const next = { ...current };
        delete next[id];
        return next;
      });
      if (form.editingId === id) {
        setForm(emptyForm);
        setFormOpen(false);
      }
      loadProviders();
    } catch (error) {
      setActionError(
        error instanceof CommandError
          ? error.message
          : "The provider could not be removed.",
      );
    } finally {
      setBusyDelete(false);
    }
  }

  async function handleTest(id: string) {
    setTesting((current) => ({ ...current, [id]: true }));
    setActionError(null);
    try {
      const result = await testAiCustomProvider(id);
      setTestResults((current) => ({ ...current, [id]: result }));
    } catch (error) {
      setActionError(
        error instanceof CommandError ? error.message : "The connection test failed.",
      );
    } finally {
      setTesting((current) => ({ ...current, [id]: false }));
    }
  }

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="ai-settings-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <div className="panel__header">
          <h2 className="panel__heading" id="ai-settings-title">
            {t("Settings")}
          </h2>
          <button
            type="button"
            className="icon-button provider-action"
            aria-label={t("Close settings")}
            title={t("Close")}
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        </div>

        <div
          className="tabs settings-tabs"
          role="tablist"
          aria-label={t("Settings sections")}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
              return;
            }
            event.preventDefault();
            const tabs = ["appearance", "providers", "translation", "server"] as const;
            const direction = event.key === "ArrowRight" ? 1 : -1;
            const next =
              tabs[(tabs.indexOf(tab) + direction + tabs.length) % tabs.length];
            setTab(next);
            document.getElementById(`ai-tab-${next}`)?.focus();
          }}
        >
          <button
            type="button"
            role="tab"
            id="ai-tab-appearance"
            aria-selected={tab === "appearance"}
            aria-controls="ai-panel-appearance"
            tabIndex={tab === "appearance" ? 0 : -1}
            className="tab"
            onClick={() => setTab("appearance")}
          >
            <Icon name="sun" />
            {t("Appearance")}
          </button>
          <button
            type="button"
            role="tab"
            id="ai-tab-providers"
            aria-selected={tab === "providers"}
            aria-controls="ai-panel-providers"
            tabIndex={tab === "providers" ? 0 : -1}
            className="tab"
            onClick={() => setTab("providers")}
          >
            <Icon name="sparkles" />
            {t("Providers")}
          </button>
          <button
            type="button"
            role="tab"
            id="ai-tab-translation"
            aria-selected={tab === "translation"}
            aria-controls="ai-panel-translation"
            tabIndex={tab === "translation" ? 0 : -1}
            className="tab"
            onClick={() => setTab("translation")}
          >
            <Icon name="sparkles" />
            {t("Translation Model")}
          </button>
          <button
            type="button"
            role="tab"
            id="ai-tab-server"
            aria-selected={tab === "server"}
            aria-controls="ai-panel-server"
            tabIndex={tab === "server" ? 0 : -1}
            className="tab"
            onClick={() => setTab("server")}
          >
            <Icon name="server" />
            {t("Content server")}
          </button>
        </div>

        <div
          role="tabpanel"
          id="ai-panel-server"
          aria-labelledby="ai-tab-server"
          hidden={tab !== "server"}
        >
          <ContentServerSettings enabled={tab === "server"} />
        </div>

        <div
          role="tabpanel"
          id="ai-panel-appearance"
          aria-labelledby="ai-tab-appearance"
          hidden={tab !== "appearance"}
        >
          <AppearanceSettings {...appearance} />
        </div>

        {(tab === "providers" || tab === "translation") && loadError ? (
          <StatusNotice kind="error" onRetry={loadProviders}>
            {loadError}
          </StatusNotice>
        ) : null}
        {(tab === "providers" || tab === "translation") && !loadError && !providers ? (
          <StatusNotice kind="loading">{t("Loading AI providers…")}</StatusNotice>
        ) : null}

        {providers ? (
          <>
            <div
              role="tabpanel"
              id="ai-panel-providers"
              aria-labelledby="ai-tab-providers"
              hidden={tab !== "providers"}
            >
              <section className="provider-section" aria-labelledby="ai-cli-heading">
                <h3 className="panel__heading" id="ai-cli-heading">
                  {t("Coding assistants")}
                </h3>
                <ul className="provider-list">
                  {providers.cli.map((provider) => {
                    const result = checkResults[provider.id];
                    const isChecking = checking[provider.id] ?? false;
                    return (
                      <li key={provider.id}>
                        <div className="provider-row">
                          <div className="provider-row__info">
                            <strong>{provider.label}</strong>
                            {result?.version ? (
                              <span className="dialog__muted">{result.version}</span>
                            ) : null}
                          </div>
                          <span
                            className={`chapter-export-status chapter-export-status--${result?.connected ? "reviewed" : "original"}`}
                            role="status"
                          >
                            {isChecking
                              ? t("Checking…")
                              : result?.connected
                                ? t("Connected")
                                : result
                                  ? t("Failed")
                                  : provider.available
                                    ? t("Available")
                                    : t("Not found")}
                          </span>
                          <button
                            type="button"
                            className="icon-button provider-action"
                            disabled={isChecking}
                            aria-label={`${t("Check")} ${provider.label}`}
                            title={`${t("Check")} ${provider.label}`}
                            onClick={() => void handleCheck(provider.id)}
                          >
                            <Icon name="refresh" spin={isChecking} />
                          </button>
                        </div>
                        {result && !result.connected ? (
                          <p className="provider-row__feedback" role="alert">
                            {result.message}
                          </p>
                        ) : null}
                      </li>
                    );
                  })}
                </ul>
              </section>

              <section className="provider-section" aria-labelledby="ai-custom-heading">
                <div className="provider-section__header">
                  <h3 className="panel__heading" id="ai-custom-heading">
                    {t("Custom providers")}
                  </h3>
                  {!formOpen ? (
                    <button
                      type="button"
                      className="btn btn--small"
                      onClick={openAddForm}
                    >
                      <Icon name="plus" />
                      {t("Add provider")}
                    </button>
                  ) : null}
                </div>
                {providers.custom.length === 0 ? (
                  <p className="dialog__muted">{t("No custom providers yet.")}</p>
                ) : (
                  <ul className="provider-list">
                    {providers.custom.map((provider) => {
                      const result = testResults[provider.id];
                      const isTesting = testing[provider.id] ?? false;
                      const expanded = expandedProvider === provider.id;
                      const protocol =
                        provider.protocol === "gemini"
                          ? "Gemini"
                          : provider.protocol === "anthropic"
                            ? "Anthropic (Claude)"
                            : "OpenAI";
                      return (
                        <li key={provider.id}>
                          <div className="provider-row">
                            <div className="provider-row__info">
                              <strong>{provider.name}</strong>
                              <span className="dialog__muted">
                                {protocol} · {provider.model}
                              </span>
                            </div>
                            <span
                              className={`chapter-export-status chapter-export-status--${result?.ok ? "reviewed" : "original"}`}
                              role="status"
                            >
                              {isTesting
                                ? t("Testing…")
                                : result
                                  ? result.ok
                                    ? t("Connected")
                                    : t("Failed")
                                  : t("Saved")}
                            </span>
                            <div className="provider-row__actions">
                              <button
                                type="button"
                                className="icon-button provider-action"
                                disabled={isTesting}
                                aria-label={`${t("Test")} ${provider.name}`}
                                title={`${t("Test")} ${provider.name}`}
                                onClick={() => void handleTest(provider.id)}
                              >
                                <Icon name="refresh" spin={isTesting} />
                              </button>
                              <button
                                type="button"
                                className="icon-button provider-action"
                                aria-label={`${t("Edit")} ${provider.name}`}
                                title={`${t("Edit")} ${provider.name}`}
                                onClick={() => openEditForm(provider)}
                              >
                                <Icon name="edit" />
                              </button>
                              <button
                                type="button"
                                className="icon-button provider-action"
                                aria-expanded={expanded}
                                aria-controls={`provider-details-${provider.id}`}
                                aria-label={`${t("Connection details")} · ${provider.name}`}
                                title={t("Connection details")}
                                onClick={() =>
                                  setExpandedProvider(expanded ? null : provider.id)
                                }
                              >
                                <Icon name={expanded ? "minus" : "chevronDown"} />
                              </button>
                              <button
                                type="button"
                                className="icon-button provider-action"
                                disabled={busyDelete}
                                aria-label={`${t("Delete")} ${provider.name}`}
                                title={`${t("Delete")} ${provider.name}`}
                                onClick={() => void handleDelete(provider.id)}
                              >
                                <Icon name="trash" />
                              </button>
                            </div>
                          </div>
                          <div
                            id={`provider-details-${provider.id}`}
                            className="provider-row__details"
                            hidden={!expanded}
                          >
                            <dl>
                              <dt>{t("Base URL")}</dt>
                              <dd>{provider.baseUrl}</dd>
                              <dt>{t("API key")}</dt>
                              <dd>{provider.hasApiKey ? t("key saved") : t("no key")}</dd>
                            </dl>
                          </div>
                          {result ? (
                            <p
                              className="provider-row__feedback"
                              role={result.ok ? "status" : "alert"}
                            >
                              {result.message}
                            </p>
                          ) : null}
                          {confirmDelete === provider.id ? (
                            <div className="provider-row__confirm">
                              <p className="provider-row__confirm-label">
                                {t("Delete provider?")} <strong>{provider.name}</strong>
                              </p>
                              <div className="button-row">
                                <button
                                  type="button"
                                  className="btn"
                                  disabled={busyDelete}
                                  onClick={() => setConfirmDelete(null)}
                                >
                                  <Icon name="close" />
                                  {t("Cancel")}
                                </button>
                                <button
                                  type="button"
                                  className="btn btn--danger"
                                  disabled={busyDelete}
                                  onClick={() => void handleDelete(provider.id)}
                                >
                                  <Icon name="trash" />
                                  {busyDelete ? t("Deleting…") : t("Confirm delete")}
                                </button>
                              </div>
                            </div>
                          ) : null}
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
              <details className="settings-help">
                <summary>
                  <Icon name="keep" />
                  {t("Connection help")}
                </summary>
                <p className="dialog__muted">
                  {t(
                    "Detected from tools installed on this computer. Select Check to verify a tool responds.",
                  )}
                </p>
                <p className="dialog__muted">
                  {t(
                    "OpenAI, Gemini and Anthropic-compatible endpoints: a base URL, an API key and a model. Test requests only the model list.",
                  )}
                </p>
                <p className="dialog__muted">
                  {t(
                    "API keys stay on this computer in the app database and are never shown again. Settings never send book text; a connection test only requests the provider's model list.",
                  )}
                </p>
              </details>

              {formOpen ? (
                <section
                  ref={formRef}
                  aria-label={form.editingId ? t("Edit provider") : t("Add provider")}
                >
                  <label className="form-field">
                    <span>{t("API format")}</span>
                    <select
                      value={form.protocol}
                      disabled={form.saving}
                      onChange={(event) => {
                        const protocol = event.target.value as ApiProtocol;
                        const defaults = {
                          openai: "https://api.openai.com/v1",
                          gemini: "https://generativelanguage.googleapis.com/v1beta",
                          anthropic: "https://api.anthropic.com/v1",
                        };
                        setForm((current) => ({
                          ...current,
                          protocol,
                          baseUrl:
                            !current.baseUrl ||
                            Object.values(defaults).includes(current.baseUrl)
                              ? defaults[protocol]
                              : current.baseUrl,
                        }));
                      }}
                    >
                      <option value="openai">{t("OpenAI Compatible")}</option>
                      <option value="gemini">{t("Gemini API Compatible")}</option>
                      <option value="anthropic">
                        {t("Anthropic (Claude) API Compatible")}
                      </option>
                    </select>
                  </label>
                  <details className="settings-help">
                    <summary>
                      <Icon name="code" />
                      {t("Connection details")}
                    </summary>
                    <p className="dialog__muted">
                      {form.protocol === "gemini"
                        ? "Uses Gemini Interactions without saved conversation state. Use https://generativelanguage.googleapis.com/v1beta or the full /v1beta/interactions endpoint."
                        : form.protocol === "anthropic"
                          ? "Uses Anthropic Messages with your API key. Accepts a base URL or full /v1/messages endpoint, including compatible services such as https://api.deepseek.com/anthropic. No CLI installation is needed."
                          : "Uses OpenAI Chat Completions. Include the API version path in the base URL."}
                    </p>
                  </details>
                  <div className="form-grid">
                    <label className="form-field">
                      <span>{t("Name")}</span>
                      <input
                        type="text"
                        value={form.name}
                        maxLength={64}
                        onChange={(event) =>
                          setForm((current) => ({ ...current, name: event.target.value }))
                        }
                        aria-invalid={form.fieldErrors.name !== undefined}
                      />
                      {form.fieldErrors.name ? (
                        <span className="form-field__error">{form.fieldErrors.name}</span>
                      ) : null}
                    </label>
                    <label className="form-field">
                      <span>{t("Model")}</span>
                      <input
                        type="text"
                        value={form.model}
                        maxLength={128}
                        placeholder="gpt-4o-mini"
                        onChange={(event) =>
                          setForm((current) => ({
                            ...current,
                            model: event.target.value,
                          }))
                        }
                        aria-invalid={form.fieldErrors.model !== undefined}
                      />
                      {form.fieldErrors.model ? (
                        <span className="form-field__error">
                          {form.fieldErrors.model}
                        </span>
                      ) : null}
                    </label>
                  </div>
                  <label className="form-field">
                    <span>{t("Base URL")}</span>
                    <input
                      type="url"
                      value={form.baseUrl}
                      placeholder="https://api.example.com/v1"
                      onChange={(event) =>
                        setForm((current) => ({
                          ...current,
                          baseUrl: event.target.value,
                        }))
                      }
                      aria-invalid={form.fieldErrors.baseUrl !== undefined}
                    />
                    {form.fieldErrors.baseUrl ? (
                      <span className="form-field__error">
                        {form.fieldErrors.baseUrl}
                      </span>
                    ) : null}
                  </label>
                  <label className="form-field">
                    <span>
                      {t("API key")}
                      {form.editingId ? t(" (leave blank to keep the saved key)") : ""}
                    </span>
                    <input
                      type="password"
                      value={form.apiKey}
                      autoComplete="off"
                      onChange={(event) =>
                        setForm((current) => ({ ...current, apiKey: event.target.value }))
                      }
                      aria-invalid={form.fieldErrors.apiKey !== undefined}
                    />
                    {form.fieldErrors.apiKey ? (
                      <span className="form-field__error">{form.fieldErrors.apiKey}</span>
                    ) : null}
                  </label>
                  <div className="button-row dialog__actions">
                    <button
                      type="button"
                      className="btn"
                      disabled={form.saving}
                      onClick={() => {
                        setForm(emptyForm);
                        setFormOpen(false);
                      }}
                    >
                      <Icon name="close" />
                      {t("Cancel")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={form.saving}
                      onClick={() => void handleSaveForm()}
                    >
                      <Icon name="save" />
                      {form.saving ? t("Saving…") : t("Save")}
                    </button>
                  </div>
                </section>
              ) : null}

              {actionError ? (
                <StatusNotice kind="error">{actionError}</StatusNotice>
              ) : null}
            </div>
            <div
              role="tabpanel"
              id="ai-panel-translation"
              aria-labelledby="ai-tab-translation"
              hidden={tab !== "translation"}
            >
              {tab === "translation" ? (
                <TranslationModelSection cli={providers.cli} custom={providers.custom} />
              ) : null}
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
