import { invoke } from "@tauri-apps/api/core";
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { useTranslation } from "../../lib/i18n";
import {
  linkedChapter,
  loadLink,
  type SagarNetLink,
  type SagarNetVisibility,
  sagarnetRequest,
  saveLink,
} from "../../lib/sagarnet/link";
import { listChapters } from "../../lib/tauri/commands";
import { toCommandError } from "../../lib/tauri/errors";
import type { BookSettings, ProjectSummary } from "../../types/project";
import { type TocResult, TocStatus, tocResultMessage } from "./TocStatus";
import {
  type UploadItem,
  uploadProgress,
  uploadStateLabel,
  useUploadAll,
} from "./uploadAll";

type Availability = { available: boolean; host: string | null };
type Account = { id: number; email: string };
type Match = {
  exact: { book_id: number; edition_id: number }[];
  suggestions: unknown[];
};
type BookDetail = {
  id: number;
  title: string;
  visibility: SagarNetVisibility;
  can_manage: boolean;
  editions?: { id: number; has_toc?: boolean }[];
};
type RemoteChapter = { id: number; spine_position: number };

type Plan = {
  queue: UploadItem[];
  /** Chapters with a translation that are not linked to a SagarNet chapter. */
  unlinked: number;
  /** Chapters without any translation. */
  untranslated: number;
  /** Translated chapters left out by "only completed chapters". */
  incomplete: number;
};

type View =
  | { kind: "loading"; label: string }
  | { kind: "unavailable" }
  | { kind: "signed_out" }
  | { kind: "not_found"; suggestions: number }
  | {
      kind: "ready";
      book: BookDetail;
      link: SagarNetLink;
      plan: Plan;
      onlyCompleted: boolean;
    }
  | { kind: "error"; message: string; linked: boolean };

/** Enter would submit the surrounding Book Settings form. */
function keepEnter(event: KeyboardEvent) {
  if (event.key === "Enter") event.preventDefault();
}

