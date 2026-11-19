/**
 * Presentation preferences (theme, text zoom, pane sizes). These are stored
 * separately from translations and never authoritative content.
 */

export type AppLanguage = "en" | "my" | "zh" | "ja" | "ko" | "th" | "shn" | "rki" | "mnw";

export type ThemePreference = "system" | "light" | "dark";

const KEYS = {
  theme: "sagar.theme",
  language: "sagar.language",
  zoom: "sagar.textZoom",
  sidebarWidth: "sagar.sidebarWidth",
  sourceWidth: "sagar.sourceWidth",
  scrollSync: "sagar.scrollSync",
  autoUpdateCheck: "sagar.autoUpdateCheck",
} as const;

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Presentation preferences are best-effort only.
  }
}

export function loadTheme(): ThemePreference {
  const value = read(KEYS.theme);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

export function saveTheme(theme: ThemePreference): void {
  write(KEYS.theme, theme);
}

export function loadTextZoom(): number {
  const value = Number(read(KEYS.zoom));
  return Number.isFinite(value) && value >= 14 && value <= 28 ? value : 18;
}

export function saveTextZoom(size: number): void {
  write(KEYS.zoom, String(size));
}

function clampWidth(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function loadSidebarWidth(): number {
  const value = Number(read(KEYS.sidebarWidth));
  return Number.isFinite(value) ? clampWidth(value, 200, 360) : 240;
}

export function saveSidebarWidth(value: number): void {
  write(KEYS.sidebarWidth, String(clampWidth(value, 200, 360)));
}

export function loadSourceWidth(): number {
  const stored = read(KEYS.sourceWidth);
  if (stored === null) return 0;
  const value = Number(stored);
  return Number.isFinite(value) ? clampWidth(value, 320, 2000) : 0;
}

export function saveSourceWidth(value: number): void {
  write(KEYS.sourceWidth, String(clampWidth(value, 320, 2000)));
}

export function loadScrollSync(): boolean {
  return read(KEYS.scrollSync) === "1";
}

export function saveScrollSync(enabled: boolean): void {
  write(KEYS.scrollSync, enabled ? "1" : "0");
}

export function loadAppLanguage(): AppLanguage {
  const value = read(KEYS.language);
  return value === "mnw" ||
    value === "shn" ||
    value === "rki" ||
    value === "my" ||
    value === "zh" ||
    value === "ja" ||
    value === "ko" ||
    value === "th"
    ? value
    : "en";
}

export function saveAppLanguage(language: AppLanguage): void {
  write(KEYS.language, language);
}

export type FontRole = "ui" | "translation";
export type FontPreferences = Record<FontRole, string>;

export function loadFonts(): FontPreferences {
  return {
    ui: read("sagar.font.ui") ?? "",
    translation: read("sagar.font.translation") ?? "",
  };
}

export function saveFont(role: FontRole, family: string): void {
  write(`sagar.font.${role}`, family);
}

/** Quote a family as a CSS string so punctuation cannot become CSS syntax. */
export function fontStack(family: string, fallback: string): string {
  const quoted = family
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/[\n\r\f]/g, " ");
  return family ? `"${quoted}", ${fallback}` : fallback;
}

/** Whether Sagar checks GitHub for a new version on launch (default on). */
export function loadAutoUpdateCheck(): boolean {
  return read(KEYS.autoUpdateCheck) !== "0";
}

export function saveAutoUpdateCheck(enabled: boolean): void {
  write(KEYS.autoUpdateCheck, enabled ? "1" : "0");
}
