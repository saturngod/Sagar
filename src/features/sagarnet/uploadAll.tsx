import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  loadLink,
  recordUpload,
  type SagarNetLink,
  type SagarNetVisibility,
  sagarnetRequest,
} from "../../lib/sagarnet/link";
import { toCommandError } from "../../lib/tauri/errors";

export type UploadPhase = "running" | "done" | "stopped" | "failed";

export type UploadFailure = { chapterId: string; title: string; message: string };

/** One local chapter and the SagarNet chapter it is linked to. */
export type UploadItem = { chapterId: string; title: string; remote: number };

/**
 * One "upload all" run to SagarNet. Chapters are uploaded one at a time in
 * reading order from their latest saved draft. Book images are inlined by the
 * Rust service; the local draft is never changed.
 */
export type UploadState = {
  projectId: string;
  projectTitle: string;
  phase: UploadPhase;
  total: number;
  processed: number;
  uploaded: number;
  unchanged: number;
  empty: number;
  failures: UploadFailure[];
  currentTitle: string;
  message: string;
};

export type UploadStartInput = {
  projectId: string;
  projectTitle: string;
  host: string | null;
  link: SagarNetLink;
  targetLanguage: string;
  visibility: SagarNetVisibility;
  queue: UploadItem[];
};

type UploadResult = {
  status: "uploaded" | "unchanged" | "empty";
  local_revision?: number;
  revision?: number;
  visibility?: SagarNetVisibility;
};

type UploadAllValue = {
  upload: UploadState | null;
  start: (input: UploadStartInput) => void;
  stop: () => void;
  dismiss: () => void;
};

const UploadAllContext = createContext<UploadAllValue>({
  upload: null,
  start: () => {},
  stop: () => {},
  dismiss: () => {},
});

/** Stop after this many chapters fail in a row (for example a server outage). */
const MAX_CONSECUTIVE_FAILURES = 3;

export function isUploadActive(upload: UploadState | null): boolean {
  return upload?.phase === "running";
}

export function UploadAllProvider({ children }: { children: ReactNode }) {
  const [upload, setUpload] = useState<UploadState | null>(null);
  const running = useRef(false);
  const stopped = useRef(false);

  const patch = useCallback((next: Partial<UploadState>) => {
    setUpload((current) => (current ? { ...current, ...next } : current));
  }, []);

  const run = useCallback(
    async (input: UploadStartInput) => {
      let consecutiveFailures = 0;
      let uploaded = 0;
      let unchanged = 0;
      let empty = 0;
      const failures: UploadFailure[] = [];
      for (let index = 0; index < input.queue.length; index += 1) {
        if (stopped.current) break;
        const item = input.queue[index];
        patch({ currentTitle: item.title });
        // Re-read each time: a single-chapter upload may have updated it.
        const previous = loadLink(input.host, input.projectId)?.uploads?.[item.chapterId];
        const republish =
          input.visibility === "public" && previous?.visibility !== "public";
        try {
          const result = await sagarnetRequest<UploadResult>("upload_chapter", {
            project: input.projectId,
            local_chapter: item.chapterId,
            chapter: item.remote,
            public: input.visibility === "public",
            target_language: input.targetLanguage,
            last_local_revision: republish ? null : (previous?.local ?? null),
            last_server_revision: previous?.remote ?? null,
          });
          if (result.status === "uploaded") {
            uploaded += 1;
            if (result.local_revision !== undefined && result.revision !== undefined)
              recordUpload(input.host, input.projectId, input.link, item.chapterId, {
                local: result.local_revision,
                remote: result.revision,
                visibility: result.visibility ?? "private",
              });
          } else if (result.status === "unchanged") {
            unchanged += 1;
          } else {
            empty += 1;
          }
          consecutiveFailures = 0;
        } catch (error) {
          const problem = toCommandError(error);
          failures.push({
            chapterId: item.chapterId,
            title: item.title,
            message: problem.message || "The chapter could not be uploaded.",
          });
          if (problem.code === "sharing_unauthorized") {
            patch({
              phase: "failed",
              processed: index + 1,
              uploaded,
              unchanged,
              empty,
              failures: [...failures],
              currentTitle: "",
              message: "Sign in to SagarNet again in Book Settings, then upload again.",
            });
            return;
          }
          // A conflict or a rejected chapter is specific to that chapter.
          if (problem.code !== "sharing_conflict" && problem.code !== "sharing_error")
            consecutiveFailures += 1;
        }
        patch({
          processed: index + 1,
          uploaded,
          unchanged,
          empty,
          failures: [...failures],
        });
        if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
          patch({
            phase: "failed",
            currentTitle: "",
            message: `Stopped after ${MAX_CONSECUTIVE_FAILURES} chapters failed in a row. Check your connection, then upload again.`,
          });
          return;
        }
      }
      patch({
        phase: stopped.current ? "stopped" : "done",
        currentTitle: "",
        message: "",
      });
    },
    [patch],
  );

  const start = useCallback(
    (input: UploadStartInput) => {
      if (running.current)
        throw new Error("Another book is being uploaded to SagarNet. Stop it first.");
      if (!input.queue.length) throw new Error("No chapters are ready to upload.");
      running.current = true;
      stopped.current = false;
      setUpload({
        projectId: input.projectId,
        projectTitle: input.projectTitle,
        phase: "running",
        total: input.queue.length,
        processed: 0,
        uploaded: 0,
        unchanged: 0,
        empty: 0,
        failures: [],
        currentTitle: "",
        message: "",
      });
      void run(input).finally(() => {
        running.current = false;
      });
    },
    [run],
  );

  const stop = useCallback(() => {
    stopped.current = true;
  }, []);

  const dismiss = useCallback(() => {
    if (running.current) return;
    setUpload(null);
  }, []);

  const value = useMemo<UploadAllValue>(
    () => ({ upload, start, stop, dismiss }),
    [upload, start, stop, dismiss],
  );
  return <UploadAllContext.Provider value={value}>{children}</UploadAllContext.Provider>;
}

export function useUploadAll(): UploadAllValue {
  return useContext(UploadAllContext);
}

/** Progress line, e.g. "3 of 12 · Chapter 3" or "10 uploaded · 2 unchanged". */
export function uploadProgress(upload: UploadState, t: (message: string) => string) {
  if (upload.phase === "running") {
    const position = `${Math.min(upload.processed + 1, upload.total)} ${t("of")} ${upload.total}`;
    return [position, upload.currentTitle].filter(Boolean).join(" · ");
  }
  const parts = [`${upload.uploaded} ${t("uploaded")}`];
  if (upload.unchanged) parts.push(`${upload.unchanged} ${t("unchanged")}`);
  if (upload.empty) parts.push(`${upload.empty} ${t("empty")}`);
  if (upload.failures.length) parts.push(`${upload.failures.length} ${t("failed")}`);
  return parts.join(" · ");
}

export function uploadStateLabel(upload: UploadState): string {
  if (upload.phase === "running") return "Uploading all chapters to SagarNet";
  if (upload.phase === "stopped") return "Upload stopped";
  if (upload.phase === "failed") return "Upload stopped after errors";
  return "Finished uploading to SagarNet";
}
