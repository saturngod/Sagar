// biome-ignore-all lint/a11y/noNoninteractiveTabindex: the scrollable chapter region must support keyboard scrolling.

import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  type ExportFormat,
  exportEpub,
  pickExportDestination,
  reviewExport,
} from "../../lib/tauri/commands";
import type { ExportOutcome, ExportReview } from "../../types/chapter";
import type { ProjectSummary } from "../../types/project";

type ExportDialogProps = {
  project: ProjectSummary;
  /** Output format of the translated book. */
  format: ExportFormat;
  onClose: () => void;
  /** Flushes pending chapter saves before reviewing/exporting. */
  onFlushSaves: () => Promise<boolean>;
  onOutcome: (outcome: ExportOutcome) => void;
};

/**
 * Export review dialog: metadata fields, actionable chapter counts and an
 * explicit draft acknowledgement, per the export design rules.
 */
export function ExportDialog({
  project,
  format,
  onClose,
  onFlushSaves,
  onOutcome,
}: ExportDialogProps) {
  const { t } = useTranslation();
  const [review, setReview] = useState<ExportReview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [title, setTitle] = useState(project.title);
  const [language, setLanguage] = useState(project.targetLanguage);
  const [languageError, setLanguageError] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [includeOriginal, setIncludeOriginal] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => {
      previousFocus.current?.focus();
    };
  }, []);

  const loadReview = useCallback(() => {
    setLoadError(null);
    onFlushSaves()
      .then((ok) => {
        if (!ok) {
          setLoadError("The chapter could not be saved. Resolve its save error first.");
          return null;
        }
        return reviewExport(project.id);
      })
      .then((result) => {
        if (result) {
          setReview(result);
          if (result.translatedTitle !== null && result.translatedTitle !== "") {
            setTitle(result.translatedTitle);
          }
        }
      })
      .catch((error: unknown) => {
        setLoadError(
          error instanceof Error
            ? error.message
            : "The export review could not be loaded.",
        );
      });
  }, [onFlushSaves, project.id]);

  useEffect(() => {
    loadReview();
  }, [loadReview]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        if (!exporting) onClose();
      }
      if (event.key === "Tab") {
        const controls = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), input:not(:disabled), [tabindex="0"]',
          ) ?? [],
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
  }, [onClose, exporting]);

  const detailsId = useId();
  const languageValid =
    language.trim().length > 0 &&
    [...language.trim()].every((c) => /[A-Za-z0-9-]/.test(c));

  const formatLabel = format === "kfx" ? "KFX" : "EPUB";
  const translatedChapters =
    review?.chapters.filter((c) => c.reviewed || c.draft).length ?? 0;
  const totalChapters = review?.chapters.length ?? 0;
  const readyChapters = review ? review.reviewedTotal + review.skippedTotal : 0;
  const readyPercent =
    totalChapters > 0 ? Math.round((readyChapters / totalChapters) * 100) : 0;
  // Chapters with no saved translation: they keep the original text or are left out.
  const untranslatedChapters = review ? review.pendingTotal + review.skippedTotal : 0;
  const mode: "completed" | "draft" = review?.complete ? "completed" : "draft";
  const nothingToExport =
    mode === "draft" && !includeOriginal && translatedChapters === 0;

  async function runExport() {
    if (!review) {
      return;
    }
    if (!languageValid) {
      setLanguageError("Enter a language tag such as “my” or “en-US”.");
      return;
    }
    if (nothingToExport) {
      setExportError(
        "No chapters are translated yet. Choose “Keep the original text” or translate a chapter first.",
      );
      return;
    }
    setExportError(null);
    setLanguageError(null);
    const fileBase = (title.trim() || review.title).replace(/[\\/:*?"<>|]/g, "_");
    const destination = await pickExportDestination(
      `${fileBase}-${language.trim()}.${format}`,
      format,
    );
    if (destination === null) {
      return; // Save dialog cancelled.
    }
    setExporting(true);
    try {
      const outcome = await exportEpub(
        project.id,
        destination,
        mode,
        title.trim() === "" ? null : title.trim(),
        language.trim(),
        mode === "draft" ? includeOriginal : true,
        format,
      );
      onOutcome(outcome);
    } catch (error) {
      setExportError(
        error instanceof Error ? error.message : "The book could not be exported.",
      );
    } finally {
      setExporting(false);
    }
  }

  function chapterStatus(chapter: ExportReview["chapters"][number]) {
    if (chapter.reviewed) return { label: t("Translated"), tone: "reviewed" };
    if (chapter.draft) return { label: t("Draft"), tone: "draft" };
    const omitted = mode === "draft" && !includeOriginal;
    if (chapter.skipped)
      return { label: omitted ? t("Left out") : t("Original text"), tone: "original" };
    return {
      label: omitted ? t("Left out") : t("Not translated · original text"),
      tone: "pending",
    };
  }

  const incompleteParts = review
    ? [
        review.pendingTotal > 0
          ? `${review.pendingTotal} ${t(review.pendingTotal === 1 ? "chapter isn’t translated" : "chapters aren’t translated")}`
          : null,
        review.draftTotal > 0
          ? `${review.draftTotal} ${t(review.draftTotal === 1 ? "draft isn’t reviewed" : "drafts aren’t reviewed")}`
          : null,
      ].filter((part): part is string => part !== null)
    : [];

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog export-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="export-dialog-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <div className="panel__header export-dialog__header">
          <h2 className="panel__heading" id="export-dialog-title">
            {format === "kfx" ? t("Export translated KFX") : t("Export translated EPUB")}
          </h2>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Close")}
            title={t("Close")}
            disabled={exporting}
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        </div>

        {loadError ? (
          <div className="export-dialog__body">
            <StatusNotice kind="error" onRetry={loadReview}>
              {loadError}
            </StatusNotice>
          </div>
        ) : null}
        {!loadError && !review ? (
          <div className="export-dialog__body">
            <StatusNotice kind="loading">
              {t("Reviewing saved translations…")}
            </StatusNotice>
          </div>
        ) : null}

        {review ? (
          <>
            <div className="export-dialog__body">
              <section className="export-progress" aria-label={t("Translation progress")}>
                <div className="export-progress__headline">
                  <strong>
                    {readyChapters} / {totalChapters} {t("chapters ready")}
                  </strong>
                  <span>{readyPercent}%</span>
                </div>
                <div
                  className="export-progress__track"
                  role="progressbar"
                  aria-label={t("Chapters ready")}
                  aria-valuemin={0}
                  aria-valuemax={totalChapters}
                  aria-valuenow={readyChapters}
                >
                  <span style={{ width: `${readyPercent}%` }} />
                </div>
                <ul className="export-progress__legend">
                  <li className="export-progress__item export-progress__item--reviewed">
                    {review.reviewedTotal} {t("translated")}
                  </li>
                  <li className="export-progress__item export-progress__item--original">
                    {review.skippedTotal} {t("kept original")}
                  </li>
                  {review.draftTotal > 0 ? (
                    <li className="export-progress__item export-progress__item--draft">
                      {review.draftTotal} {t("draft")}
                    </li>
                  ) : null}
                  {review.pendingTotal > 0 ? (
                    <li className="export-progress__item export-progress__item--pending">
                      {review.pendingTotal} {t("not translated")}
                    </li>
                  ) : null}
                  {review.unsupportedTotal > 0 ? (
                    <li className="export-progress__item export-progress__item--original">
                      {review.unsupportedTotal} {t("preserved unsupported")}
                    </li>
                  ) : null}
                </ul>
              </section>

              {!review.complete ? (
                <p className="export-dialog__draft-note" role="note">
                  <Icon name="edit" size={16} />
                  <span>
                    {incompleteParts.join(" · ")}.{" "}
                    {t("This export will be saved as a draft.")}
                  </span>
                </p>
              ) : null}

              <div className="form-grid export-dialog__fields">
                <label className="form-field">
                  <span>{t("Book title")}</span>
                  <input
                    type="text"
                    value={title}
                    placeholder={review.title}
                    onChange={(event) => setTitle(event.target.value)}
                  />
                </label>
                <label className="form-field">
                  <span>{t("Language code")}</span>
                  <input
                    type="text"
                    value={language}
                    placeholder="my"
                    onChange={(event) => {
                      setLanguage(event.target.value);
                      setLanguageError(null);
                    }}
                    aria-invalid={language !== "" && !languageValid}
                  />
                  {languageError ? (
                    <span className="form-field__error">{languageError}</span>
                  ) : null}
                </label>
              </div>

              {!review.complete && untranslatedChapters > 0 ? (
                <fieldset className="export-choice">
                  <legend>
                    {t("Chapters without a translation")} ({untranslatedChapters})
                  </legend>
                  <label className="export-choice__option">
                    <input
                      type="radio"
                      name={`${detailsId}-untranslated`}
                      checked={includeOriginal}
                      onChange={() => setIncludeOriginal(true)}
                    />
                    <span>
                      <strong>{t("Keep the original text")}</strong>
                      <small>
                        {t("The book stays complete; these chapters stay untranslated.")}
                      </small>
                    </span>
                  </label>
                  <label className="export-choice__option">
                    <input
                      type="radio"
                      name={`${detailsId}-untranslated`}
                      checked={!includeOriginal}
                      onChange={() => setIncludeOriginal(false)}
                    />
                    <span>
                      <strong>{t("Leave them out")}</strong>
                      <small>{t("Only translated chapters are included.")}</small>
                    </span>
                  </label>
                </fieldset>
              ) : null}

              {review.chapters.length > 0 ? (
                <div className="export-details">
                  <button
                    type="button"
                    className="export-details__toggle"
                    aria-expanded={detailsOpen}
                    aria-controls={detailsId}
                    onClick={() => {
                      const opening = !detailsOpen;
                      setDetailsOpen(opening);
                      if (opening)
                        requestAnimationFrame(() =>
                          document
                            .getElementById(detailsId)
                            ?.scrollIntoView({ block: "nearest" }),
                        );
                    }}
                  >
                    <Icon name={detailsOpen ? "chevronDown" : "right"} size={16} />
                    <span>
                      {detailsOpen
                        ? t("Hide chapter details")
                        : t("Show chapter details")}{" "}
                      ({review.chapters.length})
                    </span>
                  </button>
                  {detailsOpen ? (
                    <ul
                      id={detailsId}
                      className="export-details__list"
                      aria-label={t("Chapter export review")}
                      tabIndex={0}
                    >
                      {review.chapters.map((chapter) => {
                        const status = chapterStatus(chapter);
                        return (
                          <li key={chapter.chapterId} className="export-details__row">
                            <span className="export-details__title">{chapter.title}</span>
                            <span
                              className={`chapter-export-status chapter-export-status--${status.tone}`}
                            >
                              {status.label}
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="export-dialog__footer">
              {exportError ? (
                <StatusNotice kind="error">{exportError}</StatusNotice>
              ) : null}
              <div className="export-dialog__actions">
                <p className="dialog__muted">
                  {format === "kfx"
                    ? t(
                        "Your original book is not changed. KFX keeps JPEG, PNG and GIF images only.",
                      )
                    : t("Your original book is not changed.")}
                </p>
                <button
                  type="button"
                  className="btn"
                  disabled={exporting}
                  onClick={onClose}
                >
                  <Icon name="close" size={16} />
                  {t("Cancel")}
                </button>
                <button
                  type="button"
                  className="btn btn--primary"
                  disabled={exporting || nothingToExport}
                  title={
                    nothingToExport
                      ? t("Choose “Keep the original text” or translate a chapter first")
                      : undefined
                  }
                  onClick={() => void runExport()}
                >
                  <Icon name="download" size={16} />
                  {exporting
                    ? t("Exporting…")
                    : mode === "completed"
                      ? `${t("Export")} ${formatLabel}`
                      : `${t("Export draft")} ${formatLabel}`}
                </button>
              </div>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
