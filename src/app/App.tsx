import { useCallback, useEffect, useState } from "react";
import { LibraryScreen } from "../features/library/LibraryScreen";
import { AiJobsProvider } from "../features/translator/aiJobs";
import { UpdateBanner } from "../features/updates/UpdateControls";
import { WorkspaceFrame } from "../features/workspace/WorkspaceFrame";
import { useTranslation } from "../lib/i18n";
import {
  type FontRole,
  fontStack,
  loadFonts,
  loadTextZoom,
  loadTheme,
  saveFont,
  saveTextZoom,
  saveTheme,
  type ThemePreference,
} from "../lib/prefs";
import type { ImportWarning } from "../types/project";

/**
 * App shell: navigation between the project library and the translation
 * workspace, plus global presentation preferences (theme, text zoom).
 */
export default function App() {
  const { t } = useTranslation();
  const [openProject, setOpenProject] = useState<{
    projectId: string;
    importWarnings: ImportWarning[];
  } | null>(null);
  const [theme, setTheme] = useState<ThemePreference>(loadTheme);
  const [zoom, setZoom] = useState<number>(loadTextZoom);

  const [fonts, setFonts] = useState(loadFonts);
  const updateFont = useCallback((role: FontRole, family: string) => {
    setFonts((current) => ({ ...current, [role]: family }));
    saveFont(role, family);
  }, []);
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--font-ui", fontStack(fonts.ui, "var(--font-ui-default)"));
    root.style.setProperty(
      "--font-translation",
      fontStack(fonts.translation, "var(--font-book)"),
    );
  }, [fonts]);

  // Apply presentation preferences to the document. The "system" theme is
  // resolved in CSS via prefers-color-scheme.
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.style.setProperty("--text-book", `${zoom}px`);
  }, [theme, zoom]);

  const updateTheme = useCallback((next: ThemePreference) => {
    setTheme(next);
    saveTheme(next);
  }, []);
  const updateTextSize = useCallback((size: number) => {
    const next = Math.min(28, Math.max(14, size));
    setZoom(next);
    saveTextZoom(next);
  }, []);

  const zoomIn = useCallback(() => {
    setZoom((current) => {
      const next = Math.min(28, current + 2);
      saveTextZoom(next);
      return next;
    });
  }, []);

  const zoomOut = useCallback(() => {
    setZoom((current) => {
      const next = Math.max(14, current - 2);
      saveTextZoom(next);
      return next;
    });
  }, []);

  // Global shortcuts: Cmd/Ctrl+O opens a book, Cmd/Ctrl+= / - adjusts
  // reading text size. Editor shortcuts (undo, paste) stay native.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "o" && openProject === null) {
        event.preventDefault();
        document.querySelector<HTMLButtonElement>("[data-open-epub]")?.click();
      } else if (event.key === "=" || event.key === "+") {
        event.preventDefault();
        zoomIn();
      } else if (event.key === "-") {
        event.preventDefault();
        zoomOut();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openProject, zoomIn, zoomOut]);

  if (openProject) {
    return (
      <AiJobsProvider>
        <WorkspaceFrame
          projectId={openProject.projectId}
          importWarnings={openProject.importWarnings}
          onBackToLibrary={() => setOpenProject(null)}
        />
      </AiJobsProvider>
    );
  }
  return (
    <div className="app-with-controls">
      <div className="app-controls">
        <div className="app-brand">
          <img
            className="app-brand__logo"
            src="/icon.png"
            alt=""
            width={32}
            height={32}
          />{" "}
          Sagar <span className="app-brand__descriptor">{t("Translation studio")}</span>
        </div>
      </div>
      <UpdateBanner />
      <LibraryScreen
        appearance={{
          fonts,
          onFontChange: updateFont,
          theme,
          textSize: zoom,
          onThemeChange: updateTheme,
          onTextSizeChange: updateTextSize,
        }}
        onOpenProject={(projectId, importWarnings) =>
          setOpenProject({ projectId, importWarnings })
        }
      />
    </div>
  );
}
