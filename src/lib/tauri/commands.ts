import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  BlockStatus,
  ChapterContent,
  ExportOutcome,
  ExportReview,
  SaveResult,
} from "../../types/chapter";
import type { HealthReport } from "../../types/health";
import type {
  ChapterSummary,
  CoverImage,
  ImportResult,
  ProjectSummary,
} from "../../types/project";
import type { AppLanguage } from "../prefs";
import { toCommandError } from "./errors";
import { preparePreviewImages } from "./preview";

/**
 * Typed wrappers around Tauri commands. Frontend code calls these instead of
 * `invoke` directly so DTOs stay typed and errors become `CommandError`.
 */

export async function healthCheck(): Promise<HealthReport> {
  try {
    return await invoke<HealthReport>("health_check");
  } catch (error) {
    throw toCommandError(error);
  }
}

/** Opens the native EPUB picker; returns the chosen path or null on cancel. */
export async function pickEpubFile(): Promise<string | null> {
  try {
    const selection = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "EPUB books", extensions: ["epub"] }],
    });
    return typeof selection === "string" ? selection : null;
  } catch (error) {
    throw toCommandError(error);
  }
}

export type ExportFormat = "epub" | "kfx";

/** Opens the native Save dialog for exports; null on cancel. */
export async function pickExportDestination(
  defaultName: string,
  format: ExportFormat = "epub",
): Promise<string | null> {
  try {
    const selection = await save({
      defaultPath: defaultName,
      filters:
        format === "kfx"
          ? [{ name: "Kindle books (KFX)", extensions: ["kfx"] }]
          : [{ name: "EPUB books", extensions: ["epub"] }],
    });
    return typeof selection === "string" ? selection : null;
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function importEpub(
  sourcePath: string,
  allowDuplicate: boolean,
  appLanguage: AppLanguage = "en",
): Promise<ImportResult> {
  try {
    return await invoke<ImportResult>("import_epub", {
      sourcePath,
      allowDuplicate,
      appLanguage,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listProjects(): Promise<ProjectSummary[]> {
  try {
    return await invoke<ProjectSummary[]>("list_projects");
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getProject(projectId: string): Promise<ProjectSummary> {
  try {
    return await invoke<ProjectSummary>("get_project", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listChapters(projectId: string): Promise<ChapterSummary[]> {
  try {
    return await invoke<ChapterSummary[]>("list_chapters", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getProjectCover(projectId: string): Promise<CoverImage | null> {
  try {
    return await invoke<CoverImage | null>("get_project_cover", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function deleteProject(projectId: string): Promise<void> {
  try {
    await invoke("delete_project", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getChapter(
  projectId: string,
  chapterId: string,
): Promise<ChapterContent> {
  try {
    const content = await invoke<ChapterContent>("get_chapter", { projectId, chapterId });
    return {
      ...content,
      previewHtml: preparePreviewImages(content.previewHtml, (path) =>
        convertFileSrc(path, "epubres"),
      ),
    };
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function saveTranslation(
  blockId: string,
  targetHtml: string | null,
  status: BlockStatus,
  expectedRevision: number,
): Promise<SaveResult> {
  try {
    return await invoke<SaveResult>("save_rich_translation", {
      blockId,
      targetHtml,
      status,
      expectedRevision,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getLastChapter(projectId: string): Promise<string | null> {
  try {
    return await invoke<string | null>("get_last_chapter", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function updateProjectMetadata(
  projectId: string,
  translatedTitle: string | null,
  targetLanguage: string | null,
  sourceLanguage: string | null = null,
): Promise<void> {
  try {
    await invoke("update_project_metadata", {
      projectId,
      translatedTitle,
      targetLanguage,
      sourceLanguage,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function reviewExport(projectId: string): Promise<ExportReview> {
  try {
    return await invoke<ExportReview>("review_export", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function exportEpub(
  projectId: string,
  destination: string,
  mode: "completed" | "draft",
  title: string | null,
  language: string | null,
  includeOriginal = true,
  format: ExportFormat = "epub",
): Promise<ExportOutcome> {
  try {
    return await invoke<ExportOutcome>("export_epub", {
      projectId,
      destination,
      mode,
      title,
      language,
      includeOriginal,
      format,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

/** Exports the untranslated source book (exact EPUB copy, or KFX). */
export async function exportOriginal(
  projectId: string,
  destination: string,
  format: ExportFormat,
): Promise<ExportOutcome> {
  try {
    return await invoke<ExportOutcome>("export_original", {
      projectId,
      destination,
      format,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function saveChapterDocument(
  chapterId: string,
  html: string | null,
  status: BlockStatus,
  expectedRevision: number,
): Promise<SaveResult> {
  try {
    return await invoke<SaveResult>("save_chapter_document", {
      chapterId,
      html,
      status,
      expectedRevision,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function validateChapterHtml(html: string): Promise<string> {
  try {
    return await invoke<string>("validate_chapter_html", { html });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getBookSettings(
  projectId: string,
): Promise<import("../../types/project").BookSettings> {
  try {
    return await invoke("get_book_settings", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function saveBookSettings(
  projectId: string,
  settings: import("../../types/project").BookSettings,
): Promise<void> {
  try {
    await invoke("save_book_settings", { projectId, settings });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listStylesheets(
  projectId: string,
): Promise<import("../../types/project").StylesheetSummary[]> {
  try {
    return await invoke("list_stylesheets", { projectId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getStylesheet(
  projectId: string,
  href: string,
): Promise<import("../../types/project").StylesheetContent> {
  try {
    return await invoke("get_stylesheet", { projectId, href });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function saveStylesheet(
  projectId: string,
  href: string,
  css: string,
): Promise<import("../../types/project").StylesheetContent> {
  try {
    return await invoke("save_stylesheet", { projectId, href, css });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function resetStylesheet(
  projectId: string,
  href: string,
): Promise<import("../../types/project").StylesheetContent> {
  try {
    return await invoke("reset_stylesheet", { projectId, href });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listTranslationSkills(): Promise<
  import("../../types/project").TranslationSkillSummary[]
> {
  try {
    return await invoke("list_translation_skills");
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function getTranslationSkill(
  skillId: string,
): Promise<import("../../types/project").TranslationSkill> {
  try {
    return await invoke("get_translation_skill", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}

/** Opens the native picker for a SKILL.md file; null on cancel. */
export async function pickSkillFile(): Promise<string | null> {
  try {
    const selection = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Skill files", extensions: ["md"] }],
    });
    return typeof selection === "string" ? selection : null;
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function getTranslationSkillSource(skillId: string): Promise<string> {
  try {
    return await invoke<string>("get_translation_skill_source", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function importTranslationSkill(
  path: string,
  replace: boolean,
): Promise<import("../../types/project").TranslationSkillSummary> {
  try {
    return await invoke("import_translation_skill", { path, replace });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function saveTranslationSkill(
  skillId: string,
  text: string,
): Promise<import("../../types/project").TranslationSkillSummary> {
  try {
    return await invoke("save_translation_skill", { skillId, text });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function duplicateTranslationSkill(
  skillId: string,
): Promise<import("../../types/project").TranslationSkillSummary> {
  try {
    return await invoke("duplicate_translation_skill", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function translationSkillUsage(skillId: string): Promise<number> {
  try {
    return await invoke<number>("translation_skill_usage", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}
/** Deletes an imported skill; returns CLIs that keep a changed copy. */
export async function deleteTranslationSkill(skillId: string): Promise<string[]> {
  try {
    return await invoke<string[]>("delete_translation_skill", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function listSkillInstalls(
  skillId: string,
): Promise<import("../../types/project").SkillInstall[]> {
  try {
    return await invoke("list_skill_installs", { skillId });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function installSkill(
  skillId: string,
  providerId: string,
  force: boolean,
): Promise<import("../../types/project").SkillInstallStatus> {
  try {
    return await invoke("install_skill", { skillId, providerId, force });
  } catch (error) {
    throw toCommandError(error);
  }
}
export async function uninstallSkill(
  skillId: string,
  providerId: string,
  force: boolean,
): Promise<import("../../types/project").SkillInstallStatus> {
  try {
    return await invoke("uninstall_skill", { skillId, providerId, force });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listSystemFonts(): Promise<string[]> {
  try {
    return await invoke<string[]>("list_system_fonts");
  } catch (error) {
    throw toCommandError(error);
  }
}
