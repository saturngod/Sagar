import { invoke } from "@tauri-apps/api/core";

export type SagarNetVisibility = "public" | "private";

/** What this device last uploaded for one local chapter. */
export type SagarNetUpload = {
  /** Local draft revision that was sent. */
  local: number;
  /** SagarNet revision returned for it. */
  remote: number;
  visibility: SagarNetVisibility;
};

/**
 * The SagarNet book and edition a local project is linked to. `exact` links
 * map chapters by reading position; other links use confirmed `chapters`.
 */
export type SagarNetLink = {
  book: number;
  edition: number;
  exact: boolean;
  chapters: Record<string, number>;
  uploads?: Record<string, SagarNetUpload>;
};

/** Sends one operation to the Rust SagarNet service. */
export function sagarnetRequest<T>(kind: string, fields: Record<string, unknown> = {}) {
  return invoke<T>("sharing_request", { operation: { kind, ...fields } });
}

export function storedValue<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function persistValue(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Preferences are a convenience. Sharing still works without them.
  }
}

export function linkKey(host: string | null | undefined, projectId: string) {
  return `sagarnet:link:${host ?? ""}:${projectId}`;
}

export function loadLink(host: string | null | undefined, projectId: string) {
  return storedValue<SagarNetLink | null>(linkKey(host, projectId), null);
}

function sameEdition(a: SagarNetLink | null, b: SagarNetLink | null) {
  return !!a && !!b && a.book === b.book && a.edition === b.edition;
}

/**
 * Saves the link. Upload records are kept while the edition is unchanged,
 * because only `recordUpload` maintains them.
 */
export function saveLink(
  host: string | null | undefined,
  projectId: string,
  link: SagarNetLink | null,
) {
  const current = loadLink(host, projectId);
  const uploads = sameEdition(current, link) ? current?.uploads : undefined;
  persistValue(linkKey(host, projectId), link ? { ...link, uploads } : null);
}

/** Remembers an upload so unchanged chapters are skipped next time. */
export function recordUpload(
  host: string | null | undefined,
  projectId: string,
  link: SagarNetLink,
  chapterId: string,
  upload: SagarNetUpload,
) {
  const current = loadLink(host, projectId);
  const base = sameEdition(current, link) && current ? current : link;
  persistValue(linkKey(host, projectId), {
    ...base,
    uploads: {
      ...(sameEdition(current, link) ? current?.uploads : undefined),
      [chapterId]: upload,
    },
  });
}

/** SagarNet chapter linked to a local chapter, if any. */
export function linkedChapter(
  link: SagarNetLink,
  local: { id: string; spineIndex: number },
  remote: { id: number; spine_position: number }[],
): number | undefined {
  return (
    link.chapters[local.id] ??
    (link.exact
      ? remote.find((chapter) => chapter.spine_position === local.spineIndex)?.id
      : undefined)
  );
}
