/** Project/chapter DTOs returned by the Rust project services. */

export type ProjectSummary = {
  id: string;
  title: string;
  author: string | null;
  sourceLanguage: string | null;
  targetLanguage: string;
  reviewedBlocks: number;
  totalBlocks: number;
  unsupportedBlocks: number;
  createdAt: string;
  updatedAt: string;
};

export type ChapterSummary = {
  navigationGroups?: { id: string; title: string; resource: string | null }[];
  id: string;
  spineIndex: number;
  title: string;
  resourceHref: string;
  status: string;
  totalBlocks: number;
  reviewedBlocks: number;
  unsupportedBlocks: number;
};

export type ImportWarning = {
  code: string;
  message: string;
  location: string | null;
};

export type ImportResult = {
  project: ProjectSummary;
  chapters: ChapterSummary[];
  warnings: ImportWarning[];
};

export type CoverImage = {
  mediaType: string;
  base64: string;
};

/** A stylesheet from the book's manifest; `href` is its stable ZIP path. */
export type StylesheetSummary = {
  href: string;
  name: string;
  size: number;
  modified: boolean;
  updatedAt: string | null;
};

export type StylesheetContent = {
  href: string;
  css: string;
  modified: boolean;
  updatedAt: string | null;
};

export type BookSettings = {
  title: string;
  author: string;
  publisher: string;
  description: string;
  sourceLanguage: string;
  targetLanguage: string;
  instructions: string;
  customCover: string | null;
  /** Selected genre translation skill, or null for none. */
  genreSkillId?: string | null;
  /** Selected target-language translation skill, or null for none. */
  languageSkillId?: string | null;
  /** Seconds to pause between chapters when translating all untranslated chapters. */
  batchDelaySecs: number;
};

export type TranslationSkillKind = "genre" | "language";

export type TranslationSkillSummary = {
  id: string;
  label: string;
  description: string;
  kind: TranslationSkillKind;
  /** Primary language subtag for language skills (for example "my"). */
  target: string | null;
  version: string;
  builtin: boolean;
};

export type TranslationSkill = TranslationSkillSummary & { body: string };

export type SkillInstallStatus =
  | "notInstalled"
  | "installed"
  | "updateAvailable"
  | "modified"
  | "conflict";

/** One coding CLI's copy of a translation skill. */
export type SkillInstall = {
  providerId: string;
  label: string;
  /** User-level skills folder, with the home folder shown as "~". */
  folder: string;
  /** True when the CLI was found on this computer. */
  detected: boolean;
  status: SkillInstallStatus;
};
