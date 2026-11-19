import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { formatDuration } from "../../lib/duration";
import { useTranslation } from "../../lib/i18n";
import { formatDirection } from "../../lib/languages";
import {
  cancelAiTranslation,
  getTranslationSelection,
  listAiProviders,
  startAiTranslation,
} from "../../lib/tauri/ai";
import { getProject } from "../../lib/tauri/commands";
import { CommandError } from "../../lib/tauri/errors";
import type {
  AiProviderList,
  AiTranslationEvent,
  TranslationSelection,
} from "../../types/ai";
import type { ProjectSummary } from "../../types/project";
import { agentLabel } from "../settings/TranslationModelSection";
import { AiSelectionPanel } from "./AiSelectionPanel";
import { ApiRequestDetails } from "./ApiRequestDetails";
import { useAiJobs } from "./aiJobs";
import { hasContent, sanitizePreviewHtml } from "./richText";
import type { useChapterEditor } from "./useChapterEditor";

type Phase = "idle" | "running" | "proposal" | "failed";

/** Silence before a running job shows a stall warning (job keeps running). */
const STALL_WARNING_AFTER_SECS = 120;

function plainText(html: string): string {
  return new DOMParser().parseFromString(html, "text/html").body.textContent ?? "";
}

/**
 * AI proposal dialog: confirms the agent/model/direction and consent, then
 * streams the tool's output live while it translates. Proposals keep the
 * original HTML structure with only text translated. Empty drafts receive the
 * result straight into the Rich editor; anything with content previews
 * read-only and enters the draft only on explicit Insert — reviewed or
 * existing manual text is never overwritten silently.
 */