export function UploadAllPanel({
  project,
  settings,
  disabled,
  translating,
  onFlush,
  onStarted,
}: {
  project: ProjectSummary;
  settings: BookSettings;
  disabled: boolean;
  /** True while Translate all is running for this book. */
  translating: boolean;
  onFlush?: () => Promise<boolean>;
  onStarted: () => void;
}) {
  const { t } = useTranslation();
  const uploadApi = useUploadAll();
  const upload = uploadApi.upload;
  const uploadActive = upload?.phase === "running";
  const thisUploadActive = uploadActive && upload?.projectId === project.id;
  const [view, setView] = useState<View>({
    kind: "loading",
    label: "Connecting to SagarNet…",
  });
  const [host, setHost] = useState<string | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [busy, setBusy] = useState(false);
  /** Why the TOC upload after creating the book did not happen, shown once. */
  const [tocError, setTocError] = useState<string | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [visibility, setVisibility] = useState<SagarNetVisibility>("private");
  const [publishBook, setPublishBook] = useState(false);
  const [onlyCompleted, setOnlyCompleted] = useState(false);
  const [title, setTitle] = useState((settings.title || project.title).slice(0, 255));
  const [author, setAuthor] = useState(
    (settings.author || project.author || "").slice(0, 255),
  );
  const [description, setDescription] = useState(settings.description.slice(0, 10000));
  const [publicBook, setPublicBook] = useState(false);
  const generation = useRef(0);

  const buildPlan = useCallback(
    async (link: SagarNetLink, onlyDone: boolean): Promise<Plan> => {
      const [local, remote] = await Promise.all([
        listChapters(project.id),
        sagarnetRequest<RemoteChapter[]>("all_chapters", {
          book: link.book,
          edition: link.edition,
        }),
      ]);
      const plan: Plan = { queue: [], unlinked: 0, untranslated: 0, incomplete: 0 };
      for (const chapter of [...local].sort((a, b) => a.spineIndex - b.spineIndex)) {
        if (chapter.status === "not_started") {
          plan.untranslated += 1;
          continue;
        }
        if (onlyDone && chapter.status !== "completed") {
          plan.incomplete += 1;
          continue;
        }
        const target = linkedChapter(link, chapter, remote);
        if (target === undefined) plan.unlinked += 1;
        else
          plan.queue.push({
            chapterId: chapter.id,
            title: chapter.title,
            remote: target,
          });
      }
      return plan;
    },
    [project.id],
  );

  const load = useCallback(
    async (onlyDone: boolean) => {
      const current = ++generation.current;
      const show = (next: View) => {
        if (current === generation.current) setView(next);
      };
      setError(null);
      let linked = false;
      try {
        show({ kind: "loading", label: "Connecting to SagarNet…" });
        const availability = await invoke<Availability>("sharing_availability");
        setHost(availability.host);
        if (!availability.available) return show({ kind: "unavailable" });
        const signedIn = await sagarnetRequest<Account | null>("account");
        setAccount(signedIn);
        if (!signedIn) return show({ kind: "signed_out" });
        let link = loadLink(availability.host, project.id);
        if (!link) {
          show({ kind: "loading", label: "Looking for this book on SagarNet…" });
          const match = await sagarnetRequest<Match>("match", { project: project.id });
          const exact = match.exact[0];
          if (!exact)
            return show({ kind: "not_found", suggestions: match.suggestions.length });
          link = {
            book: exact.book_id,
            edition: exact.edition_id,
            exact: true,
            chapters: {},
          };
          saveLink(availability.host, project.id, link);
        }
        linked = true;
        show({ kind: "loading", label: "Reading chapters…" });
        const book = await sagarnetRequest<BookDetail>("book", { book: link.book });
        const plan = await buildPlan(link, onlyDone);
        show({ kind: "ready", book, link, plan, onlyCompleted: onlyDone });
      } catch (cause) {
        const problem = toCommandError(cause);
        if (problem.code === "sharing_unauthorized") {
          setAccount(null);
          return show({ kind: "signed_out" });
        }
        show({ kind: "error", message: problem.message, linked });
      }
    },
    [buildPlan, project.id],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: Loads once per book; later reloads are explicit.
  useEffect(() => {
    void load(false);
    return () => {
      generation.current += 1;
    };
  }, [project.id]);

  async function act(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      const problem = toCommandError(cause);
      if (problem.code === "sharing_unauthorized") {
        setAccount(null);
        setView({ kind: "signed_out" });
      }
      setError(problem.message);
    } finally {
      setBusy(false);
    }
  }

  async function signIn() {
    setSigningIn(true);
    try {
      await invoke<Account>("sharing_sign_in");
    } finally {
      setSigningIn(false);
    }
    await load(onlyCompleted);
  }

  async function createBook() {
    if (!title.trim()) throw new Error("Enter a book title.");
    const created = await sagarnetRequest<{
      book_id: number;
      edition_id: number;
      toc?: TocResult;
    }>("create", {
      project: project.id,
      title,
      author,
      description,
      public: publicBook,
    });
    const tocProblem = created.toc ? tocResultMessage(created.toc) : null;
    setTocError(tocProblem ? t(tocProblem) : null);
    saveLink(host, project.id, {
      book: created.book_id,
      edition: created.edition_id,
      exact: true,
      chapters: {},
    });
    await load(onlyCompleted);
  }

  async function startUpload(ready: Extract<View, { kind: "ready" }>) {
    if (onFlush && !(await onFlush()))
      throw new Error("Save the current chapter before uploading.");
    if (visibility === "public" && ready.book.visibility !== "public") {
      if (!publishBook) throw new Error("Confirm that the book becomes public first.");
      await sagarnetRequest("publish_book", { book: ready.book.id });
    }
    // Plan again so chapters saved since the tab opened are included.
    const plan = await buildPlan(ready.link, ready.onlyCompleted);
    uploadApi.start({
      projectId: project.id,
      projectTitle: project.title,
      host,
      link: ready.link,
      targetLanguage: project.targetLanguage,
      visibility,
      queue: plan.queue,
    });
    onStarted();
  }

  const fieldsDisabled = busy || disabled;
  let step: 0 | 1 | 2 = 0;
  let body: ReactNode;
  if (thisUploadActive && upload) {
    step = 2;
    body = (
      <div className="book-settings__batch-status">
        <StatusNotice kind="loading">
          {t(uploadStateLabel(upload))} · {uploadProgress(upload, t)}
        </StatusNotice>
        <button type="button" className="btn btn--cancel" onClick={uploadApi.stop}>
          <Icon name="stop" />
          {t("Stop")}
        </button>
      </div>
    );
  } else if (view.kind === "loading") {
    body = <StatusNotice kind="loading">{t(view.label)}</StatusNotice>;
  } else if (view.kind === "unavailable") {
    body = (
      <StatusNotice kind="info">
        {t("SagarNet is not available in this build of Sagar.")}
      </StatusNotice>
    );
  } else if (view.kind === "signed_out") {
    body = (
      <>
        <p className="upload-all__lead">
          {t("Sign in to SagarNet to upload this book’s translations.")}
        </p>
        <div className="button-row">
          <button
            type="button"
            className="btn btn--primary"
            disabled={fieldsDisabled}
            onClick={() => void act(signIn)}
          >
            <Icon name="user" />
            {signingIn ? t("Waiting for your browser…") : t("Sign in")}
          </button>
          {signingIn ? (
            <button
              type="button"
              className="btn btn--cancel"
              onClick={() => void invoke("sharing_cancel_sign_in").catch(() => {})}
            >
              <Icon name="close" />
              {t("Cancel")}
            </button>
          ) : null}
        </div>
      </>
    );
  } else if (view.kind === "not_found") {
    step = 1;
    body = (
      <>
        <p className="upload-all__lead">
          {t("This book isn’t on SagarNet yet. Add it first, then upload chapters.")}
        </p>
        <div className="book-settings__fields">
          <label className="form-field">
            <span>{t("Title")}</span>
            <input
              value={title}
              required
              maxLength={255}
              disabled={fieldsDisabled}
              onKeyDown={keepEnter}
              onChange={(event) => setTitle(event.target.value)}
            />
          </label>
          <label className="form-field">
            <span>{t("Author")}</span>
            <input
              value={author}
              maxLength={255}
              disabled={fieldsDisabled}
              onKeyDown={keepEnter}
              onChange={(event) => setAuthor(event.target.value)}
            />
          </label>
        </div>
        <details className="settings-help">
          <summary>
            <Icon name="paragraph" />
            {t("Description (optional)")}
          </summary>
          <label className="form-field">
            <span className="visually-hidden">{t("Description")}</span>
            <textarea
              rows={3}
              value={description}
              maxLength={10000}
              disabled={fieldsDisabled}
              onChange={(event) => setDescription(event.target.value)}
            />
          </label>
        </details>
        <label className="export-ack">
          <input
            type="checkbox"
            checked={publicBook}
            disabled={fieldsDisabled}
            onChange={(event) => setPublicBook(event.target.checked)}
          />
          <span>{t("Public book (anyone can find it)")}</span>
        </label>
        {view.suggestions ? (
          <p className="dialog__muted">
            {t(
              "SagarNet has similar books. To use one of them, open Upload to SagarNet in the book workspace.",
            )}
          </p>
        ) : null}
        <div className="button-row">
          <button
            type="button"
            className="btn btn--primary"
            disabled={fieldsDisabled || !title.trim()}
            onClick={() => void act(createBook)}
          >
            <Icon name="plus" />
            {busy ? t("Adding…") : t("Add book to SagarNet")}
          </button>
        </div>
      </>
    );
  } else if (view.kind === "error") {
    body = (
      <>
        <StatusNotice kind="error" onRetry={() => void load(onlyCompleted)}>
          {view.message}
        </StatusNotice>
        {view.linked ? (
          <div className="button-row">
            <button
              type="button"
              className="btn"
              disabled={fieldsDisabled}
              onClick={() => {
                saveLink(host, project.id, null);
                void load(onlyCompleted);
              }}
            >
              <Icon name="unlink" />
              {t("Unlink SagarNet book")}
            </button>
          </div>
        ) : null}
      </>
    );
  } else {
    step = 2;
    const ready = view;
    const { plan, book } = ready;
    const count = plan.queue.length;
    const bookPrivate = book.visibility !== "public";
    const publicBlocked = bookPrivate && !book.can_manage;
    const blocked =
      fieldsDisabled ||
      translating ||
      uploadActive ||
      !count ||
      (visibility === "public" && bookPrivate && !publishBook);
    const skipped = plan.unlinked + plan.incomplete;
    body = (
      <>
        <p className="upload-all__lead">
          {count ? (
            <>
              <strong>
                {count} {count === 1 ? t("chapter") : t("chapters")}
              </strong>{" "}
              {t("ready to upload to")} <strong>{book.title}</strong>
            </>
          ) : (
            t("No translated chapters to upload yet.")
          )}
        </p>
        <TocStatus
          key={`${book.id}-${ready.link.edition}`}
          project={project.id}
          book={book.id}
          edition={ready.link.edition}
          hasToc={book.editions?.find((item) => item.id === ready.link.edition)?.has_toc}
          canManage={book.can_manage}
          disabled={fieldsDisabled || uploadActive}
          initialError={tocError}
        />
        {skipped ? (
          <p className="dialog__muted">
            {skipped} {t("skipped")}
            {plan.unlinked ? ` · ${plan.unlinked} ${t("not linked to SagarNet")}` : ""}
            {plan.incomplete ? ` · ${plan.incomplete} ${t("not completed")}` : ""}
          </p>
        ) : null}
        <fieldset className="upload-all__visibility">
          <legend>{t("Who can read the chapters")}</legend>
          <label className="export-ack">
            <input
              type="radio"
              name="sagarnet-visibility"
              checked={visibility === "private"}
              disabled={fieldsDisabled}
              onChange={() => setVisibility("private")}
            />
            <span>{t("Only me")}</span>
          </label>
          <label className="export-ack">
            <input
              type="radio"
              name="sagarnet-visibility"
              checked={visibility === "public"}
              disabled={fieldsDisabled || publicBlocked}
              onChange={() => setVisibility("public")}
            />
            <span>
              {t("Everyone")}
              {publicBlocked ? ` (${t("the book owner keeps this book private")})` : ""}
            </span>
          </label>
        </fieldset>
        {visibility === "public" && bookPrivate && !publicBlocked ? (
          <label className="export-ack">
            <input
              type="checkbox"
              checked={publishBook}
              disabled={fieldsDisabled}
              onChange={(event) => setPublishBook(event.target.checked)}
            />
            <span>{t("Make the book public too")}</span>
          </label>
        ) : null}
        <label className="export-ack">
          <input
            type="checkbox"
            checked={ready.onlyCompleted}
            disabled={fieldsDisabled}
            onChange={(event) => {
              setOnlyCompleted(event.target.checked);
              void load(event.target.checked);
            }}
          />
          <span>{t("Only completed chapters")}</span>
        </label>
        {translating ? (
          <StatusNotice kind="info">
            {t("Wait for Translate all to finish.")}
          </StatusNotice>
        ) : null}
        {uploadActive && upload ? (
          <StatusNotice kind="info">
            {t("Another book is uploading:")} {upload.projectTitle}
          </StatusNotice>
        ) : null}
        <div className="button-row">
          <button
            type="button"
            className="btn btn--primary"
            disabled={blocked}
            onClick={() => void act(() => startUpload(ready))}
          >
            <Icon name="cloudUpload" />
            {busy
              ? t("Starting…")
              : count
                ? `${t("Upload")} ${count} ${count === 1 ? t("chapter") : t("chapters")}`
                : t("Upload")}
          </button>
        </div>
      </>
    );
  }

  return (
    <div className="book-settings__instructions">
      <section className="upload-all" aria-labelledby="book-settings-upload-heading">
        <div className="upload-all__header">
          <h3 className="book-settings__section-title" id="book-settings-upload-heading">
            <Icon name="cloudUpload" />
            {t("Upload to SagarNet")}
          </h3>
          {account ? (
            <span className="dialog__muted upload-all__account" title={account.email}>
              <Icon name="user" size={14} />
              {account.email}
            </span>
          ) : null}
        </div>
        {step ? (
          <ol className="upload-all__steps" aria-label={t("Steps")}>
            {[t("Add book"), t("Upload chapters")].map((label, index) => (
              <li
                key={label}
                className={
                  index + 1 === step
                    ? "upload-all__step upload-all__step--current"
                    : index + 1 < step
                      ? "upload-all__step upload-all__step--done"
                      : "upload-all__step"
                }
                aria-current={index + 1 === step ? "step" : undefined}
              >
                <span className="upload-all__step-number" aria-hidden="true">
                  {index + 1 < step ? <Icon name="check" size={12} /> : index + 1}
                </span>
                {label}
              </li>
            ))}
          </ol>
        ) : null}
        {body}
        {error ? <StatusNotice kind="error">{t(error)}</StatusNotice> : null}
        <details className="settings-help book-settings__help">
          <summary>
            <Icon name="keep" />
            {t("How uploading works")}
          </summary>
          <p className="dialog__muted">
            {t("Chapters upload one at a time, using your latest saved translation.")}
          </p>
          <p className="dialog__muted">
            {t(
              "Pictures are sent inside each chapter. Your book on this computer is not changed.",
            )}
          </p>
          <p className="dialog__muted">
            {t(
              "Unchanged chapters are skipped. Chapters edited on SagarNet since your last upload are never overwritten.",
            )}
          </p>
        </details>
      </section>
    </div>
  );
}
