import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  deleteProject,
  getProjectCover,
  importEpub,
  listProjects,
  pickEpubFile,
} from "../../lib/tauri/commands";
import { CommandError } from "../../lib/tauri/errors";
import type { ImportWarning, ProjectSummary } from "../../types/project";
import { BookExportMenu } from "../export/BookExportMenu";
import { AiSettingsDialog } from "../settings/AiSettingsDialog";
import type { AppearancePreferences } from "../settings/AppearanceSettings";
import { BookSettingsDialog } from "../workspace/BookSettingsDialog";

type LibraryView = "table" | "grid";
const LIBRARY_VIEW_KEY = "sagar.library-view";

function readLibraryView(): LibraryView {
  try {
    return localStorage.getItem(LIBRARY_VIEW_KEY) === "grid" ? "grid" : "table";
  } catch {
    return "table";
  }
}

type LibraryState =
  | { status: "loading" }
  | { status: "ready"; projects: ProjectSummary[] }
  | { status: "error"; message: string };

type DuplicatePrompt = {
  path: string;
  fileName: string;
  existing: ProjectSummary;
};

type LibraryScreenProps = {
  appearance: AppearancePreferences;
  onOpenProject: (projectId: string, importWarnings: ImportWarning[]) => void;
};

export function LibraryScreen({ onOpenProject, appearance }: LibraryScreenProps) {
  const { t, language } = useTranslation();
  const [library, setLibrary] = useState<LibraryState>({ status: "loading" });
  const [view, setView] = useState<LibraryView>(readLibraryView);
  const [query, setQuery] = useState("");
  const [importingName, setImportingName] = useState<string | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const [duplicate, setDuplicate] = useState<DuplicatePrompt | null>(null);
  const [removing, setRemoving] = useState<ProjectSummary | null>(null);
  const [removeBusy, setRemoveBusy] = useState(false);
  const [removeNotice, setRemoveNotice] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [bookSettings, setBookSettings] = useState<ProjectSummary | null>(null);

  useEffect(() => {
    try {
      localStorage.setItem(LIBRARY_VIEW_KEY, view);
    } catch {
      // Keep the switch usable when local preferences cannot be saved.
    }
  }, [view]);

  const refreshProjects = useCallback(() => {
    setLibrary({ status: "loading" });
    listProjects()
      .then((projects) => setLibrary({ status: "ready", projects }))
      .catch((error) =>
        setLibrary({
          status: "error",
          message: readableError(error, "Recent projects could not be loaded."),
        }),
      );
  }, []);

  useEffect(() => {
    refreshProjects();
  }, [refreshProjects]);

  async function importPath(path: string, fileName: string, allowDuplicate: boolean) {
    setDuplicate(null);
    setImportError(null);
    setImportingName(fileName);
    try {
      const result = await importEpub(path, allowDuplicate, language);
      setImportingName(null);
      onOpenProject(result.project.id, result.warnings);
    } catch (error) {
      setImportingName(null);
      if (
        error instanceof CommandError &&
        error.code === "duplicate_project" &&
        isProjectSummary(error.details)
      ) {
        setDuplicate({ path, fileName, existing: error.details });
        return;
      }
      setImportError(readableError(error, "The book could not be imported."));
    }
  }

  async function handleOpenEpub() {
    setPickError(null);
    setImportError(null);
    setDuplicate(null);
    let file: string | null;
    try {
      file = await pickEpubFile();
    } catch (error) {
      setPickError(readableError(error, "The file picker could not be opened."));
      return;
    }
    if (file === null) {
      return; // Picker cancelled — return quietly, no error notice.
    }
    const segments = file.split(/[\\/]/);
    const fileName = segments[segments.length - 1] ?? file;
    await importPath(file, fileName, false);
  }

  const filtered =
    library.status === "ready"
      ? library.projects.filter((project) =>
          `${project.title} ${project.author ?? ""}`
            .toLocaleLowerCase()
            .includes(query.toLocaleLowerCase()),
        )
      : [];
  return (
    <main className="screen screen--library">
      <header className="screen__header">
        <div>
          <h1 className="screen__heading">{t("Library")}</h1>
          <p className="library-count">
            {library.status === "ready"
              ? `${library.projects.length} ${library.projects.length === 1 ? t("book") : t("books")}`
              : t("Your books")}
          </p>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="icon-button"
            aria-label={t("Settings")}
            title={t("Settings")}
            onClick={(event) => {
              // WebKit does not focus buttons on pointer clicks by default.
              // Give the dialog a concrete trigger to restore on dismissal.
              event.currentTarget.focus();
              setSettingsOpen(true);
            }}
          >
            <Icon name="settings" />
          </button>
          <button
            type="button"
            className="btn btn--primary"
            data-open-epub
            disabled={importingName !== null}
            onClick={() => void handleOpenEpub()}
          >
            <Icon name="plus" /> {t("Import EPUB")}
          </button>
        </div>
      </header>
      {importingName ? (
        <StatusNotice kind="loading">
          {t("Importing “")}
          {importingName}”…
        </StatusNotice>
      ) : null}
      {importError ? <StatusNotice kind="error">{importError}</StatusNotice> : null}
      {pickError ? (
        <StatusNotice kind="error" onRetry={() => void handleOpenEpub()}>
          {pickError}
        </StatusNotice>
      ) : null}

      {duplicate ? (
        <section className="panel duplicate-panel" aria-label={t("Duplicate book")}>
          <div className="panel__header">
            <h2 className="panel__heading">{t("Already in your library")}</h2>
          </div>
          <p>
            “{duplicate.existing.title}
            {t(
              "” was imported from this same file. Continue that project, or import it again as a separate project.",
            )}
          </p>
          <div className="button-row">
            <button
              type="button"
              className="btn btn--primary"
              onClick={() => onOpenProject(duplicate.existing.id, [])}
            >
              <Icon name="book" />
              {t("Continue “")}
              {duplicate.existing.title}”
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => void importPath(duplicate.path, duplicate.fileName, true)}
            >
              <Icon name="upload" />
              {t("Import as separate project")}
            </button>
            <button type="button" className="btn" onClick={() => setDuplicate(null)}>
              <Icon name="close" />
              {t("Cancel")}
            </button>
          </div>
        </section>
      ) : null}

      <section className="panel library-books" aria-labelledby="recent-projects-heading">
        <div className="panel__header">
          <h2 className="panel__heading" id="recent-projects-heading">
            {t("Your books")}
          </h2>
          <div className="library-controls">
            <label className="library-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                aria-label={t("Search books")}
                placeholder={t("Search books")}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <fieldset
              className="view-switch view-switch--icons"
              aria-label={t("Library view")}
            >
              <button
                type="button"
                className="icon-button"
                aria-label={t("Table view")}
                title={t("Table view")}
                aria-pressed={view === "table"}
                onClick={() => setView("table")}
              >
                <Icon name="list" />
              </button>
              <button
                type="button"
                className="icon-button"
                aria-label={t("Grid view")}
                title={t("Grid view")}
                aria-pressed={view === "grid"}
                onClick={() => setView("grid")}
              >
                <Icon name="grid" />
              </button>
            </fieldset>
          </div>
        </div>
        {library.status === "loading" ? (
          <StatusNotice kind="loading">{t("Loading projects…")}</StatusNotice>
        ) : null}
        {library.status === "error" ? (
          <StatusNotice kind="error" onRetry={refreshProjects}>
            {library.message}
          </StatusNotice>
        ) : null}
        {library.status === "ready" && library.projects.length === 0 ? (
          <div className="empty-state">
            <Icon name="book" size={36} />
            <h3 className="empty-state__heading">{t("Add your first book")}</h3>
            <p>{t("Import an EPUB to start translating.")}</p>
          </div>
        ) : null}
        {library.status === "ready" && library.projects.length > 0 ? (
          <ul className={`project-list${view === "grid" ? " project-list--grid" : ""}`}>
            {filtered.map((project) => (
              <li key={project.id} className="project-item">
                <ProjectRow
                  project={project}
                  onOpen={onOpenProject}
                  onRequestRemove={setRemoving}
                  onSettings={setBookSettings}
                />
              </li>
            ))}
          </ul>
        ) : null}
        {library.status === "ready" &&
        library.projects.length > 0 &&
        filtered.length === 0 ? (
          <p className="library-no-results">{t("No books match your search.")}</p>
        ) : null}
      </section>

      {removeNotice ? <StatusNotice kind="info">{removeNotice}</StatusNotice> : null}

      {bookSettings ? (
        <BookSettingsDialog
          project={bookSettings}
          onClose={() => setBookSettings(null)}
          onSaved={(updated) =>
            setLibrary((current) =>
              current.status === "ready"
                ? {
                    ...current,
                    projects: current.projects.map((project) =>
                      project.id === updated.id ? updated : project,
                    ),
                  }
                : current,
            )
          }
        />
      ) : null}

      {settingsOpen ? (
        <AiSettingsDialog
          appearance={appearance}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}

      {removing ? (
        <ConfirmRemoveDialog
          project={removing}
          busy={removeBusy}
          onCancel={() => setRemoving(null)}
          onConfirm={async () => {
            setRemoveBusy(true);
            try {
              await deleteProject(removing.id);
              setRemoving(null);
              setRemoveNotice(null);
              refreshProjects();
            } catch (error) {
              if (error instanceof CommandError && error.code === "cleanup_failed") {
                // The project is gone from the library; only leftover files failed.
                setRemoving(null);
                setRemoveNotice(error.message);
                refreshProjects();
              } else {
                setRemoving(null);
                setImportError(readableError(error, "The project could not be removed."));
              }
            } finally {
              setRemoveBusy(false);
            }
          }}
        />
      ) : null}
    </main>
  );
}

