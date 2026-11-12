import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import { formatDirection } from "../../lib/languages";
import {
  controlAiSelectionRun,
  deleteAiSelectionRun,
  getAiSelectionState,
  prepareAiSelectionRun,
} from "../../lib/tauri/aiSelection";
import type { TranslationSelection } from "../../types/ai";
import type { SelectionEvent, SelectionState } from "../../types/aiSelection";
import { ApiRequestDetails } from "./ApiRequestDetails";
import { hasContent } from "./richText";
import type { useChapterEditor } from "./useChapterEditor";

const PAGE_SIZE = 20;
const message = (error: unknown) =>
  error instanceof Error
    ? error.message
    : "The selected translation could not be completed.";

import {
  chapterProposals,
  ReviewRows,
  readyItems,
  SourceRows,
  stepForRun,
  type WorkflowStep,
} from "./AiSelectionViews";
import { useAiJobs } from "./aiJobs";

export function AiSelectionPanel({
  chapterId,
  chapterTitle,
  projectId,
  selection,
  bookInstructions,
  currentInstructions,
  onInstructions,
  editor,
  onBusy,
  onBackground,
}: {
  chapterId: string;
  chapterTitle: string;
  projectId: string;
  selection: TranslationSelection | null;
  bookInstructions: string;
  currentInstructions: string;
  onInstructions: (value: string) => void;
  editor: ReturnType<typeof useChapterEditor>;
  onBusy: (busy: boolean) => void;
  onBackground: () => void;
}) {
  const { t } = useTranslation();
  const { trackSelectionJob, setForeground } = useAiJobs();
  const [step, setStep] = useState<WorkflowStep>("select");
  const observedRun = useRef("");
  const bodyRef = useRef<HTMLDivElement>(null);
  const previousStep = useRef<WorkflowStep>("select");
  useEffect(() => {
    if (previousStep.current !== step) {
      previousStep.current = step;
      bodyRef.current?.querySelector<HTMLElement>("h3")?.focus();
    }
  }, [step]);
  const [data, setData] = useState<SelectionState | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [accepted, setAccepted] = useState<Set<string>>(new Set());
  const [runId, setRunId] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [stream, setStream] = useState("");
  const [trace, setTrace] = useState<string[]>([]);
  const [page, setPage] = useState(0);
  const [previewPage, setPreviewPage] = useState(0);
  const [allowAgain, setAllowAgain] = useState(false);
  const [confirmOrder, setConfirmOrder] = useState(false);
  const [orderRequired, setOrderRequired] = useState(false);
  const [appendError, setAppendError] = useState<{
    message: string;
    code: string;
  } | null>(null);
  const [appendNotice, setAppendNotice] = useState<string | null>(null);
  const [eventsUnavailable, setEventsUnavailable] = useState(false);
  const mounted = useRef(true);
  const activeRun = useRef<string | null>(null);
  const currentJob = useRef<string | null>(null);
  const starting = useRef(false);
  const bufferedEvents = useRef<SelectionEvent[]>([]);
  const receiveEvent = useRef<(event: SelectionEvent) => void>(() => {});
  const loadVersion = useRef(0);
  const appendRequest = useRef<{ key: string; id: string } | null>(null);
  const run = data?.runs.find((r) => r.id === runId) ?? null;
  const running = busy || deleting || run?.state === "running";
  const chosen = data?.units.filter((u) => selected.has(u.id)) ?? [];
  const needsProvider = chosen.some((u) => u.text.length > 0);
  const localOnly = chosen.length > 0 && !needsProvider;
  const generateLabel = localOnly
    ? chosen.every((u) => u.kind === "image" || u.kind === "figure")
      ? "Preview selected images"
      : "Preview selected content"
    : "Translate selected";

  const reload = useCallback(async () => {
    const version = ++loadVersion.current;
    const next = await getAiSelectionState(chapterId);
    if (!mounted.current || version !== loadVersion.current) return;
    setData(next);
    const available = new Set(
      next.runs.flatMap((saved) => readyItems(saved).map((item) => item.id)),
    );
    setAccepted((previous) => {
      const kept = new Set([...previous].filter((id) => available.has(id)));
      return kept.size === previous.size ? previous : kept;
    });
    setRunId((id) =>
      next.runs.some((value) => value.id === id) ? id : (next.runs[0]?.id ?? null),
    );
  }, [chapterId]);

  useEffect(() => {
    mounted.current = true;
    void reload().catch((e: unknown) => {
      if (mounted.current) setError(message(e));
    });
    let unlisten: (() => void) | undefined;
    const receive = (payload: SelectionEvent) => {
      if (
        !mounted.current ||
        payload.chapterId !== chapterId ||
        payload.runId !== activeRun.current
      )
        return;
      if (starting.current) {
        bufferedEvents.current.push(payload);
        return;
      }
      if (payload.jobId !== currentJob.current) return;
      if (payload.kind === "stream") setStream(payload.partialText ?? "");
      else if (payload.kind === "status")
        setTrace((old) => [...old.slice(-19), payload.message]);
      else {
        setNotice(payload.message);
        if (["done", "failed", "paused"].includes(payload.kind)) setBusy(false);
        void reload().catch((e: unknown) => {
          if (mounted.current) setError(message(e));
        });
      }
    };
    receiveEvent.current = receive;
    void listen<SelectionEvent>("ai-selection-event", ({ payload }) => receive(payload))
      .then((fn) => {
        if (mounted.current) unlisten = fn;
        else fn();
      })
      .catch(() => {
        if (mounted.current) setEventsUnavailable(true);
      });
    return () => {
      mounted.current = false;
      unlisten?.();
      if (currentJob.current) {
        // The run keeps working in the background: closing the dialog never
        // throws away completed work or a running queue.
        setForeground(currentJob.current, false);
        currentJob.current = null;
      }
    };
  }, [chapterId, reload, setForeground]);

  useEffect(() => {
    onBusy(running || editor.appending);
  }, [running, editor.appending, onBusy]);
  // Persisted checkpoints recover missed events and terminal-state races.
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => {
      void reload().catch((e: unknown) => {
        if (mounted.current) setError(message(e));
      });
    }, 2000);
    return () => clearInterval(timer);
  }, [running, reload]);
  useEffect(() => {
    if (run?.state === "running") activeRun.current = run.id;
    if (!starting.current && run && run.state !== "running") setBusy(false);
  }, [run]);

  const start = async () => {
    if (starting.current || running) return;
    starting.current = true;
    bufferedEvents.current = [];
    setBusy(true);
    setError(null);
    setNotice(null);
    setStream("");
    setTrace([]);
    try {
      const id = await prepareAiSelectionRun({
        chapterId,
        unitIds: [...selected],
        agent: selection?.agent ?? "",
        model: selection?.model ?? "",
        reasoningEffort: selection?.reasoningEffort ?? null,
        currentInstructions,
      });
      if (!mounted.current) return;
      activeRun.current = id;
      setRunId(id);
      setPreviewPage(0);
      const job = await controlAiSelectionRun(id, "start", consent);
      currentJob.current = job;
      trackSelectionJob({
        jobId: job,
        runId: id,
        projectId,
        chapterId,
        chapterTitle,
        mode: `${selected.size} ${t("selected items")}`,
        foreground: mounted.current,
      });
      if (!mounted.current) {
        // The dialog closed while the run was starting: it keeps working in
        // the background and the status bar follows it.
        return;
      }
      starting.current = false;
      for (const event of bufferedEvents.current) receiveEvent.current(event);
      bufferedEvents.current = [];
      setStep("progress");
      await reload();
    } catch (e) {
      starting.current = false;
      if (mounted.current) {
        setError(message(e));
        setBusy(false);
        await reload().catch(() => {});
      }
    }
  };

  const control = async (action: "start" | "retry" | "smaller" | "skip" | "cancel") => {
    if (!runId) return;
    if (action !== "cancel" && starting.current) return;
    setError(null);
    setNotice(null);
    setStream("");
    if (action !== "cancel") setBusy(true);
    if (action !== "cancel") {
      starting.current = true;
      bufferedEvents.current = [];
    }
    activeRun.current = runId;
    try {
      const job = await controlAiSelectionRun(runId, action, consent);
      if (action !== "cancel") {
        currentJob.current = job;
        trackSelectionJob({
          jobId: job,
          runId,
          projectId,
          chapterId,
          chapterTitle,
          mode: `${run?.items.length ?? selected.size} ${t("selected items")}`,
          foreground: mounted.current,
        });
      }
      if (!mounted.current) {
        // The dialog closed while the run was starting: it keeps working in
        // the background and the status bar follows it.
        return;
      }
      starting.current = false;
      for (const event of bufferedEvents.current) receiveEvent.current(event);
      bufferedEvents.current = [];
      setStep("progress");
      await reload();
    } catch (e) {
      starting.current = false;
      if (mounted.current) {
        setError(message(e));
        setBusy(false);
      }
    }
  };

  const append = async () => {
    const proposalIds = [...accepted].sort();
    const key = JSON.stringify(proposalIds);
    if (appendRequest.current?.key !== key)
      appendRequest.current = { key, id: crypto.randomUUID() };
    setError(null);
    setNotice(null);
    setAppendError(null);
    setAppendNotice(null);
    try {
      await editor.appendProposals({
        proposalIds,
        requestId: appendRequest.current.id,
        allowAgain,
        confirmOutOfOrder: confirmOrder,
      });
      if (!mounted.current) return;
      appendRequest.current = null;
      setAccepted(new Set());
      setSelected(new Set());
      setConfirmOrder(false);
      setAppendNotice(`${proposalIds.length} results added to your draft and saved.`);
      await reload();
    } catch (e) {
      if (mounted.current) {
        const code =
          typeof e === "object" && e !== null && "code" in e ? String(e.code) : "unknown";
        if (code === "append_out_of_order") setOrderRequired(true);
        setAppendError({ message: message(e), code });
      }
    }
  };

  const toggle = (id: string, checked: boolean, target: "source" | "preview") => {
    const setter = target === "source" ? setSelected : setAccepted;
    setter((old) => {
      const next = new Set(old);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };
  useEffect(() => {
    if (!run) return;
    const identity = `${run.id}:${run.state}`;
    if (observedRun.current !== identity) {
      observedRun.current = identity;
      setStep(stepForRun(run));
    }
  }, [run]);
  const proposals = chapterProposals(data?.runs ?? []);
  const ready = proposals.map(({ item }) => item);
  const runReady = readyItems(run);
  const added = new Set(data?.units.filter((u) => u.added).map((u) => u.id));
  const hasPendingText = run?.items.some(
    (i) =>
      !["ready", "skipped", "added", "superseded"].includes(i.state) &&
      data?.units.some((u) => u.id === i.unitId && u.text.length > 0),
  );

  const deleteSavedRun = async () => {
    if (!runId || running || editor.appending) return;
    setDeleting(true);
    setError(null);
    ++loadVersion.current;
    try {
      await deleteAiSelectionRun(chapterId, runId);
      if (!mounted.current) return;
      activeRun.current = null;
      currentJob.current = null;
      observedRun.current = "";
      appendRequest.current = null;
      setRunId(null);
      setAccepted(new Set());
      setSelected(new Set());
      setPreviewPage(0);
      setConfirmDelete(false);
      setConfirmOrder(false);
      setStream("");
      setTrace([]);
      setStep("select");
      setNotice("Saved proposals deleted. Your chapter draft is unchanged.");
      await reload();
    } catch (error) {
      if (mounted.current) setError(message(error));
    } finally {
      if (mounted.current) setDeleting(false);
    }
  };

  const previouslyAddedSelected = ready.some(
    (item) => accepted.has(item.id) && added.has(item.unitId),
  );
  // Consent to re-add applies only to the exact proposal selection in this run.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset confirmation when its selection or run changes
  useEffect(() => {
    setAllowAgain(false);
    setConfirmOrder(false);
    setOrderRequired(false);
    setAppendError(null);
  }, [accepted, runId]);
  const earlier =
    hasContent(editor.draft?.html ?? "") &&
    ready.some(
      (item) =>
        accepted.has(item.id) &&
        data?.units.some((unit) => unit.added && unit.order > item.order),
    );
  const needsOrderConfirmation = earlier || orderRequired;
  const placementConfirmed =
    (!previouslyAddedSelected || allowAgain) && (!needsOrderConfirmation || confirmOrder);
  const appendDisabled =
    !accepted.size ||
    running ||
    editor.appending ||
    !!editor.draft?.conflict ||
    !placementConfirmed;
  const consentBox = (
    <label className="export-ack">
      <input
        type="checkbox"
        checked={consent}
        disabled={running}
        onChange={(event) => setConsent(event.target.checked)}
      />
      <span>
        {t(
          "I understand selected text and neighboring context will be sent to the chosen provider. Image bytes are never sent.",
        )}
      </span>
    </label>
  );
  const history = data?.runs.length ? (
    <details className="ai-run-history">
      <summary>
        {t("History ·")} {data.runs.length} {t("saved runs")}
      </summary>
      <label className="form-field">
        <span>{t("Saved proposals and runs")}</span>
        <select
          value={runId ?? ""}
          disabled={running || editor.appending}
          onChange={(event) => {
            const id = event.target.value;
            setRunId(id);
            setConfirmDelete(false);
            setStep(stepForRun(data.runs.find((value) => value.id === id) ?? null));
            setPreviewPage(0);
            setStream("");
            setTrace([]);
            setError(null);
          }}
        >
          {data.runs.map((value, index) => (
            <option key={value.id} value={value.id}>
              {t("Run")} {data.runs.length - index} ·{" "}
              {value.config.model || "Images only"} · {value.state}
            </option>
          ))}
        </select>
      </label>
      {run ? (
        <>
          <p>
            {formatDirection(run.config.sourceLanguage, run.config.targetLanguage)} ·{" "}
            {run.config.model || "Images only"} ·{" "}
            {run.config.reasoningEffort ?? "Default effort"}
          </p>
          <p className="dialog__muted">
            {t("Resume and Retry use this saved configuration.")}
          </p>
          {confirmDelete ? (
            <section aria-label={t("Confirm deleting saved proposals")}>
              <p>
                {t(
                  "Delete this run and all its saved proposals? This cannot be undone. Text already added to your draft and insertion history will stay.",
                )}
              </p>
              <div className="button-row">
                <button
                  type="button"
                  className="btn"
                  disabled={running || editor.appending}
                  onClick={() => void deleteSavedRun()}
                >
                  <Icon name="trash" />
                  {deleting ? t("Deleting…") : "Delete saved proposals"}
                </button>
                <button
                  type="button"
                  className="btn"
                  disabled={deleting}
                  onClick={() => setConfirmDelete(false)}
                >
                  <Icon name="close" />
                  {t("Cancel deletion")}
                </button>
              </div>
            </section>
          ) : (
            <button
              type="button"
              className="btn"
              disabled={running || editor.appending}
              onClick={() => setConfirmDelete(true)}
            >
              <Icon name="trash" />
              {t("Delete this saved run…")}
            </button>
          )}

          <details>
            <summary>{t("Saved instructions")}</summary>
            <p className="book-instructions">
              {run.config.bookInstructions || "No book instructions"}
            </p>
            <p className="book-instructions">
              {run.config.currentInstructions || "No additional instructions"}
            </p>
          </details>
        </>
      ) : null}
    </details>
  ) : null;

  return (
    <section className="ai-selection" aria-label={t("Selected paragraphs translation")}>
      <nav className="ai-selection__steps" aria-label={t("Translation workflow")}>
        {(["select", "progress", "review"] as const).map((value, index) => (
          <button
            type="button"
            key={value}
            className="tab"
            aria-current={step === value ? "step" : undefined}
            disabled={
              editor.appending ||
              (value === "select"
                ? running
                : !run || (value === "review" && (!ready.length || running)))
            }
            onClick={() => setStep(value)}
          >
            <Icon
              name={
                value === "select"
                  ? "paragraph"
                  : value === "progress"
                    ? "sparkles"
                    : "check"
              }
            />
            {index + 1}.{" "}
            {value === "select"
              ? t("Select")
              : value === "progress"
                ? t("Translate")
                : t("Review & Add")}
          </button>
        ))}
      </nav>
      <div className="ai-selection__body" ref={bodyRef}>
        {error ? (
          <StatusNotice
            kind="error"
            onRetry={() => {
              setError(null);
              void reload().catch((e: unknown) => setError(message(e)));
            }}
          >
            {error}
          </StatusNotice>
        ) : null}
        {notice ? <StatusNotice kind="info">{notice}</StatusNotice> : null}
        {!data ? (
          error ? null : (
            <StatusNotice kind="loading">{t("Loading source selections…")}</StatusNotice>
          )
        ) : (
          <>
            {history}
            {step === "select" ? (
              <>
                <div className="ai-selection__toolbar">
                  <div className="ai-selection__heading">
                    <h3 className="visually-hidden" tabIndex={-1}>
                      {t("Select source items")}
                    </h3>
                    <span role="status">
                      {selected.size} {t("selected ·")} {data.units.length}{" "}
                      {t("source items")}
                    </span>
                  </div>
                  <div className="button-row ai-selection__batch-actions">
                    <button
                      type="button"
                      className="btn btn--small"
                      disabled={running}
                      onClick={() =>
                        setSelected(
                          new Set(
                            data.units
                              .filter((unit) => !unit.unavailable)
                              .map((unit) => unit.id),
                          ),
                        )
                      }
                    >
                      <Icon name="check" />
                      {t("Select all available")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--small"
                      disabled={running}
                      onClick={() =>
                        setSelected(
                          new Set(
                            data.units
                              .filter((unit) => !unit.unavailable && !unit.hasProposal)
                              .map((unit) => unit.id),
                          ),
                        )
                      }
                    >
                      <Icon name="paragraph" />
                      {t("Select untranslated")}
                    </button>
                    {data.units.length > PAGE_SIZE ? (
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={running}
                        onClick={() =>
                          setSelected(
                            new Set(
                              data.units
                                .slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)
                                .filter((unit) => !unit.unavailable && !unit.hasProposal)
                                .map((unit) => unit.id),
                            ),
                          )
                        }
                      >
                        <Icon name="paragraph" />
                        {t("Select untranslated on page")} {page + 1}
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="btn btn--small"
                      disabled={running}
                      onClick={() => setSelected(new Set())}
                    >
                      <Icon name="close" />
                      {t("Clear selection")}
                    </button>
                  </div>
                </div>
                {!selection ? (
                  <p className="dialog__muted">
                    Choose an AI model in Settings to translate text. Images need no
                    provider.
                  </p>
                ) : null}
                <SourceRows
                  units={data.units.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)}
                  selected={selected}
                  disabled={running || editor.appending}
                  toggle={(id, checked) => toggle(id, checked, "source")}
                />
                {data.units.length > PAGE_SIZE ? (
                  <div className="button-row">
                    <button
                      type="button"
                      className="btn"
                      disabled={!page}
                      onClick={() => setPage(page - 1)}
                    >
                      <Icon name="left" />
                      {t("Previous items")}
                    </button>
                    <span>
                      {t("Page")} {page + 1} {t("of")}{" "}
                      {Math.ceil(data.units.length / PAGE_SIZE)}
                    </span>
                    <button
                      type="button"
                      className="btn"
                      disabled={(page + 1) * PAGE_SIZE >= data.units.length}
                      onClick={() => setPage(page + 1)}
                    >
                      <Icon name="right" />
                      {t("Next items")}
                    </button>
                  </div>
                ) : null}
                <details className="settings-help">
                  <summary>
                    <Icon name="keep" />
                    {t("Translation instructions")}
                  </summary>
                  {bookInstructions ? (
                    <details>
                      <summary>{t("Book instructions")}</summary>
                      <p className="book-instructions">{bookInstructions}</p>
                    </details>
                  ) : null}
                  <label className="form-field">
                    <span>{t("Instructions for this run (optional)")}</span>
                    <textarea
                      rows={3}
                      maxLength={20000}
                      value={currentInstructions}
                      disabled={running || editor.appending}
                      onChange={(event) => onInstructions(event.target.value)}
                    />
                  </label>
                  <p className="dialog__muted">
                    {t(
                      "Applies to new runs. Resume and Retry keep the saved run’s instructions.",
                    )}
                  </p>
                </details>
              </>
            ) : run ? (
              <>
                {step === "progress" ? (
                  <>
                    <h3 tabIndex={-1}>
                      {running
                        ? "Translating selected items"
                        : run.state === "failed"
                          ? "Translation paused after an error"
                          : "Saved translation progress"}
                    </h3>
                    <p role="status">
                      {run.config.model || "Local images"} · {runReady.length}{" "}
                      {t("ready ·")}{" "}
                      {run.items.filter((item) => item.state === "translating").length}{" "}
                      {t("translating ·")}{" "}
                      {run.items.filter((item) => item.state === "failed").length}{" "}
                      {t("failed")}
                    </p>
                    {run.message ? (
                      <StatusNotice kind={run.state === "failed" ? "error" : "info"}>
                        {run.message}
                      </StatusNotice>
                    ) : null}
                    {!running && hasPendingText ? consentBox : null}
                    {!running && run.state !== "done" ? (
                      <div className="button-row">
                        <button
                          type="button"
                          className="btn"
                          disabled={hasPendingText && !consent}
                          onClick={() =>
                            void control(run.state === "failed" ? "retry" : "start")
                          }
                        >
                          <Icon name="refresh" />
                          {run.state === "failed"
                            ? "Retry failed batch"
                            : "Resume saved run"}
                        </button>
                        {run.state === "failed" ? (
                          <>
                            <button
                              type="button"
                              className="btn"
                              disabled={!consent || run.budget <= 512}
                              onClick={() => void control("smaller")}
                            >
                              <Icon name="minus" />
                              {t("Retry with smaller batches")}
                            </button>
                            <button
                              type="button"
                              className="btn"
                              disabled={hasPendingText && !consent}
                              onClick={() => void control("skip")}
                            >
                              <Icon name="right" />
                              {t("Skip and continue")}
                            </button>
                          </>
                        ) : null}
                      </div>
                    ) : null}
                    {run.items
                      .slice(previewPage * PAGE_SIZE, (previewPage + 1) * PAGE_SIZE)
                      .map((item) => (
                        <article key={item.id} className="ai-selection__item">
                          <strong>
                            {item.order + 1}.{" "}
                            {data.units.find((unit) => unit.id === item.unitId)?.kind ??
                              "Content"}
                          </strong>
                          <p>
                            {item.state === "superseded"
                              ? "Replaced by a newer translation"
                              : item.state === "added"
                                ? "Added to draft"
                                : item.state === "ready"
                                  ? "Ready"
                                  : item.state === "translating"
                                    ? "Translating…"
                                    : item.state === "failed"
                                      ? t("Failed")
                                      : item.state === "skipped"
                                        ? "Skipped"
                                        : "Pending"}{" "}
                            {!["added", "superseded"].includes(item.state)
                              ? `· ${item.partsDone}/${item.partsTotal} parts saved`
                              : ""}
                          </p>
                          {item.failedOutput ? (
                            <details>
                              <summary>{t("Recoverable provider output")}</summary>
                              <textarea
                                readOnly
                                value={item.failedOutput}
                                aria-label={`Recoverable output for item ${item.order + 1}`}
                              />
                            </details>
                          ) : null}
                        </article>
                      ))}
                    <details>
                      <summary>{t("Live provider output")}</summary>
                      <textarea
                        className="ai-stream"
                        aria-label={t("Selected translation live output")}
                        readOnly
                        value={stream}
                      />
                    </details>
                    <details>
                      <summary>{t("Technical request details")}</summary>
                      <ApiRequestDetails messages={trace} />
                    </details>
                    {eventsUnavailable ? (
                      <p role="status">
                        {t(
                          "Live output is unavailable. Saved progress is refreshed while the run is active.",
                        )}
                      </p>
                    ) : null}
                  </>
                ) : (
                  <>
                    <div className="ai-selection__heading">
                      <h3 tabIndex={-1}>{t("Review translations")}</h3>
                      <p className="dialog__muted">
                        {t(
                          "All saved batches in this chapter · Results wait here until you add them to the draft.",
                        )}
                      </p>
                      <span>
                        {ready.length} {t("ready ·")}{" "}
                        {ready.filter((item) => added.has(item.unitId)).length}{" "}
                        {t("added before")}
                      </span>
                    </div>
                    {!ready.length ? (
                      <StatusNotice kind="info">
                        {t(
                          "No results waiting to be added. Added translations are saved in your chapter draft. Select more source items to continue.",
                        )}
                      </StatusNotice>
                    ) : null}
                    <div className="button-row">
                      <button
                        type="button"
                        className="btn"
                        disabled={editor.appending || !ready.length}
                        onClick={() => setAccepted(new Set(ready.map((item) => item.id)))}
                      >
                        <Icon name="check" />
                        {t("Select all results")}
                      </button>
                      <button
                        type="button"
                        className="btn"
                        disabled={editor.appending || !accepted.size}
                        onClick={() => setAccepted(new Set())}
                      >
                        <Icon name="close" />
                        {t("Clear selection")}
                      </button>
                    </div>
                    <p className="dialog__muted">
                      {t(
                        "Check results to add to the draft. Clear selection only unchecks them; it does not delete saved proposals or draft text.",
                      )}
                    </p>
                    {ready.some((item) => added.has(item.unitId)) ? (
                      <StatusNotice kind="info">
                        {t(
                          "“Added before” is insertion history, not a check of your current draft. If you removed the text, select its result and add it again.",
                        )}
                      </StatusNotice>
                    ) : null}
                    {run.items.some(
                      (item) => !["ready", "added", "superseded"].includes(item.state),
                    ) ? (
                      <p className="dialog__muted">
                        {
                          run.items.filter(
                            (item) =>
                              !["ready", "added", "superseded"].includes(item.state),
                          ).length
                        }{" "}
                        {t(
                          "items are pending, failed or skipped. Return to Translate to inspect them.",
                        )}
                      </p>
                    ) : null}
                    <ReviewRows
                      entries={proposals.slice(
                        previewPage * PAGE_SIZE,
                        (previewPage + 1) * PAGE_SIZE,
                      )}
                      units={data.units}
                      accepted={accepted}
                      disabled={editor.appending}
                      toggle={(id, checked) => toggle(id, checked, "preview")}
                    />
                    <p className="dialog__muted">
                      {t(
                        "Adds to the end of your draft, preserving existing writing. The chapter remains Draft until you review it.",
                      )}
                    </p>
                  </>
                )}
                {(step === "review" ? ready.length : run.items.length) > PAGE_SIZE ? (
                  <div className="button-row">
                    <button
                      type="button"
                      className="btn"
                      disabled={!previewPage}
                      onClick={() => setPreviewPage(previewPage - 1)}
                    >
                      <Icon name="left" />
                      {t("Previous results")}
                    </button>
                    <button
                      type="button"
                      className="btn"
                      disabled={
                        (previewPage + 1) * PAGE_SIZE >=
                        (step === "review" ? ready.length : run.items.length)
                      }
                      onClick={() => setPreviewPage(previewPage + 1)}
                    >
                      <Icon name="right" />
                      {t("Next results")}
                    </button>
                  </div>
                ) : null}
              </>
            ) : null}
          </>
        )}
      </div>
      <footer className="ai-selection__actions">
        {step === "review" && appendNotice ? (
          <div className="ai-selection__consent">
            <StatusNotice kind="success">{appendNotice}</StatusNotice>
          </div>
        ) : null}
        {step === "review" && appendError ? (
          <div className="ai-selection__consent">
            <StatusNotice
              kind={appendError.code === "append_out_of_order" ? "info" : "error"}
              onRetry={
                appendError.code === "append_out_of_order" || appendDisabled
                  ? undefined
                  : () => void append()
              }
              retryLabel="Retry adding results"
            >
              {appendError.code === "append_out_of_order"
                ? "Confirm placement below, then choose Add to draft again. Your results are still selected."
                : appendError.message}
            </StatusNotice>
          </div>
        ) : null}
        {step === "review" &&
        accepted.size > 0 &&
        (previouslyAddedSelected || needsOrderConfirmation) ? (
          <label className="export-ack ai-selection__consent">
            <input
              type="checkbox"
              checked={placementConfirmed}
              disabled={editor.appending}
              onChange={(event) => {
                setAllowAgain(previouslyAddedSelected && event.target.checked);
                setConfirmOrder(needsOrderConfirmation && event.target.checked);
              }}
            />
            <span>
              {previouslyAddedSelected
                ? "Add these results again. I removed the earlier text or want another copy. "
                : ""}
              {needsOrderConfirmation
                ? "Add earlier source items at the end of the draft, even if this differs from source order. "
                : ""}
              {t("Existing writing is preserved; duplicate anchors are still rejected.")}
            </span>
          </label>
        ) : null}

        {step === "select" && needsProvider ? (
          <div className="ai-selection__consent">{consentBox}</div>
        ) : null}
        <div role="status">
          {step === "select"
            ? `${selected.size} ${t("items selected")}`
            : step === "review"
              ? `${accepted.size} selected · Add to end of draft`
              : "Completed parts are saved locally"}
          {editor.draft?.conflict ? (
            <p className="dialog__muted">
              {t("Resolve the draft’s save conflict before adding results.")}
            </p>
          ) : null}
          {step === "select" && needsProvider && !consent ? (
            <p className="dialog__muted">
              {t("Approve sharing selected text to translate.")}
            </p>
          ) : null}
        </div>
        <div className="button-row">
          {step !== "select" && !running ? (
            <button
              type="button"
              className="btn"
              disabled={editor.appending}
              onClick={() => setStep("select")}
            >
              <Icon name="left" />
              {t("Select more")}
            </button>
          ) : null}
          {step === "select" ? (
            <button
              type="button"
              className="btn btn--primary"
              disabled={
                running ||
                editor.appending ||
                !selected.size ||
                (needsProvider && (!consent || !selection))
              }
              onClick={() => void start()}
            >
              <Icon name={localOnly ? "image" : "sparkles"} />
              {localOnly
                ? generateLabel
                : `${t("Translate")} ${selected.size} ${t("items")}`}
            </button>
          ) : step === "progress" ? (
            running ? (
              <>
                <button
                  type="button"
                  className="btn"
                  onClick={() => void control("cancel")}
                >
                  <Icon name="close" />
                  {t("Cancel translation")}
                </button>
                <button type="button" className="btn" onClick={onBackground}>
                  <Icon name="minimize" />
                  {t("Continue in background")}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="btn btn--primary"
                disabled={!ready.length}
                onClick={() => {
                  setPreviewPage(0);
                  setStep("review");
                }}
              >
                <Icon name="check" />
                {t("Review")} {ready.length} {t("results")}
              </button>
            )
          ) : (
            <button
              type="button"
              className="btn btn--primary"
              disabled={appendDisabled}
              onClick={() => void append()}
            >
              <Icon name="plus" />
              {editor.appending ? t("Saving…") : `Add ${accepted.size} results to draft`}
            </button>
          )}
        </div>
      </footer>
    </section>
  );
}
