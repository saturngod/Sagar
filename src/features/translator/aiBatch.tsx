import { listen } from "@tauri-apps/api/event";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  cancelAiTranslation,
  getTranslationSelection,
  startAiTranslation,
} from "../../lib/tauri/ai";
import { getChapter, listChapters, saveChapterDocument } from "../../lib/tauri/commands";
import { isBusyError } from "../../lib/tauri/errors";
import type { AiTranslationEvent } from "../../types/ai";
import type { ChapterSummary } from "../../types/project";
import { hasContent } from "./richText";

export type BatchPhase = "running" | "waiting" | "done" | "stopped" | "failed";

export type BatchFailure = { chapterId: string; title: string; message: string };

/**
 * One "translate all untranslated chapters" run. Chapters are translated one
 * at a time, in reading order, with a pause between them. Each result is saved
 * as an unreviewed draft only when the chapter is still empty, so manual work
 * is never overwritten.
 */
export type BatchState = {
  projectId: string;
  projectTitle: string;
  phase: BatchPhase;
  total: number;
  /** Chapters finished in this run (translated, failed or skipped). */
  processed: number;
  translated: number;
  skipped: number;
  failures: BatchFailure[];
  currentChapterId: string | null;
  currentTitle: string;
  chapterStartedAt: number | null;
  /** Epoch ms when the pause before the next chapter ends. */
  waitUntil: number | null;
  delaySecs: number;
  message: string;
};

export type BatchStartInput = {
  projectId: string;
  projectTitle: string;
  delaySecs: number;
  /** Optional inclusive range of untranslated chapters (by id). */
  fromChapterId?: string;
  toChapterId?: string;
};

type AiBatchValue = {
  batch: BatchState | null;
  start: (input: BatchStartInput) => Promise<void>;
  stop: () => void;
  dismiss: () => void;
  /** Latest chapter saved by the batch, so an open workspace can reload it. */
  saved: { chapterId: string; seq: number } | null;
};

const AiBatchContext = createContext<AiBatchValue>({
  batch: null,
  start: async () => {},
  stop: () => {},
  dismiss: () => {},
  saved: null,
});

/** Stop after this many chapters fail in a row (for example a broken provider). */
const MAX_CONSECUTIVE_FAILURES = 3;
const MAX_BUSY_RETRIES = 3;

/** Chapters with text that have no translation yet. */
export function untranslatedChapters(chapters: ChapterSummary[]): ChapterSummary[] {
  return chapters.filter(
    (chapter) => chapter.status === "not_started" && chapter.totalBlocks > 0,
  );
}

/** Inclusive slice of `chapters` between the from and to ids (either may be omitted). */
export function chaptersInRange(
  chapters: ChapterSummary[],
  fromId?: string,
  toId?: string,
): ChapterSummary[] {
  const from = fromId ? chapters.findIndex((chapter) => chapter.id === fromId) : 0;
  const to = toId
    ? chapters.findIndex((chapter) => chapter.id === toId)
    : chapters.length - 1;
  if (from < 0 || to < 0) return [];
  return chapters.slice(from, to + 1);
}

function sleep(ms: number, signal: { stopped: boolean }): Promise<void> {
  return new Promise((resolve) => {
    const started = Date.now();
    const timer = setInterval(() => {
      if (signal.stopped || Date.now() - started >= ms) {
        clearInterval(timer);
        resolve();
      }
    }, 200);
  });
}

function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}

