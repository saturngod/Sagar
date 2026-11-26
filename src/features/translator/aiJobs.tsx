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
import { cancelAiTranslation } from "../../lib/tauri/ai";
import { controlAiSelectionRun } from "../../lib/tauri/aiSelection";
import type { AiTranslationEvent } from "../../types/ai";
import type { SelectionEvent } from "../../types/aiSelection";

export type AiJobKind = "chapter" | "selection";
export type AiJobState = "running" | "done" | "failed" | "paused" | "cancelled";

/**
 * One AI translation run that may keep working while the user reads, writes
 * or switches chapters. The Rust worker owns the run; this record only
 * mirrors its progress so the app can show it outside the dialog.
 */
export type AiJob = {
  jobId: string;
  kind: AiJobKind;
  projectId: string;
  chapterId: string;
  chapterTitle: string;
  /** Short label such as "Whole chapter" or "12 selected items". */
  mode: string;
  state: AiJobState;
  startedAt: number;
  elapsedSecs: number;
  message: string;
  /** Whole-chapter proposal, kept until it is inserted or dismissed. */
  proposalHtml: string | null;
  /** Saved selection run id (selected paragraphs mode). */
  runId: string | null;
  /** The proposal already reached the chapter draft. */
  inserted: boolean;
  /** An open dialog owns the run; the background bar stays out of the way. */
  foreground: boolean;
  /**
   * Draft HTML an Improve run started from. The result replaces the draft
   * automatically only while the draft still matches it.
   */
  improveBase: string | null;
};

export type AiJobInput = {
  jobId: string;
  projectId: string;
  chapterId: string;
  chapterTitle: string;
  mode: string;
  runId?: string | null;
  foreground?: boolean;
  improveBase?: string | null;
};

type AiJobsValue = {
  jobs: AiJob[];
  trackChapterJob: (input: AiJobInput) => void;
  trackSelectionJob: (input: AiJobInput) => void;
  setForeground: (jobId: string, foreground: boolean) => void;
  markInserted: (jobId: string) => void;
  dismissJob: (jobId: string) => void;
  cancelJob: (jobId: string) => void;
};

const AiJobsContext = createContext<AiJobsValue>({
  jobs: [],
  trackChapterJob: () => {},
  trackSelectionJob: () => {},
  setForeground: () => {},
  markInserted: () => {},
  dismissJob: () => {},
  cancelJob: () => {},
});

/** Events that arrive before their job is registered (fast local providers). */
const MAX_BUFFERED_EVENTS = 20;

function elapsed(startedAt: number, now: number): number {
  return Math.max(0, Math.floor((now - startedAt) / 1000));
}