export function AiGenerateDialog({
  projectId,
  chapterId,
  chapterTitle,
  editor,
  onClose,
}: {
  projectId: string;
  chapterId: string;
  chapterTitle: string;
  editor: ReturnType<typeof useChapterEditor>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { jobs, trackChapterJob, setForeground, markInserted } = useAiJobs();
  const [providers, setProviders] = useState<AiProviderList | null>(null);
  const [selection, setSelection] = useState<TranslationSelection | null>(null);
  const [project, setProject] = useState<ProjectSummary | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  const [currentInstructions, setCurrentInstructions] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [proposal, setProposal] = useState<string | null>(null);
  const [streamed, setStreamed] = useState("");
  const [apiTrace, setApiTrace] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmInsert, setConfirmInsert] = useState(false);
  const [copied, setCopied] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [stalledSecs, setStalledSecs] = useState(0);
  const [eventsDead, setEventsDead] = useState(false);
  const [mode, setMode] = useState<"whole" | "selected">("whole");
  const [selectedBusy, setSelectedBusy] = useState(false);
  const appendPending = useRef(false);
  appendPending.current = editor.appending ?? false;
  const lastOutputAt = useRef<number>(Date.now());
  const jobId = useRef<string | null>(null);
  /** The run this dialog owns, even after it reached a terminal state. */
  const ownedJobId = useRef<string | null>(null);
  const starting = useRef(false);
  /** Runs this dialog already saw finish; never re-adopt them as running. */
  const finishedJobIds = useRef(new Set<string>());
  const closed = useRef(false);
  const pendingEvents = useRef<AiTranslationEvent[]>([]);
  const receiveEvent = useRef<(event: AiTranslationEvent) => void>(() => {});
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const streamRef = useRef<HTMLTextAreaElement | null>(null);
  // The event listener outlives renders; refs keep it on the latest editor.
  const editorRef = useRef(editor);
  editorRef.current = editor;
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => {
      previousFocus.current?.focus();
    };
  }, []);

  const handleClose = useCallback(() => {
    if (appendPending.current) return;
    closed.current = true;
    // Closing never destroys a run: an in-flight translation keeps working in
    // the background until it finishes or the user cancels it.
    closeRef.current();
  }, []);

  /** Leave the run working in the background and return to the workspace. */
  const handleBackground = useCallback(() => {
    handleClose();
  }, [handleClose]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        handleClose();
      }
      if (event.key === "Tab") {
        const controls = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), summary, [tabindex="0"]',
          ) ?? [],
        );
        const visibleControls = controls.filter((control) => {
          if (control.closest("[hidden]")) return false;
          for (
            let parent = control.parentElement;
            parent;
            parent = parent.parentElement
          ) {
            if (
              parent.tagName === "DETAILS" &&
              !parent.hasAttribute("open") &&
              parent.querySelector(":scope > summary") !== control
            )
              return false;
          }
          return true;
        });
        const first = visibleControls[0];
        const last = visibleControls[visibleControls.length - 1];
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
  }, [handleClose]);

  useEffect(() => {
    let cancelled = false;
    Promise.all([listAiProviders(), getTranslationSelection(), getProject(projectId)])
      .then(([list, saved, current]) => {
        if (cancelled) return;
        setProviders(list);
        setSelection(saved);
        setProject(current);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setLoadError(
          error instanceof CommandError
            ? error.message
            : "AI settings could not be loaded.",
        );
      });
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  // Own exactly one job: events for other chapters are ignored. Closing the
  // dialog hands the run to the background instead of cancelling it.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const handleEvent = (payload: AiTranslationEvent) => {
      if (payload.chapterId !== chapterId || payload.jobId !== jobId.current) {
        return;
      }
      if (payload.kind === "started") {
        // The worker is alive; the elapsed ticker shows progress.
        return;
      }
      if (payload.kind === "status") {
        setApiTrace((messages) =>
          messages.length < 20
            ? [...messages, payload.message]
            : [...messages.slice(0, 3), ...messages.slice(-16), payload.message],
        );
        return;
      }
      if (payload.kind === "stream") {
        setStreamed(payload.partialText ?? "");
        lastOutputAt.current = Date.now();
        setStalledSecs(0);
        // Follow the live output; runs after paint so the new text is laid out.
        requestAnimationFrame(() => {
          if (streamRef.current) {
            streamRef.current.scrollTop = streamRef.current.scrollHeight;
          }
        });
        return;
      }
      finishedJobIds.current.add(payload.jobId);
      if (payload.partialText != null) setStreamed(payload.partialText);
      if (payload.kind === "done" && payload.proposalHtml) {
        const current = editorRef.current;
        const emptyDraft =
          current.draft &&
          !current.draft.conflict &&
          !hasContent(current.draft.html ?? "");
        jobId.current = null;
        if (emptyDraft) {
          // Fresh chapters go straight into the Rich editor; anything with
          // content stays for explicit Insert confirmation below.
          current.setHtml(payload.proposalHtml);
          markInserted(payload.jobId);
          closeRef.current();
          return;
        }
        setProposal(payload.proposalHtml);
        setPhase("proposal");
        setNotice(null);
        setConfirmInsert(false);
      } else if (payload.kind === "cancelled") {
        setPhase("idle");
        setNotice("The run was cancelled. Nothing was inserted.");
      } else {
        setPhase("failed");
        setNotice(payload.message);
      }
      jobId.current = null;
    };
    receiveEvent.current = handleEvent;
    listen<AiTranslationEvent>("ai-translation-event", (event) => {
      if (starting.current && !jobId.current && event.payload.chapterId === chapterId) {
        pendingEvents.current.push(event.payload);
      } else {
        handleEvent(event.payload);
      }
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        if (!disposed) {
          setEventsDead(true);
        }
      });
    return () => {
      disposed = true;
      receiveEvent.current = () => {};
      unlisten?.();
      if (ownedJobId.current) {
        // The run continues in the background; the status bar keeps it visible
        // and the finished proposal stays available for review.
        setForeground(ownedJobId.current, false);
        ownedJobId.current = null;
      }
      jobId.current = null;
      setPhase("idle");
    };
  }, [chapterId, markInserted, setForeground]);

  // Elapsed + stall tickers while a run is in flight; translation can take
  // up to 30 minutes, and slow connections can stall output for minutes.
  useEffect(() => {
    if (phase !== "running") {
      setElapsed(0);
      setStalledSecs(0);
      return;
    }
    setElapsed(0);
    setStalledSecs(0);
    const timer = setInterval(() => {
      setElapsed((value) => value + 1);
      setStalledSecs(Math.floor((Date.now() - lastOutputAt.current) / 1000));
    }, 1000);
    return () => clearInterval(timer);
  }, [phase]);

  // Re-open a run that is already working in the background for this chapter
  // and pick up a finished proposal that is waiting for review.
  useEffect(() => {
    const active = jobs.find(
      (job) =>
        job.kind === "chapter" &&
        job.chapterId === chapterId &&
        job.state === "running" &&
        !finishedJobIds.current.has(job.jobId),
    );
    if (active && !jobId.current && !starting.current) {
      jobId.current = active.jobId;
      ownedJobId.current = active.jobId;
      setForeground(active.jobId, true);
      setPhase("running");
    }
    const finished = jobs.find(
      (job) =>
        job.kind === "chapter" &&
        job.chapterId === chapterId &&
        job.state === "done" &&
        job.proposalHtml &&
        !job.inserted,
    );
    if (finished?.proposalHtml && phase !== "proposal" && !proposal) {
      // The dialog owns the review now; the draft stays untouched until the
      // user inserts the proposal.
      setProposal(finished.proposalHtml);
      setPhase("proposal");
      setNotice(null);
      ownedJobId.current = finished.jobId;
      setForeground(finished.jobId, true);
    }
  }, [chapterId, jobs, phase, proposal, setForeground]);

  const handleGenerate = useCallback(async () => {
    if (!selection || !consent || jobId.current || starting.current) {
      return;
    }
    starting.current = true;
    pendingEvents.current = [];
    lastOutputAt.current = Date.now();
    setStalledSecs(0);
    setPhase("running");
    setApiTrace([]);
    setNotice(null);
    setProposal(null);
    const previousOutput = streamed;
    setConfirmInsert(false);
    setCopied(false);
    try {
      const started = await startAiTranslation({
        chapterId,
        agent: selection.agent,
        model: selection.model,
        consent: true,
        previousOutput,
        currentInstructions,
        reasoningEffort: selection.reasoningEffort,
      });
      starting.current = false;
      const detached = closed.current || !mounted.current;
      trackChapterJob({
        jobId: started.jobId,
        projectId,
        chapterId,
        chapterTitle,
        mode: t("Whole chapter"),
        foreground: !detached,
      });
      if (detached) {
        // The dialog closed while the run was starting: it keeps working in
        // the background and reports through the status bar.
        return;
      }
      jobId.current = started.jobId;
      ownedJobId.current = started.jobId;
      for (const event of pendingEvents.current) receiveEvent.current(event);
      pendingEvents.current = [];
    } catch (error) {
      starting.current = false;
      pendingEvents.current = [];
      if (closed.current || !mounted.current) return;
      setPhase("failed");
      setNotice(
        error instanceof CommandError ? error.message : "The proposal could not start.",
      );
    }
  }, [
    chapterId,
    chapterTitle,
    consent,
    currentInstructions,
    projectId,
    selection,
    streamed,
    t,
    trackChapterJob,
  ]);

  const handleCancel = useCallback(() => {
    if (!jobId.current) {
      return;
    }
    const running = jobId.current;
    void cancelAiTranslation(running).catch(() => {
      // The worker emits the terminal event; unknown jobs are already gone.
    });
  }, []);

  const handleInsert = useCallback(() => {
    if (!proposal || !editor.draft || editor.draft.conflict) {
      return;
    }
    if (hasContent(editor.draft.html) && !confirmInsert) {
      setConfirmInsert(true);
      return;
    }
    editor.setHtml(proposal);
    if (ownedJobId.current) markInserted(ownedJobId.current);
    setConfirmInsert(false);
    onClose();
  }, [confirmInsert, editor, markInserted, onClose, proposal]);

  const handleCopy = useCallback(() => {
    if (!proposal) {
      return;
    }
    const text = plainText(proposal);
    if (typeof navigator.clipboard?.writeText !== "function") {
      setNotice("Copying is not available in this view.");
      return;
    }
    navigator.clipboard
      .writeText(text)
      .then(() => setCopied(true))
      .catch(() => setNotice("The proposal could not be copied."));
  }, [proposal]);

  const running = phase === "running";
  const safeProposal = useMemo(
    () => (proposal ? sanitizePreviewHtml(proposal) : ""),
    [proposal],
  );

  return (
    <div className="dialog-backdrop">
      <div
        className={
          mode === "selected"
            ? "dialog dialog--selection ai-generation"
            : "dialog ai-generation"
        }
        role="dialog"
        aria-modal="true"
        aria-labelledby="ai-generate-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <div className="panel__header">
          <h2 className="panel__heading" id="ai-generate-title">
            {t("AI translation proposal")}
          </h2>
          <button
            type="button"
            className="icon-button"
            aria-label={
              !editor.appending && (running || selectedBusy)
                ? t("Close and keep translating in the background")
                : t("Close")
            }
            title={
              editor.appending
                ? "Wait for results to finish saving"
                : running || selectedBusy
                  ? t("Close and keep translating in the background")
                  : t("Close")
            }
            disabled={editor.appending}
            onClick={handleClose}
          >
            <Icon name="close" />
          </button>
        </div>

        <div className="ai-generation__bar">
          <fieldset className="tabs ai-mode" aria-label={t("Translation mode")}>
            <button
              type="button"
              className="tab"
              aria-pressed={mode === "whole"}
              disabled={running || selectedBusy || editor.appending}
              onClick={() => setMode("whole")}
            >
              <Icon name="book" />
              {t("Whole chapter")}
            </button>
            <button
              type="button"
              className="tab"
              aria-pressed={mode === "selected"}
              disabled={running || selectedBusy || editor.appending}
              onClick={() => setMode("selected")}
            >
              <Icon name="paragraph" />
              {t("Selected paragraphs")}
            </button>
          </fieldset>

          {selection && providers ? (
            <div className="ai-config-summary" role="status">
              <span>
                <Icon name="sparkles" />
                <strong>
                  {agentLabel(selection.agent, providers.cli, providers.custom)}
                </strong>{" "}
                · {selection.model} ·{" "}
                {selection.reasoningEffort ?? t("Default (provider setting)")}
              </span>
              {project ? (
                <span>
                  <Icon name="languages" />
                  {formatDirection(project.sourceLanguage, project.targetLanguage)}
                </span>
              ) : null}
            </div>
          ) : null}
        </div>

        {mode === "selected" ? (
          <AiSelectionPanel
            key={chapterId}
            chapterId={chapterId}
            chapterTitle={chapterTitle}
            projectId={projectId}
            selection={selection}
            currentInstructions={currentInstructions}
            onInstructions={(value) => {
              setCurrentInstructions(value);
              // Changed instructions restart a whole-chapter Retry.
              setStreamed("");
            }}
            editor={editor}
            onBusy={setSelectedBusy}
            onBackground={handleBackground}
          />
        ) : loadError ? (
          <StatusNotice kind="error">{loadError}</StatusNotice>
        ) : !providers ? (
          <StatusNotice kind="loading">{t("Loading AI settings…")}</StatusNotice>
        ) : !selection ? (
          <StatusNotice kind="info">
            {t(
              "No translation agent selected yet. Choose one in Library → AI settings → Translation Model.",
            )}
          </StatusNotice>
        ) : (
          <>
            {phase === "idle" || phase === "failed" ? (
              <>
                <label className="form-field">
                  <span>{t("Instructions for this run (optional)")}</span>
                  <textarea
                    rows={3}
                    maxLength={20000}
                    value={currentInstructions}
                    onChange={(event) => {
                      setCurrentInstructions(event.target.value);
                      setStreamed("");
                    }}
                    placeholder={t("Additional guidance for this chapter…")}
                  />
                </label>
                <div className="ai-generation__start">
                  <label className="export-ack">
                    <input
                      type="checkbox"
                      checked={consent}
                      onChange={(event) => setConsent(event.target.checked)}
                    />
                    <span>
                      {t("Send this chapter’s text to")}{" "}
                      <strong>
                        {agentLabel(selection.agent, providers.cli, providers.custom)}
                      </strong>{" "}
                      {selection.agent.startsWith("custom:")
                        ? t("via API")
                        : t("via installed tool")}
                      .
                    </span>
                  </label>
                  {phase === "failed" && streamed ? (
                    <p className="dialog__muted">
                      {t(
                        "Retry keeps complete sections whose formatting matches the source and translates the rest. An unfinished section starts again. If no complete section can be kept, the chapter starts again. A completed result is revalidated without translating again.",
                      )}
                    </p>
                  ) : null}
                  <div className="button-row">
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={!consent || editor.draft?.conflict === true}
                      onClick={() => void handleGenerate()}
                    >
                      <Icon name="refresh" />
                      {phase === "failed" ? t("Retry") : t("Generate")}
                    </button>
                  </div>
                </div>
              </>
            ) : null}

            {running ? (
              <div className="button-row">
                <button type="button" className="btn btn--cancel" onClick={handleCancel}>
                  <Icon name="close" />
                  {t("Cancel")}
                </button>
                <button type="button" className="btn" onClick={handleBackground}>
                  <Icon name="minimize" />
                  {t("Continue in background")}
                </button>
              </div>
            ) : null}

            {eventsDead ? (
              <StatusNotice kind="error">
                {t(
                  "Live progress is unavailable. If a run never finishes, close and reopen this dialog — your draft is untouched.",
                )}
              </StatusNotice>
            ) : null}
            {running ? (
              <StatusNotice kind="loading">
                {t("Translating… (")}
                {formatDuration(elapsed)}
                {t("elapsed — this can take up to 30 minutes)")}
              </StatusNotice>
            ) : null}
            {running && stalledSecs >= STALL_WARNING_AFTER_SECS ? (
              <StatusNotice kind="info">
                {t("No answer text for")} {formatDuration(stalledSecs)}
                {t(". Your draft is safe. You can wait or Cancel.")}
                {selection.agent.startsWith("custom:")
                  ? " Open API request details below to check the response and reasoning activity."
                  : " The tool has not returned more answer text yet."}
              </StatusNotice>
            ) : null}
            <ApiRequestDetails messages={apiTrace} />
            {running || streamed !== "" ? (
              <textarea
                ref={streamRef}
                className="ai-stream"
                aria-label={t("Live translation output")}
                readOnly
                value={streamed}
              />
            ) : null}
            {phase === "failed" && notice ? (
              <StatusNotice kind="error">{notice}</StatusNotice>
            ) : null}
            {phase !== "failed" && notice ? (
              <StatusNotice kind="info">{notice}</StatusNotice>
            ) : null}

            {proposal ? (
              <section aria-label={t("AI proposal preview")}>
                <div
                  className="ai-proposal"
                  // Tool output may contain arbitrary or malformed HTML. The
                  // raw result is saved only on Insert; preview is scrubbed.
                  // biome-ignore lint/security/noDangerouslySetInnerHtml: sanitized preview-only fragment
                  dangerouslySetInnerHTML={{ __html: safeProposal }}
                />
                <div className="button-row">
                  <button
                    type="button"
                    className={confirmInsert ? "btn btn--danger" : "btn btn--primary"}
                    disabled={!editor.draft || editor.draft.conflict}
                    onClick={handleInsert}
                  >
                    <Icon name="check" />
                    {confirmInsert ? "Confirm replace draft" : "Insert into draft"}
                  </button>
                  <button type="button" className="btn" onClick={handleCopy}>
                    <Icon name="copy" />
                    {copied ? "Copied" : "Copy text"}
                  </button>
                </div>
                {confirmInsert ? (
                  <p className="dialog__muted">
                    {t(
                      "Your current draft has content. Inserting replaces it (kept as an unreviewed draft, never marked reviewed).",
                    )}
                  </p>
                ) : null}
              </section>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