export function AiBatchProvider({ children }: { children: ReactNode }) {
  const [batch, setBatch] = useState<BatchState | null>(null);
  const [saved, setSaved] = useState<{ chapterId: string; seq: number } | null>(null);
  const running = useRef(false);
  const signal = useRef<{ stopped: boolean }>({ stopped: false });
  const currentJob = useRef<string | null>(null);

  const patch = useCallback((next: Partial<BatchState>) => {
    setBatch((current) => (current ? { ...current, ...next } : current));
  }, []);

  /** Starts one chapter run and resolves with its terminal event. */
  const translateChapter = useCallback(
    async (
      chapterId: string,
      selection: { agent: string; model: string; reasoningEffort: string | null },
    ): Promise<AiTranslationEvent> => {
      let jobId: string | null = null;
      const early: AiTranslationEvent[] = [];
      let settle: (event: AiTranslationEvent) => void = () => {};
      const finished = new Promise<AiTranslationEvent>((resolve) => {
        settle = resolve;
      });
      const terminal = (event: AiTranslationEvent) =>
        event.kind === "done" || event.kind === "failed" || event.kind === "cancelled";
      const unlisten = await listen<AiTranslationEvent>(
        "ai-translation-event",
        ({ payload }) => {
          if (payload.chapterId !== chapterId) return;
          if (!jobId) {
            early.push(payload);
            return;
          }
          if (payload.jobId === jobId && terminal(payload)) settle(payload);
        },
      );
      try {
        const started = await startAiTranslation({
          chapterId,
          agent: selection.agent,
          model: selection.model,
          consent: true,
          reasoningEffort: selection.reasoningEffort,
        });
        jobId = started.jobId;
        currentJob.current = jobId;
        // A fast provider may finish before the start call returns.
        const done = early.find((event) => event.jobId === jobId && terminal(event));
        if (done) settle(done);
        // Stop pressed while the run was starting.
        if (signal.current.stopped) void cancelAiTranslation(jobId).catch(() => {});
        return await finished;
      } finally {
        currentJob.current = null;
        unlisten();
      }
    },
    [],
  );

  /** Saves the proposal only if the chapter is still untranslated. */
  const saveProposal = useCallback(
    async (projectId: string, chapterId: string, html: string): Promise<boolean> => {
      for (let attempt = 0; ; attempt += 1) {
        try {
          const content = await getChapter(projectId, chapterId);
          if (hasContent(content.document.html ?? "")) return false;
          await saveChapterDocument(chapterId, html, "draft", content.document.revision);
          return true;
        } catch (error) {
          if (isBusyError(error) && attempt < MAX_BUSY_RETRIES) {
            await sleep(400 * (attempt + 1), { stopped: false });
            continue;
          }
          if (
            typeof error === "object" &&
            error !== null &&
            "code" in error &&
            error.code === "revision_conflict"
          )
            return false;
          throw error;
        }
      }
    },
    [],
  );

  const run = useCallback(
    async (
      input: BatchStartInput,
      queue: ChapterSummary[],
      selection: { agent: string; model: string; reasoningEffort: string | null },
    ) => {
      let consecutiveFailures = 0;
      let translated = 0;
      let skipped = 0;
      const failures: BatchFailure[] = [];
      for (let index = 0; index < queue.length; index += 1) {
        if (signal.current.stopped) break;
        const chapter = queue[index];
        patch({
          phase: "running",
          currentChapterId: chapter.id,
          currentTitle: chapter.title,
          chapterStartedAt: Date.now(),
          waitUntil: null,
          message: "",
        });
        let worked = false;
        try {
          // The user may have translated this chapter since the run began.
          const content = await getChapter(input.projectId, chapter.id);
          if (hasContent(content.document.html ?? "")) {
            skipped += 1;
          } else {
            worked = true;
            const event = await translateChapter(chapter.id, selection);
            if (event.kind === "cancelled") {
              signal.current.stopped = true;
              break;
            }
            if (event.kind === "done" && event.proposalHtml) {
              if (await saveProposal(input.projectId, chapter.id, event.proposalHtml)) {
                translated += 1;
                setSaved((current) => ({
                  chapterId: chapter.id,
                  seq: (current?.seq ?? 0) + 1,
                }));
              } else {
                skipped += 1;
              }
              consecutiveFailures = 0;
            } else {
              throw new Error(event.message || "The translation did not finish.");
            }
          }
        } catch (error) {
          consecutiveFailures += 1;
          failures.push({
            chapterId: chapter.id,
            title: chapter.title,
            message: messageOf(error, "The chapter could not be translated."),
          });
        }
        patch({
          processed: index + 1,
          translated,
          skipped,
          failures: [...failures],
        });
        if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
          patch({
            phase: "failed",
            currentChapterId: null,
            chapterStartedAt: null,
            waitUntil: null,
            message: `Stopped after ${MAX_CONSECUTIVE_FAILURES} chapters failed in a row. Check AI settings, then start again.`,
          });
          return;
        }
        const last = index === queue.length - 1;
        if (worked && !last && !signal.current.stopped && input.delaySecs > 0) {
          const ms = input.delaySecs * 1000;
          patch({
            phase: "waiting",
            currentChapterId: null,
            chapterStartedAt: null,
            waitUntil: Date.now() + ms,
          });
          await sleep(ms, signal.current);
        }
      }
      patch({
        phase: signal.current.stopped ? "stopped" : "done",
        currentChapterId: null,
        chapterStartedAt: null,
        waitUntil: null,
        message: "",
      });
    },
    [patch, saveProposal, translateChapter],
  );

  const start = useCallback(
    async (input: BatchStartInput) => {
      if (running.current) {
        throw new Error("Another book is already being translated. Stop it first.");
      }
      running.current = true;
      try {
        const selection = await getTranslationSelection();
        if (!selection) {
          throw new Error(
            "No translation agent selected yet. Choose one in Library → AI settings → Translation Model.",
          );
        }
        const queue = chaptersInRange(
          untranslatedChapters(await listChapters(input.projectId)),
          input.fromChapterId,
          input.toChapterId,
        );
        if (!queue.length) throw new Error("Every chapter already has a translation.");
        signal.current = { stopped: false };
        setBatch({
          projectId: input.projectId,
          projectTitle: input.projectTitle,
          phase: "running",
          total: queue.length,
          processed: 0,
          translated: 0,
          skipped: 0,
          failures: [],
          currentChapterId: null,
          currentTitle: "",
          chapterStartedAt: null,
          waitUntil: null,
          delaySecs: input.delaySecs,
          message: "",
        });
        void run(input, queue, selection).finally(() => {
          running.current = false;
        });
      } catch (error) {
        running.current = false;
        throw error;
      }
    },
    [run],
  );

  const stop = useCallback(() => {
    signal.current.stopped = true;
    const job = currentJob.current;
    if (job) void cancelAiTranslation(job).catch(() => {});
  }, []);

  const dismiss = useCallback(() => {
    if (running.current) return;
    setBatch(null);
  }, []);

  const value = useMemo<AiBatchValue>(
    () => ({ batch, start, stop, dismiss, saved }),
    [batch, start, stop, dismiss, saved],
  );
  return <AiBatchContext.Provider value={value}>{children}</AiBatchContext.Provider>;
}

export function useAiBatch(): AiBatchValue {
  return useContext(AiBatchContext);
}

/** Ticks once a second while `active`, for elapsed and countdown labels. */
export function useNow(active: boolean): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}
