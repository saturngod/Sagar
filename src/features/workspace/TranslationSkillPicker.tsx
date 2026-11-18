import { useEffect, useId, useState } from "react";
import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";
import { getTranslationSkill } from "../../lib/tauri/commands";
import type { TranslationSkillKind, TranslationSkillSummary } from "../../types/project";

export function primaryLanguage(tag: string): string {
  return tag.trim().split(/[-_]/)[0]?.toLowerCase() ?? "";
}

/** True when the skill can be used for a book with this translation language. */
export function skillFitsTarget(
  skill: TranslationSkillSummary,
  targetLanguage: string,
): boolean {
  return skill.kind !== "language" || skill.target === primaryLanguage(targetLanguage);
}

export function TranslationSkillPicker({
  kind,
  skills,
  value,
  targetLanguage,
  disabled,
  onChange,
}: {
  kind: TranslationSkillKind;
  skills: TranslationSkillSummary[];
  value: string | null;
  targetLanguage: string;
  disabled: boolean;
  onChange: (id: string | null) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const options = skills.filter(
    (skill) => skill.kind === kind && skillFitsTarget(skill, targetLanguage),
  );
  const known = skills.find((skill) => skill.id === value && skill.kind === kind);
  const selected = options.find((skill) => skill.id === value) ?? null;
  const problem = !value
    ? null
    : !known
      ? t("This skill is no longer available. Choose another skill or None.")
      : !selected
        ? t(
            "This language skill doesn't match the translation language. Choose another skill or None.",
          )
        : null;
  const [rules, setRules] = useState<{ id: string; body: string } | null>(null);
  const [rulesError, setRulesError] = useState<string | null>(null);
  const [rulesOpen, setRulesOpen] = useState(false);
  useEffect(() => {
    if (!rulesOpen || !selected || rules?.id === selected.id) return;
    let disposed = false;
    setRulesError(null);
    getTranslationSkill(selected.id)
      .then((skill) => {
        if (!disposed) setRules({ id: skill.id, body: skill.body });
      })
      .catch((failure: unknown) => {
        if (!disposed)
          setRulesError(
            failure instanceof Error ? failure.message : "The skill could not be loaded.",
          );
      });
    return () => {
      disposed = true;
    };
  }, [rulesOpen, selected, rules?.id]);
  const label = kind === "genre" ? t("Genre skill") : t("Language skill");
  return (
    <div className="skill-picker">
      <label className="form-field" htmlFor={`${id}-select`}>
        <span>{label}</span>
        <select
          id={`${id}-select`}
          value={value ?? ""}
          disabled={disabled}
          aria-invalid={problem ? true : undefined}
          aria-describedby={`${id}-detail`}
          onChange={(event) => {
            setRulesOpen(false);
            onChange(event.target.value || null);
          }}
        >
          <option value="">{t("None")}</option>
          {options.map((skill) => (
            <option key={skill.id} value={skill.id}>
              {t(skill.label)}
            </option>
          ))}
          {value && !selected ? (
            <option value={value} disabled>
              {known ? t(known.label) : t("Unavailable skill")}
            </option>
          ) : null}
        </select>
      </label>
      <p
        id={`${id}-detail`}
        className={
          problem
            ? "form-field__error skill-picker__detail"
            : "dialog__muted skill-picker__detail"
        }
      >
        {problem ??
          selected?.description ??
          (kind === "language" && options.length === 0
            ? t("No language skill is available for this translation language yet.")
            : kind === "genre"
              ? t("Choose the kind of book to apply its translation rules.")
              : t("Choose rules for writing the translation language."))}
      </p>
      {selected ? (
        <details
          className="settings-help skill-picker__rules"
          open={rulesOpen}
          onToggle={(event) => setRulesOpen(event.currentTarget.open)}
        >
          <summary>
            <Icon name="paragraph" />
            {t("Skill rules")}
          </summary>
          {rulesError ? (
            <p className="form-field__error">{rulesError}</p>
          ) : rules?.id === selected.id ? (
            <div className="skill-picker__body" lang="en">
              {rules.body}
            </div>
          ) : (
            <p className="dialog__muted">{t("Loading skill rules…")}</p>
          )}
        </details>
      ) : null}
    </div>
  );
}