export function AiJobsProvider({ children }: { children: ReactNode }) {
  const [jobs, setJobs] = useState<AiJob[]>([]);
  const jobsRef = useRef<AiJob[]>([]);
  jobsRef.current = jobs;
  /** Events that arrive before their job is registered (fast local providers). */
  const buffered = useRef(new Map<string, AiTranslationEvent[]>());
  const bufferedRuns = useRef(new Map<string, SelectionEvent[]>());
  /**
   * Registered ids, updated synchronously in `track`. `jobsRef` only refreshes
   * on render, so an instant failure (for example HTTP 503) that lands between
   * registration and the next render would otherwise be buffered after the
   * buffer was drained and leave the run "running" forever.
   */
  const trackedJobs = useRef(new Set<string>());
  const trackedRuns = useRef(new Set<string>());

  const patchJob = useCallback((jobId: string, patch: Partial<AiJob>) => {
    setJobs((current) =>
      current.map((job) => (job.jobId === jobId ? { ...job, ...patch } : job)),
    );
  }, []);

  const applyTranslationEvent = useCallback((event: AiTranslationEvent) => {
    setJobs((current) =>
      current.map((job) => {
        if (job.jobId !== event.jobId) return job;
        if (event.kind === "done") {
          return {
            ...job,
            state: "done",
            message: event.message,
            proposalHtml: event.proposalHtml ?? job.proposalHtml,
          };
        }
        if (event.kind === "failed") {
          return { ...job, state: "failed", message: event.message };
        }
        if (event.kind === "cancelled") {
          return { ...job, state: "cancelled", message: event.message };
        }
        return job.state === "running" ? job : { ...job, state: "running" };
      }),
    );
  }, []);

  const applySelectionEvent = useCallback((event: SelectionEvent) => {
    setJobs((current) =>
      current.map((job) => {
        if (job.kind !== "selection" || job.runId !== event.runId) return job;
        if (event.kind === "done") {
          return { ...job, state: "done", message: event.message };
        }
        if (event.kind === "failed") {
          return { ...job, state: "failed", message: event.message };
        }
        if (event.kind === "paused") {
          return { ...job, state: "paused", message: event.message };
        }
        return job.state === "running" ? job : { ...job, state: "running" };
      }),
    );
  }, []);

  // Register a run and replay anything the provider already reported.
  const track = useCallback(
    (kind: AiJobKind, input: AiJobInput) => {
      const now = Date.now();
      if (kind === "chapter") trackedJobs.current.add(input.jobId);
      else if (input.runId) trackedRuns.current.add(input.runId);
      setJobs((current) => {
        const existing = current.find((job) => job.jobId === input.jobId);
        if (existing) {
          return current.map((job) =>
            job.jobId === input.jobId
              ? { ...job, foreground: input.foreground ?? true, mode: input.mode }
              : job,
          );
        }
        return [
          ...current,
          {
            jobId: input.jobId,
            kind,
            projectId: input.projectId,
            chapterId: input.chapterId,
            chapterTitle: input.chapterTitle,
            mode: input.mode,
            state: "running",
            startedAt: now,
            elapsedSecs: 0,
            message: "",
            proposalHtml: null,
            runId: input.runId ?? null,
            inserted: false,
            foreground: input.foreground ?? true,
            improveBase: input.improveBase ?? null,
          },
        ];
      });
      if (kind === "chapter") {
        const pending = buffered.current.get(input.jobId) ?? [];
        buffered.current.delete(input.jobId);
        for (const event of pending) applyTranslationEvent(event);
        return;
      }
      const runId = input.runId;
      if (!runId) return;
      const pending = bufferedRuns.current.get(runId) ?? [];
      bufferedRuns.current.delete(runId);
      for (const event of pending) applySelectionEvent(event);
    },
    [applySelectionEvent, applyTranslationEvent],
  );

  const trackChapterJob = useCallback(
    (input: AiJobInput) => track("chapter", input),
    [track],
  );
  const trackSelectionJob = useCallback(
    (input: AiJobInput) => track("selection", input),
    [track],
  );

  const setForeground = useCallback((jobId: string, foreground: boolean) => {
    setJobs((current) =>
      current.map((job) => (job.jobId === jobId ? { ...job, foreground } : job)),
    );
  }, []);

  const markInserted = useCallback(
    (jobId: string) => patchJob(jobId, { inserted: true }),
    [patchJob],
  );

  const dismissJob = useCallback((jobId: string) => {
    setJobs((current) => current.filter((job) => job.jobId !== jobId));
  }, []);

  const cancelJob = useCallback((jobId: string) => {
    const job = jobsRef.current.find((entry) => entry.jobId === jobId);
    if (!job) return;
    if (job.kind === "chapter") {
      void cancelAiTranslation(jobId).catch(() => {});
      return;
    }
    if (job.runId) {
      void controlAiSelectionRun(job.runId, "cancel", false).catch(() => {});
    }
  }, []);

  // The worker reports through app-level events, so a run stays visible even
  // when its dialog is closed.
  useEffect(() => {
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const onTranslation = (event: AiTranslationEvent) => {
      if (disposed) return;
      const known =
        trackedJobs.current.has(event.jobId) ||
        jobsRef.current.some((job) => job.jobId === event.jobId);
      if (!known) {
        const pending = buffered.current.get(event.jobId) ?? [];
        buffered.current.set(
          event.jobId,
          [...pending, event].slice(-MAX_BUFFERED_EVENTS),
        );
        return;
      }
      applyTranslationEvent(event);
    };
    const onSelection = (event: SelectionEvent) => {
      if (disposed) return;
      const known =
        trackedRuns.current.has(event.runId) ||
        jobsRef.current.some(
          (job) => job.kind === "selection" && job.runId === event.runId,
        );
      if (!known) {
        const pending = bufferedRuns.current.get(event.runId) ?? [];
        bufferedRuns.current.set(
          event.runId,
          [...pending, event].slice(-MAX_BUFFERED_EVENTS),
        );
        return;
      }
      applySelectionEvent(event);
    };
    void listen<AiTranslationEvent>("ai-translation-event", (event) =>
      onTranslation(event.payload),
    )
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch(() => {});
    void listen<SelectionEvent>("ai-selection-event", (event) =>
      onSelection(event.payload),
    )
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [applySelectionEvent, applyTranslationEvent]);

  // Keep the elapsed time of running jobs moving.
  const hasRunning = jobs.some((job) => job.state === "running");
  useEffect(() => {
    if (!hasRunning) return;
    const timer = setInterval(() => {
      const now = Date.now();
      setJobs((current) =>
        current.map((job) =>
          job.state === "running"
            ? { ...job, elapsedSecs: elapsed(job.startedAt, now) }
            : job,
        ),
      );
    }, 1000);
    return () => clearInterval(timer);
  }, [hasRunning]);

  const value = useMemo<AiJobsValue>(
    () => ({
      jobs,
      trackChapterJob,
      trackSelectionJob,
      setForeground,
      markInserted,
      dismissJob,
      cancelJob,
    }),
    [
      jobs,
      trackChapterJob,
      trackSelectionJob,
      setForeground,
      markInserted,
      dismissJob,
      cancelJob,
    ],
  );

  return <AiJobsContext.Provider value={value}>{children}</AiJobsContext.Provider>;
}

export function useAiJobs(): AiJobsValue {
  return useContext(AiJobsContext);
}
