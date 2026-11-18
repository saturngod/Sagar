import { useCallback, useEffect, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  deleteTranslationSkill,
  duplicateTranslationSkill,
  getTranslationSkillSource,
  importTranslationSkill,
  listTranslationSkills,
  pickSkillFile,
  saveTranslationSkill,
  translationSkillUsage,
} from "../../lib/tauri/commands";
import { CommandError } from "../../lib/tauri/errors";
import type { TranslationSkillSummary } from "../../types/project";
import { SkillInstalls } from "./SkillInstalls";

type Busy = "import" | "duplicate" | "save" | "delete" | null;

function message(failure: unknown, fallback: string): string {
  return failure instanceof Error ? failure.message : fallback;
}

/** Settings → Translation skills: view, import, duplicate, edit and delete skills. */
export function TranslationSkillsSettings({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  const [skills, setSkills] = useState<TranslationSkillSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [sources, setSources] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<{
    id: string;
    books: number;
  } | null>(null);
  const [pendingReplace, setPendingReplace] = useState<{
    path: string;
    label: string;
  } | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoadError(null);
    listTranslationSkills()
      .then(setSkills)
      .catch((failure: unknown) =>
        setLoadError(message(failure, "Translation skills could not be loaded.")),
      );
  }, []);
  useEffect(() => {
    if (enabled && !skills) load();
  }, [enabled, skills, load]);

  useEffect(() => {
    if (!expanded || sources[expanded] !== undefined) return;
    let disposed = false;
    getTranslationSkillSource(expanded)
      .then((text) => {
        if (!disposed) setSources((current) => ({ ...current, [expanded]: text }));
      })
      .catch((failure: unknown) => {
        if (!disposed) setError(message(failure, "The skill could not be loaded."));
      });
    return () => {
      disposed = true;
    };
  }, [expanded, sources]);

  async function refresh(focusId?: string) {
    const next = await listTranslationSkills();
    setSkills(next);
    if (focusId) setExpanded(focusId);
  }

  async function runImport(path: string, replace: boolean) {
    setBusy("import");
    setError(null);
    setNotice(null);
    try {
      const skill = await importTranslationSkill(path, replace);
      setPendingReplace(null);
      setSources((current) => {
        const { [skill.id]: _, ...rest } = current;
        return rest;
      });
      await refresh(skill.id);
      setNotice(`${t("Imported")} ${t(skill.label)}`);
    } catch (failure) {
      if (failure instanceof CommandError && failure.code === "skill_exists") {
        const details = failure.details as { label?: string } | undefined;
        setPendingReplace({ path, label: details?.label ?? "" });
      } else {
        setError(message(failure, "The skill could not be imported."));
      }
    } finally {
      setBusy(null);
    }
  }

  async function chooseImport() {
    setError(null);
    try {
      const path = await pickSkillFile();
      if (path) await runImport(path, false);
    } catch (failure) {
      setError(message(failure, "The skill could not be imported."));
    }
  }

  async function duplicate(skill: TranslationSkillSummary) {
    setBusy("duplicate");
    setError(null);
    setNotice(null);
    try {
      const copy = await duplicateTranslationSkill(skill.id);
      await refresh(copy.id);
      setNotice(`${t("Created")} ${copy.label}`);
    } catch (failure) {
      setError(message(failure, "The skill could not be duplicated."));
    } finally {
      setBusy(null);
    }
  }

  async function save() {
    if (!editing) return;
    setBusy("save");
    setError(null);
    setNotice(null);
    try {
      const saved = await saveTranslationSkill(editing.id, editing.text);
      setSources((current) => ({ ...current, [saved.id]: editing.text }));
      setEditing(null);
      await refresh(saved.id);
      setNotice(`${t("Saved")} ${saved.label}`);
    } catch (failure) {
      // Keep the edited text so the problem can be fixed and saved again.
      setError(message(failure, "The skill could not be saved."));
    } finally {
      setBusy(null);
    }
  }

  async function askDelete(skill: TranslationSkillSummary) {
    setError(null);
    setNotice(null);
    try {
      setConfirmDelete({ id: skill.id, books: await translationSkillUsage(skill.id) });
    } catch (failure) {
      setError(message(failure, "The skill could not be deleted."));
    }
  }

  async function remove(skill: TranslationSkillSummary) {
    setBusy("delete");
    setError(null);
    try {
      const kept = await deleteTranslationSkill(skill.id);
      setConfirmDelete(null);
      setExpanded(null);
      await refresh();
      setNotice(
        kept.length > 0
          ? `${t("Deleted")} ${skill.label}. ${t("A changed copy was kept in:")} ${kept.join(", ")}`
          : `${t("Deleted")} ${skill.label}`,
      );
    } catch (failure) {
      setError(message(failure, "The skill could not be deleted."));
    } finally {
      setBusy(null);
    }
  }

  const kindLabel = (skill: TranslationSkillSummary) =>
    skill.kind === "genre"
      ? t("Genre")
      : `${t("Language")}${skill.target ? ` · ${skill.target}` : ""}`;

  return (
    <section className="provider-section" aria-labelledby="skills-heading">
      <div className="provider-section__header">
        <h3 className="panel__heading" id="skills-heading">
          {t("Translation skills")}
          {skills ? <span className="provider-count">{skills.length}</span> : null}
        </h3>
        <div className="provider-section__tools">
          <button
            type="button"
            className="btn btn--small"
            disabled={busy !== null}
            onClick={() => void chooseImport()}
          >
            <Icon name="upload" />
            {busy === "import" ? t("Importing…") : t("Import skill")}
          </button>
        </div>
      </div>
      <p className="dialog__muted skills-settings__help">
        {t(
          "Skills add translation rules for a kind of book or a translation language. Choose them per book in Book Settings. Built-in skills can't be changed; duplicate one to make your own version. Imported skills are saved on this computer.",
        )}
      </p>
      {pendingReplace ? (
        <div className="provider-row__confirm" role="alert">
          <p className="provider-row__confirm-label">
            {t("A skill with this name already exists. Replace it?")}{" "}
            {pendingReplace.label ? <strong>{pendingReplace.label}</strong> : null}
          </p>
          <div className="button-row">
            <button
              type="button"
              className="btn btn--small"
              disabled={busy !== null}
              onClick={() => setPendingReplace(null)}
            >
              <Icon name="close" />
              {t("Cancel")}
            </button>
            <button
              type="button"
              className="btn btn--small btn--danger"
              disabled={busy !== null}
              onClick={() => void runImport(pendingReplace.path, true)}
            >
              <Icon name="upload" />
              {busy === "import" ? t("Importing…") : t("Replace")}
            </button>
          </div>
        </div>
      ) : null}
      {error ? <StatusNotice kind="error">{error}</StatusNotice> : null}
      {notice && !error ? <StatusNotice kind="success">{notice}</StatusNotice> : null}
      {loadError ? (
        <StatusNotice kind="error" onRetry={load}>
          {loadError}
        </StatusNotice>
      ) : !skills ? (
        <StatusNotice kind="loading">{t("Loading translation skills…")}</StatusNotice>
      ) : (
        <ul className="provider-list">
          {skills.map((skill) => {
            const open = expanded === skill.id;
            const isEditing = editing?.id === skill.id;
            const source = sources[skill.id];
            return (
              <li key={skill.id}>
                <div className="provider-row">
                  <button
                    type="button"
                    className="provider-row__toggle"
                    aria-expanded={open}
                    aria-controls={`skill-details-${skill.id}`}
                    title={t("Skill details")}
                    onClick={() => {
                      setConfirmDelete(null);
                      setExpanded(open ? null : skill.id);
                    }}
                  >
                    <span className="provider-row__chevron" aria-hidden="true">
                      <Icon name="chevronDown" size={16} />
                    </span>
                    <strong>{t(skill.label)}</strong>
                    <span className="provider-row__meta">{kindLabel(skill)}</span>
                  </button>
                  <span
                    className={`provider-status provider-status--${skill.builtin ? "original" : "reviewed"}`}
                  >
                    {skill.builtin ? t("Built-in") : t("Imported")}
                  </span>
                  <button
                    type="button"
                    className="icon-button provider-action"
                    disabled={busy !== null}
                    aria-label={`${t("Duplicate")} ${t(skill.label)}`}
                    title={`${t("Duplicate")} ${t(skill.label)}`}
                    onClick={() => void duplicate(skill)}
                  >
                    <Icon name="copy" />
                  </button>
                </div>
                <div
                  id={`skill-details-${skill.id}`}
                  className="provider-row__details skills-settings__details"
                  hidden={!open}
                >
                  <p className="skills-settings__description">{skill.description}</p>
                  {isEditing ? (
                    <label className="form-field">
                      <span>{t("Skill file (SKILL.md)")}</span>
                      <textarea
                        className="skills-settings__editor"
                        value={editing.text}
                        spellCheck={false}
                        disabled={busy === "save"}
                        onChange={(event) =>
                          setEditing({ id: skill.id, text: event.target.value })
                        }
                      />
                    </label>
                  ) : source !== undefined ? (
                    <pre className="skills-settings__source" lang="en">
                      {source}
                    </pre>
                  ) : open ? (
                    <p className="dialog__muted">{t("Loading skill rules…")}</p>
                  ) : null}
                  {confirmDelete?.id === skill.id ? (
                    <div className="provider-row__confirm">
                      <p className="provider-row__confirm-label">
                        {t("Delete skill?")} <strong>{skill.label}</strong>
                        {confirmDelete.books > 0
                          ? ` ${t("Books that use it will go back to no skill:")} ${confirmDelete.books}`
                          : ""}
                      </p>
                      <p className="dialog__muted">
                        {t("Copies Sagar installed in coding CLIs are removed too.")}
                      </p>
                      <div className="button-row">
                        <button
                          type="button"
                          className="btn btn--small"
                          disabled={busy !== null}
                          onClick={() => setConfirmDelete(null)}
                        >
                          <Icon name="close" />
                          {t("Cancel")}
                        </button>
                        <button
                          type="button"
                          className="btn btn--small btn--danger"
                          disabled={busy !== null}
                          onClick={() => void remove(skill)}
                        >
                          <Icon name="trash" />
                          {busy === "delete" ? t("Deleting…") : t("Confirm delete")}
                        </button>
                      </div>
                    </div>
                  ) : isEditing ? (
                    <div className="button-row provider-row__detail-actions">
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={busy === "save"}
                        onClick={() => {
                          setEditing(null);
                          setError(null);
                        }}
                      >
                        <Icon name="close" />
                        {t("Cancel")}
                      </button>
                      <button
                        type="button"
                        className="btn btn--small btn--primary"
                        disabled={busy !== null}
                        onClick={() => void save()}
                      >
                        <Icon name="save" />
                        {busy === "save" ? t("Saving…") : t("Save")}
                      </button>
                    </div>
                  ) : !skill.builtin ? (
                    <div className="button-row provider-row__detail-actions">
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={busy !== null || source === undefined}
                        onClick={() => {
                          setError(null);
                          setNotice(null);
                          setEditing({ id: skill.id, text: source ?? "" });
                        }}
                      >
                        <Icon name="edit" />
                        {t("Edit")}
                      </button>
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={busy !== null}
                        onClick={() => void askDelete(skill)}
                      >
                        <Icon name="trash" />
                        {t("Delete")}
                      </button>
                    </div>
                  ) : null}
                  {open && !isEditing && source !== undefined ? (
                    <SkillInstalls skillId={skill.id} revision={source} />
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
