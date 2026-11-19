import { useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  getBookSettings,
  getProject,
  listChapters,
  listTranslationSkills,
  saveBookSettings,
} from "../../lib/tauri/commands";
import type {
  BookSettings,
  ProjectSummary,
  TranslationSkillSummary,
} from "../../types/project";
import { untranslatedChapters, useAiBatch, useNow } from "../translator/aiBatch";
import { batchProgress, batchStateLabel } from "../translator/BatchTranslationBar";
import { TranslationSkillPicker } from "./TranslationSkillPicker";

/** Matches the Rust limit for the pause between batch chapters. */
const MAX_BATCH_DELAY_SECS = 3600;

export function BookSettingsDialog({
  project,
  onClose,
  onSaved,
}: {
  project: ProjectSummary;
  onClose: () => void;
  onSaved: (project: ProjectSummary) => void;
}) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<BookSettings | null>(null);
  const [tab, setTab] = useState<"metadata" | "instructions">("metadata");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [readingCover, setReadingCover] = useState(false);
  const [retry, setRetry] = useState(0);
  const dialog = useRef<HTMLDivElement>(null);
  const coverInput = useRef<HTMLInputElement>(null);
  const [descriptionOpen, setDescriptionOpen] = useState(false);
  const [skills, setSkills] = useState<TranslationSkillSummary[] | null>(null);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [skillsRetry, setSkillsRetry] = useState(0);
  const batchApi = useAiBatch();
  const batch = batchApi.batch;
  const batchActive = batch?.phase === "running" || batch?.phase === "waiting";
  const thisBatchActive = batchActive && batch?.projectId === project.id;
  const now = useNow(thisBatchActive);
  const [untranslated, setUntranslated] = useState<number | null>(null);
  const [batchConsent, setBatchConsent] = useState(false);
  const [batchError, setBatchError] = useState<string | null>(null);
  const [startingBatch, setStartingBatch] = useState(false);
  const batchProcessed = batch?.processed;
  // biome-ignore lint/correctness/useExhaustiveDependencies: batchProcessed recounts as the batch saves chapters.
  useEffect(() => {
    let disposed = false;
    listChapters(project.id)
      .then((chapters) => {
        if (!disposed) setUntranslated(untranslatedChapters(chapters).length);
      })
      .catch(() => {
        if (!disposed) setUntranslated(null);
      });
    return () => {
      disposed = true;
    };
  }, [project.id, batchProcessed]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: skillsRetry explicitly reloads the skill list after a failure.
  useEffect(() => {
    let disposed = false;
    setSkillsError(null);
    listTranslationSkills()
      .then((value) => {
        if (!disposed) setSkills(value);
      })
      .catch((failure: unknown) => {
        if (!disposed)
          setSkillsError(
            failure instanceof Error
              ? failure.message
              : "Translation skills could not be loaded.",
          );
      });
    return () => {
      disposed = true;
    };
  }, [skillsRetry]);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    return () => previous?.focus();
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: retry explicitly reloads settings after an I/O failure.
  useEffect(() => {
    let disposed = false;
    setError(null);
    getBookSettings(project.id)
      .then((value) => {
        if (!disposed) {
          setSettings(value);
          setDescriptionOpen(value.description.trim() !== "");
        }
      })
      .catch((failure: unknown) => {
        if (!disposed)
          setError(
            failure instanceof Error
              ? failure.message
              : "Book settings could not be loaded.",
          );
      });
    return () => {
      disposed = true;
    };
  }, [project.id, retry]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        if (!saving && !readingCover) onClose();
      }
      if (event.key !== "Tab") return;
      const controls = Array.from(
        dialog.current?.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), summary",
        ) ?? [],
      ).filter((control) => control.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === dialog.current)
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, saving, readingCover]);
  async function save() {
    if (!settings || saving) return;
    setSaving(true);
    setError(null);
    try {
      await saveBookSettings(project.id, settings);
      onSaved(await getProject(project.id));
      onClose();
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "Book settings could not be saved.",
      );
    } finally {
      setSaving(false);
    }
  }
  const delayValid =
    settings !== null &&
    Number.isInteger(settings.batchDelaySecs) &&
    settings.batchDelaySecs >= 0 &&
    settings.batchDelaySecs <= MAX_BATCH_DELAY_SECS;
  async function translateAll() {
    if (!settings || saving || startingBatch || batchActive || !batchConsent) return;
    if (!delayValid) {
      setBatchError(`Enter a pause between 0 and ${MAX_BATCH_DELAY_SECS} seconds.`);
      return;
    }
    setStartingBatch(true);
    setBatchError(null);
    try {
      // The batch reads instructions, skills and the pause from saved settings.
      await saveBookSettings(project.id, settings);
      const updated = await getProject(project.id);
      onSaved(updated);
      await batchApi.start({
        projectId: project.id,
        projectTitle: updated.title,
        delaySecs: settings.batchDelaySecs,
      });
      onClose();
    } catch (failure) {
      setBatchError(
        failure instanceof Error
          ? failure.message
          : "Translating all chapters could not start.",
      );
    } finally {
      setStartingBatch(false);
    }
  }
  async function chooseCover(file: File | undefined) {
    if (!file) return;
    if (
      !["image/png", "image/jpeg", "image/gif"].includes(file.type) ||
      file.size > 8 * 1024 * 1024
    ) {
      setError("Choose a PNG, JPEG or GIF image under 8 MB.");
      return;
    }
    setReadingCover(true);
    setError(null);
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () =>
          typeof reader.result === "string"
            ? resolve(reader.result)
            : reject(new Error("The cover could not be read."));
        reader.onerror = () =>
          reject(new Error("The cover could not be read. Choose the image again."));
        reader.readAsDataURL(file);
      });
      // Verify browser decoding before retaining the selection; Rust also
      // checks format signatures and size at the save boundary.
      await new Promise<void>((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve();
        image.onerror = () =>
          reject(new Error("This image could not be decoded. Choose another cover."));
        image.src = data;
      });
      setSettings((current) => (current ? { ...current, customCover: data } : current));
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "The cover could not be read.",
      );
    } finally {
      setReadingCover(false);
    }
  }
  function update(key: keyof BookSettings, value: string) {
    setSettings((current) => (current ? { ...current, [key]: value } : current));
  }
  return (
    <div className="dialog-backdrop">
      <div
        className="dialog book-settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="book-settings-title"
        tabIndex={-1}
        ref={dialog}
      >
        <div className="panel__header">
          <div className="book-settings__heading">
            <h2 className="panel__heading" id="book-settings-title">
              {t("Book settings")}
            </h2>
            <p className="dialog__muted book-settings__book-title" title={project.title}>
              {project.title}
            </p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Close book settings")}
            title={t("Close")}
            disabled={saving || readingCover}
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        </div>
        <div
          className="tabs"
          role="tablist"
          aria-label={t("Book settings sections")}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            const next = tab === "metadata" ? "instructions" : "metadata";
            setTab(next);
            document.getElementById(`book-settings-tab-${next}`)?.focus();
          }}
        >
          <button
            type="button"
            className="tab"
            role="tab"
            id="book-settings-tab-metadata"
            aria-selected={tab === "metadata"}
            aria-controls="book-settings-panel-metadata"
            tabIndex={tab === "metadata" ? 0 : -1}
            onClick={() => setTab("metadata")}
          >
            <Icon name="book" />
            {t("Metadata")}
          </button>
          <button
            type="button"
            className="tab"
            role="tab"
            id="book-settings-tab-instructions"
            aria-selected={tab === "instructions"}
            aria-controls="book-settings-panel-instructions"
            tabIndex={tab === "instructions" ? 0 : -1}
            onClick={() => setTab("instructions")}
          >
            <Icon name="sparkles" />
            {t("AI instructions")}
          </button>
        </div>
        {!settings && !error ? (
          <StatusNotice kind="loading">{t("Loading book settings…")}</StatusNotice>
        ) : null}
        {error && !settings ? (
          <StatusNotice
            kind="error"
            onRetry={!settings ? () => setRetry((value) => value + 1) : undefined}
          >
            {error}
          </StatusNotice>
        ) : null}
        {settings ? (
          <form
            className="book-settings__form"
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            <div
              role="tabpanel"
              id={`book-settings-panel-${tab}`}
              aria-labelledby={`book-settings-tab-${tab}`}
            >
              {tab === "metadata" ? (
                <div className="book-settings__metadata">
                  <div className="book-cover-settings">
                    {settings.customCover ? (
                      <img
                        className="book-cover-settings__preview"
                        src={settings.customCover}
                        alt={t("Custom export cover preview")}
                      />
                    ) : (
                      <div
                        className="book-cover-settings__placeholder"
                        aria-hidden="true"
                      >
                        <Icon name="image" size={24} />
                      </div>
                    )}
                    <div className="book-cover-settings__info">
                      <span className="book-cover-settings__label">
                        {t("Export cover")}
                      </span>
                      <span className="dialog__muted">
                        {readingCover
                          ? t("Reading cover…")
                          : settings.customCover
                            ? t("Custom cover")
                            : t("Original cover")}
                      </span>
                      <span className="dialog__muted">PNG · JPEG · GIF · ≤ 8 MB</span>
                    </div>
                    <input
                      ref={coverInput}
                      hidden
                      aria-label={t("Choose custom cover")}
                      type="file"
                      accept="image/png,image/jpeg,image/gif"
                      disabled={saving || readingCover}
                      onChange={(event) => {
                        void chooseCover(event.target.files?.[0]);
                        event.target.value = "";
                      }}
                    />
                    <div className="book-cover-settings__actions">
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={saving || readingCover}
                        onClick={() => coverInput.current?.click()}
                      >
                        <Icon name="upload" />
                        {t("Choose cover")}
                      </button>
                      {settings.customCover ? (
                        <button
                          type="button"
                          className="icon-button"
                          disabled={saving || readingCover}
                          aria-label={t("Use original cover")}
                          title={t("Use original cover")}
                          onClick={() => setSettings({ ...settings, customCover: null })}
                        >
                          <Icon name="undo" />
                        </button>
                      ) : null}
                    </div>
                  </div>
                  <label className="form-field">
                    <span>{t("Export title")}</span>
                    <input
                      value={settings.title}
                      required
                      maxLength={1024}
                      disabled={saving || readingCover}
                      onChange={(event) => update("title", event.target.value)}
                    />
                  </label>
                  <div className="book-settings__fields">
                    {(
                      [
                        ["author", "Primary author"],
                        ["publisher", "Publisher"],
                        ["sourceLanguage", "Original language"],
                        ["targetLanguage", "Translation language"],
                      ] as const
                    ).map(([key, label]) => (
                      <label className="form-field" key={key}>
                        <span>{t(label)}</span>
                        <input
                          value={settings[key]}
                          required={key === "targetLanguage"}
                          maxLength={key.includes("Language") ? 64 : 1024}
                          disabled={saving || readingCover}
                          onChange={(event) => update(key, event.target.value)}
                        />
                      </label>
                    ))}
                  </div>
                  <details
                    className="settings-help book-settings__description"
                    open={descriptionOpen}
                    onToggle={(event) => setDescriptionOpen(event.currentTarget.open)}
                  >
                    <summary>
                      <Icon name="paragraph" />
                      {t("Description")}
                    </summary>
                    <label className="form-field">
                      <span className="visually-hidden">{t("Description")}</span>
                      <textarea
                        rows={3}
                        value={settings.description}
                        maxLength={20000}
                        disabled={saving || readingCover}
                        onChange={(event) => update("description", event.target.value)}
                      />
                    </label>
                  </details>
                  <details className="settings-help book-settings__help">
                    <summary>
                      <Icon name="keep" />
                      {t("Export details")}
                    </summary>
                    <p className="dialog__muted">
                      {t(
                        "These fields describe the exported EPUB. The original book is preserved. Leave author, publisher or description blank to keep its source value.",
                      )}
                    </p>
                    <p className="dialog__muted">
                      {t("The original cover will be used, if the book has one.")}
                    </p>
                    <p className="dialog__muted">
                      {t(
                        "PNG, JPEG or GIF, up to 8 MB. Saved with Book Settings and included in export. Cancel discards this selection.",
                      )}
                    </p>
                  </details>
                </div>
              ) : (
                <div className="book-settings__instructions">
                  <section
                    className="book-settings__skills"
                    aria-labelledby="book-settings-skills-heading"
                  >
                    <h3
                      className="book-settings__section-title"
                      id="book-settings-skills-heading"
                    >
                      <Icon name="sparkles" />
                      {t("Translation skills")}
                    </h3>
                    {skills ? (
                      <div className="book-settings__fields">
                        {(["genre", "language"] as const).map((kind) => (
                          <TranslationSkillPicker
                            key={kind}
                            kind={kind}
                            skills={skills}
                            value={
                              (kind === "genre"
                                ? settings.genreSkillId
                                : settings.languageSkillId) ?? null
                            }
                            targetLanguage={settings.targetLanguage}
                            disabled={saving || readingCover}
                            onChange={(id) =>
                              setSettings((current) =>
                                current
                                  ? {
                                      ...current,
                                      [kind === "genre"
                                        ? "genreSkillId"
                                        : "languageSkillId"]: id,
                                    }
                                  : current,
                              )
                            }
                          />
                        ))}
                      </div>
                    ) : skillsError ? (
                      <StatusNotice
                        kind="error"
                        onRetry={() => setSkillsRetry((v) => v + 1)}
                      >
                        {skillsError}
                      </StatusNotice>
                    ) : (
                      <StatusNotice kind="loading">
                        {t("Loading translation skills…")}
                      </StatusNotice>
                    )}
                    <p className="dialog__muted">
                      {t(
                        "Skill rules are sent with every AI translation in this book. The book and current-run instructions take precedence when they conflict.",
                      )}
                    </p>
                  </section>
                  <section
                    className="book-settings__skills book-settings__batch"
                    aria-labelledby="book-settings-batch-heading"
                  >
                    <h3
                      className="book-settings__section-title"
                      id="book-settings-batch-heading"
                    >
                      <Icon name="book" />
                      {t("Translate all untranslated chapters")}
                    </h3>
                    {thisBatchActive && batch ? (
                      <div className="book-settings__batch-status">
                        <StatusNotice kind="loading">
                          {t(batchStateLabel(batch))} · {batchProgress(batch, now, t)}
                        </StatusNotice>
                        <button
                          type="button"
                          className="btn btn--cancel"
                          onClick={batchApi.stop}
                        >
                          <Icon name="stop" />
                          {t("Stop")}
                        </button>
                      </div>
                    ) : (
                      <>
                        <div className="book-settings__batch-row">
                          <label className="form-field book-settings__batch-delay">
                            <span>{t("Pause between chapters (seconds)")}</span>
                            <input
                              type="number"
                              inputMode="numeric"
                              min={0}
                              max={MAX_BATCH_DELAY_SECS}
                              step={1}
                              value={
                                Number.isFinite(settings.batchDelaySecs)
                                  ? settings.batchDelaySecs
                                  : ""
                              }
                              aria-invalid={!delayValid}
                              disabled={saving || readingCover || startingBatch}
                              onChange={(event) => {
                                const value =
                                  event.target.value === ""
                                    ? Number.NaN
                                    : Number(event.target.value);
                                setSettings((current) =>
                                  current
                                    ? { ...current, batchDelaySecs: value }
                                    : current,
                                );
                              }}
                            />
                          </label>
                          <p className="dialog__muted book-settings__batch-count">
                            {untranslated === null
                              ? t("Counting untranslated chapters…")
                              : untranslated === 0
                                ? t("Every chapter already has a translation.")
                                : `${untranslated} ${
                                    untranslated === 1
                                      ? t("untranslated chapter")
                                      : t("untranslated chapters")
                                  }`}
                          </p>
                        </div>
                        <label className="export-ack">
                          <input
                            type="checkbox"
                            checked={batchConsent}
                            disabled={!untranslated || startingBatch}
                            onChange={(event) => setBatchConsent(event.target.checked)}
                          />
                          <span>
                            {t(
                              "Send the text of every untranslated chapter to the selected translation agent, one chapter at a time.",
                            )}
                          </span>
                        </label>
                        {batchActive && batch ? (
                          <StatusNotice kind="info">
                            {t("Another book is being translated:")} {batch.projectTitle}
                          </StatusNotice>
                        ) : null}
                        {batchError ? (
                          <StatusNotice kind="error">{t(batchError)}</StatusNotice>
                        ) : null}
                        <div className="button-row">
                          <button
                            type="button"
                            className="btn btn--primary"
                            disabled={
                              !batchConsent ||
                              !untranslated ||
                              !delayValid ||
                              batchActive ||
                              saving ||
                              readingCover ||
                              startingBatch
                            }
                            onClick={() => void translateAll()}
                          >
                            <Icon name="sparkles" />
                            {startingBatch ? t("Starting…") : t("Translate all")}
                          </button>
                        </div>
                      </>
                    )}
                    <p className="dialog__muted">
                      {t(
                        "Saves Book Settings, then translates chapters in reading order, pausing between chapters. Each result is saved as an unreviewed draft. Chapters that already have text are skipped, and you can stop at any time.",
                      )}
                    </p>
                  </section>
                  <label className="form-field">
                    <span>{t("Book translation instructions")}</span>
                    <textarea
                      className="book-settings__instructions-editor"
                      value={settings.instructions}
                      maxLength={20000}
                      disabled={saving || readingCover}
                      placeholder={t("Use these character names consistently…")}
                      onChange={(event) => update("instructions", event.target.value)}
                    />
                  </label>
                  <details className="settings-help book-settings__help">
                    <summary>
                      <Icon name="keep" />
                      {t("Instructions help")}
                    </summary>
                    <p className="dialog__muted">
                      {t(
                        "Applied to every AI translation in this book, together with the current run's instructions. Use this for consistent names, terminology and writing style.",
                      )}
                    </p>
                    <p className="dialog__muted">
                      {t(
                        "Saved locally. Sent to the selected agent only when you generate a translation; not included in the exported EPUB.",
                      )}
                    </p>
                  </details>
                </div>
              )}
            </div>
            <div className="settings-footer">
              {error ? <StatusNotice kind="error">{error}</StatusNotice> : null}
              <div className="settings-footer__actions">
                <span className="dialog__muted">{t("Save applies to both tabs.")}</span>
                <div className="button-row dialog__actions">
                  <button
                    type="button"
                    className="btn"
                    disabled={saving || readingCover}
                    onClick={onClose}
                  >
                    <Icon name="close" />
                    {t("Cancel")}
                  </button>
                  <button
                    type="submit"
                    className="btn btn--primary"
                    disabled={saving || readingCover}
                  >
                    <Icon name="save" />
                    {saving ? t("Saving…") : t("Save")}
                  </button>
                </div>
              </div>
            </div>
          </form>
        ) : null}
      </div>
    </div>
  );
}