function ConfirmRemoveDialog({
  project,
  busy,
  onCancel,
  onConfirm,
}: {
  project: ProjectSummary;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null;
    cancelRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      previousFocus.current?.focus();
    };
  }, [onCancel]);

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog dialog--narrow"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="remove-project-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <h2 className="panel__heading" id="remove-project-title">
          {t("Remove “")}
          {project.title}”?
        </h2>
        <p>
          {t(
            "The saved translations for this book will be deleted from this computer. The original EPUB file is not modified.",
          )}
        </p>
        <div className="button-row dialog__actions">
          <button type="button" className="btn" onClick={onCancel} ref={cancelRef}>
            <Icon name="close" />
            {t("Cancel")}
          </button>
          <button
            type="button"
            className="btn btn--danger"
            disabled={busy}
            onClick={() => void onConfirm()}
          >
            <Icon name="trash" size={16} /> {busy ? t("Deleting…") : t("Delete project")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** The library has no open editor, so there are no pending saves to flush. */
const flushNothing = () => Promise.resolve(true);

function ProjectRow({
  project,
  onOpen,
  onRequestRemove,
  onSettings,
}: {
  project: ProjectSummary;
  onOpen: (projectId: string, warnings: ImportWarning[]) => void;
  onRequestRemove: (project: ProjectSummary) => void;
  onSettings: (project: ProjectSummary) => void;
}) {
  const { t } = useTranslation();
  const [coverSrc, setCoverSrc] = useState<string | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: saved Book Settings can change the cover without changing project ID.
  useEffect(() => {
    let cancelled = false;
    getProjectCover(project.id)
      .then((cover) => {
        if (!cancelled) {
          setCoverSrc(cover ? `data:${cover.mediaType};base64,${cover.base64}` : null);
        }
      })
      .catch(() => {
        if (!cancelled) setCoverSrc(null);
        // No cover available; the neutral placeholder stays.
      });
    return () => {
      cancelled = true;
    };
  }, [project.id, project.updatedAt]);

  return (
    <>
      <button
        type="button"
        className="project-row"
        onClick={() => onOpen(project.id, [])}
      >
        <span className="project-row__cover">
          {coverSrc ? (
            <img src={coverSrc} alt="" />
          ) : (
            <span aria-hidden="true">{project.title.slice(0, 1)}</span>
          )}
        </span>
        <span className="project-row__main">
          <span className="project-row__title">
            <Icon name="book" />
            {project.title}
          </span>
          <span className="project-row__meta">
            {project.author ?? t("Unknown author")}{" "}
            <span className="project-row__language">
              {project.sourceLanguage ?? "—"} → {project.targetLanguage}
            </span>
          </span>
        </span>
        <span
          className="project-row__progress"
          role="img"
          aria-label={`${project.reviewedBlocks} of ${project.totalBlocks} chapters reviewed`}
          title={`${project.reviewedBlocks} of ${project.totalBlocks} chapters reviewed`}
        >
          <span>
            {project.reviewedBlocks} / {project.totalBlocks}
          </span>
          <span className="project-progress-track">
            <span
              style={{
                width: `${project.totalBlocks > 0 ? Math.min(100, (project.reviewedBlocks / project.totalBlocks) * 100) : 0}%`,
              }}
            />
          </span>
        </span>
        <span
          className="project-row__time"
          title={`Last edited ${formatDate(project.updatedAt)}`}
        >
          {formatDate(project.updatedAt)}
        </span>
      </button>
      <BookExportMenu project={project} onFlushSaves={flushNothing} iconSize={18} />
      <button
        type="button"
        className="icon-button"
        aria-label={`Book settings for ${project.title}`}
        title={t("Book metadata and AI instructions")}
        onClick={() => onSettings(project)}
      >
        <Icon name="settings" size={18} />
      </button>
      <button
        type="button"
        className="project-row__remove icon-button"
        aria-label={`Remove ${project.title} from the library`}
        title={t("Remove this project from the library")}
        onClick={() => onRequestRemove(project)}
      >
        <Icon name="trash" size={18} />
      </button>
    </>
  );
}

function readableError(error: unknown, fallback: string): string {
  return error instanceof CommandError ? error.message : fallback;
}

function isProjectSummary(value: unknown): value is ProjectSummary {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ProjectSummary).id === "string" &&
    typeof (value as ProjectSummary).title === "string"
  );
}

function formatDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }).format(date);
}
