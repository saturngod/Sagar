import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  getTranslationSelection,
  listTranslationModels,
  setTranslationSelection,
} from "../../lib/tauri/ai";
import { CommandError } from "../../lib/tauri/errors";
import type {
  CliProviderStatus,
  CustomProviderSummary,
  TranslationModelList,
  TranslationSelection,
} from "../../types/ai";

type TranslationModelSectionProps = {
  cli: CliProviderStatus[];
  custom: CustomProviderSummary[];
};

export function agentLabel(
  agent: string,
  cli: CliProviderStatus[],
  custom: CustomProviderSummary[],
) {
  const builtin = cli.find((provider) => provider.id === agent);
  if (builtin) {
    return builtin.label;
  }
  if (agent.startsWith("custom:")) {
    const found = custom.find((provider) => `custom:${provider.id}` === agent);
    if (found) {
      return found.name;
    }
  }
  return agent;
}

/**
 * Translation Model picker: choose the agent (Codex / OpenCode / Claude Code
 * or a saved custom provider) and the model future translation will use.
 * Selecting only persists the choice; it never starts translation and never
 * sends book text anywhere.
 */
export function TranslationModelSection({ cli, custom }: TranslationModelSectionProps) {
  const { t } = useTranslation();
  const [agent, setAgent] = useState("codex");
  const [catalog, setCatalog] = useState<TranslationModelList | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [draft, setDraft] = useState("");
  const [effort, setEffort] = useState("");
  const [saved, setSaved] = useState<TranslationSelection | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  const [manualOpen, setManualOpen] = useState(false);
  const catalogRequest = useRef(0);

  const loadCatalog = useCallback((nextAgent: string, refresh = false) => {
    const request = ++catalogRequest.current;
    setLoading(true);
    setLoadError(null);
    listTranslationModels(nextAgent, refresh)
      .then((list) => {
        if (request !== catalogRequest.current) return;
        setCatalog(list);
        setSearch("");
      })
      .catch((error: unknown) => {
        if (request !== catalogRequest.current) return;
        setCatalog(null);
        setLoadError(
          error instanceof CommandError
            ? error.message
            : "The model list could not be loaded.",
        );
      })
      .finally(() => {
        if (request === catalogRequest.current) setLoading(false);
      });
  }, []);

  const loadSelection = useCallback(() => {
    getTranslationSelection()
      .then((selection) => {
        setSaved(selection);
        if (selection) {
          setAgent(selection.agent);
          setDraft(selection.model);
          setEffort(selection.reasoningEffort ?? "");
        }
      })
      .catch(() => {
        // A missing selection is not an error; the picker simply starts empty.
      });
  }, []);

  useEffect(() => {
    loadSelection();
  }, [loadSelection]);

  useEffect(() => {
    loadCatalog(agent);
  }, [agent, loadCatalog]);

  const filtered = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    const models = catalog?.models ?? [];
    if (query === "") {
      return models;
    }
    return models.filter(
      (model) =>
        model.id.toLocaleLowerCase().includes(query) ||
        model.label.toLocaleLowerCase().includes(query),
    );
  }, [catalog, search]);

  const selectedModel =
    catalog?.agent === agent
      ? catalog.models.find((model) => model.id === draft.trim())
      : undefined;
  const customProtocol =
    custom.find((provider) => `custom:${provider.id}` === agent)?.protocol ?? "openai";
  const effortOptions =
    agent.startsWith("custom:") && customProtocol !== "openai"
      ? []
      : agent.startsWith("custom:")
        ? ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
        : (selectedModel?.reasoningEfforts ??
          (agent === "codex"
            ? ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
            : agent === "agy"
              ? ["low", "medium", "high"]
              : []));
  const effortOrder = [
    "none",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "ultra",
  ];
  const orderedEfforts = [...effortOptions].sort((a, b) => {
    const rank = (value: string) => {
      const index = effortOrder.indexOf(value);
      return index === -1 ? effortOrder.length : index;
    };
    return rank(a) - rank(b) || a.localeCompare(b);
  });
  const effectiveEffort = effortOptions.includes(effort) ? effort : "";
  const effortLabel = (value: string) =>
    value === "xhigh" ? "XHigh" : value.charAt(0).toUpperCase() + value.slice(1);

  async function handleSave() {
    const model = draft.trim();
    if (model === "") {
      setSaveError("Choose a model from the list or enter one manually.");
      return;
    }
    setSaving(true);
    setSaveError(null);
    setSavedNotice(null);
    try {
      const selection = await setTranslationSelection(
        agent,
        model,
        effectiveEffort || null,
      );
      setSaved(selection);
      setSavedNotice("Saved");
    } catch (error) {
      setSaveError(
        error instanceof CommandError
          ? error.message
          : "The selection could not be saved.",
      );
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    if (!loading && catalog?.agent === agent && draft.trim() !== "" && !selectedModel) {
      setManualOpen(true);
    }
  }, [loading, catalog, agent, draft, selectedModel]);

  const hasChanges =
    draft.trim() !== "" &&
    (saved?.agent !== agent ||
      saved?.model !== draft.trim() ||
      (saved?.reasoningEffort ?? "") !== effectiveEffort);

  return (
    <section className="model-settings" aria-labelledby="ai-translation-model-heading">
      <h3 className="visually-hidden" id="ai-translation-model-heading">
        {t("Translation Model")}
      </h3>
      <div className="model-settings__agent">
        <label className="form-field settings-row">
          <span className="settings-label">
            <Icon name="sparkles" />
            {t("Agent")}
          </span>
          <select
            className="settings-select"
            value={agent}
            aria-label={t("Translation agent")}
            onChange={(event) => {
              setAgent(event.target.value);
              setDraft("");
              setEffort("");
              setSavedNotice(null);
              setSaveError(null);
              setManualOpen(false);
            }}
          >
            {cli.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.label}
                {provider.available ? "" : " (not installed)"}
              </option>
            ))}
            {custom.map((provider) => (
              <option key={provider.id} value={`custom:${provider.id}`}>
                {provider.name} {t("(custom)")}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="icon-button model-settings__refresh"
          disabled={loading}
          onClick={() => loadCatalog(agent, true)}
          aria-label={t("Refresh models")}
          title={t("Refresh models")}
        >
          <Icon name="refresh" spin={loading} />
        </button>
      </div>

      <div className="model-settings__catalog">
        <div className="model-settings__list-heading">
          <span className="settings-label">
            <Icon name="list" />
            {t("Model")}
          </span>
          {catalog?.agent === agent && !loading ? (
            <span
              className="dialog__muted"
              role="status"
              aria-label={t("Available models")}
            >
              {filtered.length} / {catalog.models.length}
            </span>
          ) : null}
        </div>
        <label className="form-field">
          <span className="visually-hidden">{t("Search models")}</span>
          <input
            type="search"
            aria-label={t("Search models")}
            placeholder={t("Search models")}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>
        {loadError ? (
          <StatusNotice kind="error" onRetry={() => loadCatalog(agent)}>
            {loadError}
          </StatusNotice>
        ) : null}
        {loading ? (
          <StatusNotice kind="loading">{t("Loading models…")}</StatusNotice>
        ) : null}
        {!loading && !loadError && filtered.length === 0 ? (
          <p className="dialog__muted">
            {search.trim() === ""
              ? t("No models were listed for this agent.")
              : t("No models match your search.")}
          </p>
        ) : null}
        {!loading && catalog?.agent === agent && filtered.length > 0 ? (
          <div
            className="model-list"
            role="radiogroup"
            aria-label={t("Available models")}
            tabIndex={0}
          >
            {filtered.map((model) => (
              <label
                key={model.id}
                className="model-list__row"
                data-selected={draft === model.id}
              >
                <input
                  type="radio"
                  name="translation-model"
                  checked={draft === model.id}
                  onChange={() => {
                    setDraft(model.id);
                    setEffort("");
                    setSaveError(null);
                    setSavedNotice(null);
                  }}
                />
                <span className="model-list__main">
                  <span className="model-list__title">{model.label}</span>
                </span>
              </label>
            ))}
          </div>
        ) : null}
      </div>

      <label className="form-field settings-row model-settings__effort">
        <span className="settings-label">
          <Icon name="settings" />
          {t("Reasoning effort")}
          {agent === "opencode" ? " / model variant" : ""}
        </span>
        <select
          className="settings-select"
          value={effectiveEffort}
          disabled={loading || draft.trim() === "" || effortOptions.length === 0}
          aria-describedby="reasoning-effort-help"
          onChange={(event) => {
            setEffort(event.target.value);
            setSavedNotice(null);
            setSaveError(null);
          }}
        >
          <option value="">{t("Default (provider setting)")}</option>
          {orderedEfforts.map((value) => (
            <option key={value} value={value}>
              {effortLabel(value)}
            </option>
          ))}
        </select>
      </label>

      <details
        className="settings-help model-settings__manual model-settings__help"
        open={manualOpen}
        onToggle={(event) => setManualOpen(event.currentTarget.open)}
      >
        <summary>
          <Icon name="settings" />
          {t("More options")}
        </summary>
        <label className="form-field">
          <span>{t("Or enter a model ID manually")}</span>
          <input
            type="text"
            value={draft}
            maxLength={128}
            placeholder="e.g. gpt-5.6-sol"
            onChange={(event) => {
              setDraft(event.target.value);
              setEffort("");
              setSaveError(null);
              setSavedNotice(null);
            }}
          />
        </label>

        {selectedModel?.detail ? (
          <p className="dialog__muted">
            <strong>{selectedModel.label}</strong> · {selectedModel.detail}
          </p>
        ) : null}
        {catalog?.agent === agent ? (
          <p className="dialog__muted">
            {catalog.source === "live"
              ? t("Live list")
              : catalog.source === "cached"
                ? t("Cached list")
                : t("Known list")}{" "}
            ·{" "}
            {catalog.message}
          </p>
        ) : null}
        <p className="dialog__muted" id="reasoning-effort-help">
          {agent.startsWith("custom:") && customProtocol === "openai"
            ? "Send the selected reasoning effort to your API provider. Available choices are overrides, not detected model capabilities. If the provider rejects a value, choose another level or Default to use its own setting."
            : effortOptions.length === 0
              ? "No adjustable effort levels are listed for this model. The provider default will be used."
              : "Higher effort can take longer and use more tokens. Available levels depend on the model and provider."}
        </p>
        <p className="dialog__muted">
          {t(
            "The agent and model future translation will use. Saving only remembers the choice — nothing is translated and no book text is sent.",
          )}
        </p>
        {saved ? (
          <p className="dialog__muted">
            {t("Current selection:")} {agentLabel(saved.agent, cli, custom)} ·{" "}
            {saved.model} · {saved.reasoningEffort ?? "Default"}
          </p>
        ) : null}
      </details>

      <div className="settings-footer">
        {saveError ? <StatusNotice kind="error">{saveError}</StatusNotice> : null}
        <div className="settings-footer__actions">
          <div className="model-settings__save-state" role="status">
            <span>
              {saving
                ? t("Saving…")
                : draft.trim() === ""
                  ? t("No translation model selected yet.")
                  : hasChanges
                    ? t("Unsaved changes")
                    : saved || savedNotice
                      ? t("Saved")
                      : t("No translation model selected yet.")}
            </span>
            {draft.trim() ? <span className="dialog__muted">{draft.trim()}</span> : null}
          </div>
          <button
            type="button"
            className="btn btn--primary"
            disabled={saving || loading || draft.trim() === ""}
            onClick={() => void handleSave()}
          >
            <Icon name="save" />
            {saving ? t("Saving…") : t("Save")}
          </button>
        </div>
      </div>
    </section>
  );
}
