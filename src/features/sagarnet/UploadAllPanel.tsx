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
    const created = await sagarnetRequest<{ book_id: number; edition_id: number }>(
      "create",
      { project: project.id, title, author, description, public: publicBook },
    );
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

  const accountLine = account ? (
    <p className="dialog__muted book-settings__account">
      <Icon name="user" size={16} />
      {t("Signed in to SagarNet as")} {account.email}
    </p>
  ) : null;

  let body: ReactNode;
  if (thisUploadActive && upload) {
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
        <p className="dialog__muted">
          {t(
            "Sign in to SagarNet to upload this book’s translations. Your browser opens so you can sign in or create an account.",
          )}
        </p>
        <div className="button-row">
          <button
            type="button"
            className="btn btn--primary"
            disabled={busy || disabled}
            onClick={() => void act(signIn)}
          >
            <Icon name="user" />
            {signingIn ? t("Waiting for your browser…") : t("Sign in to SagarNet")}
          </button>
          {signingIn ? (
            <button
              type="button"
              className="btn btn--cancel"
              onClick={() => void invoke("sharing_cancel_sign_in").catch(() => {})}
            >
              <Icon name="close" />
              {t("Cancel sign-in")}
            </button>
          ) : null}
        </div>
      </>
    );
  } else if (view.kind === "not_found") {
    body = (
      <>
        {accountLine}
        <StatusNotice kind="info">
          {view.suggestions
            ? `${t("This edition is not on SagarNet yet. Similar books found:")} ${view.suggestions}. ${t(
                "To link one of them instead, use Upload to SagarNet in the book workspace.",
              )}`
            : t("This book is not on SagarNet yet. Create it to upload chapters.")}
        </StatusNotice>
        <label className="form-field">
          <span>{t("Title on SagarNet")}</span>
          <input
            value={title}
            required
            maxLength={255}
            disabled={busy || disabled}
            onKeyDown={keepEnter}
            onChange={(event) => setTitle(event.target.value)}
          />
        </label>
        <label className="form-field">
          <span>{t("Author on SagarNet")}</span>
          <input
            value={author}
            maxLength={255}
            disabled={busy || disabled}
            onKeyDown={keepEnter}
            onChange={(event) => setAuthor(event.target.value)}
          />
        </label>
        <label className="form-field">
          <span>{t("Catalog description")}</span>
          <textarea
            rows={3}
            value={description}
            maxLength={10000}
            disabled={busy || disabled}
            onChange={(event) => setDescription(event.target.value)}
          />
        </label>
        <label className="export-ack">
          <input
            type="checkbox"
            checked={publicBook}
            disabled={busy || disabled}
            onChange={(event) => setPublicBook(event.target.checked)}
          />
          <span>
            {t(
              "Make the book public. Anyone can find it; your chapters stay private until you publish them.",
            )}
          </span>
        </label>
        <div className="button-row">
          <button
            type="button"
            className="btn btn--primary"
            disabled={busy || disabled || !title.trim()}
            onClick={() => void act(createBook)}
          >
            <Icon name="plus" />
            {busy ? t("Creating…") : t("Create on SagarNet")}
          </button>
        </div>
        <p className="dialog__muted">
          {t(
            "Sends the title, author, description, cover and a fingerprint of each chapter. Chapter text is not sent until you upload.",
          )}
        </p>
      </>
    );
  } else if (view.kind === "error") {
    body = (
      <>
        {accountLine}
        <StatusNotice kind="error" onRetry={() => void load(onlyCompleted)}>
          {view.message}
        </StatusNotice>
        {view.linked ? (
          <div className="button-row">
            <button
              type="button"
              className="btn"
              disabled={busy || disabled}
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
    const ready = view;
    const { plan, book } = ready;
    const bookPrivate = book.visibility !== "public";
    const publicBlocked = bookPrivate && !book.can_manage;
    const blocked =
      busy ||
      disabled ||
      translating ||
      uploadActive ||
      !plan.queue.length ||
      (visibility === "public" && bookPrivate && !publishBook);
    body = (
      <>
        {accountLine}
        <p className="book-settings__upload-book">
          <Icon name={bookPrivate ? "lock" : "globe"} size={16} />
          <strong>{book.title}</strong>
          <span className="dialog__muted">
            {bookPrivate ? t("Private book") : t("Public book")}
          </span>
        </p>
        <p className="dialog__muted book-settings__batch-count">
          {plan.queue.length}{" "}
          {plan.queue.length === 1 ? t("chapter to upload") : t("chapters to upload")}
          {plan.untranslated ? ` · ${plan.untranslated} ${t("untranslated")}` : ""}
          {plan.incomplete ? ` · ${plan.incomplete} ${t("not completed")}` : ""}
          {plan.unlinked ? ` · ${plan.unlinked} ${t("not linked")}` : ""}
        </p>
        {plan.unlinked ? (
          <StatusNotice kind="info">
            {t(
              "Some translated chapters are not linked to a SagarNet chapter. Link them in Upload to SagarNet in the book workspace.",
            )}
          </StatusNotice>
        ) : null}
        <label className="export-ack">
          <input
            type="checkbox"
            checked={ready.onlyCompleted}
            disabled={busy || disabled}
            onChange={(event) => {
              setOnlyCompleted(event.target.checked);
              void load(event.target.checked);
            }}
          />
          <span>
            {t("Only completed chapters (skip drafts and unreviewed AI translations)")}
          </span>
        </label>
        <fieldset className="book-settings__visibility">
          <legend>{t("Visibility")}</legend>
          <label className="export-ack">
            <input
              type="radio"
              name="sagarnet-visibility"
              checked={visibility === "private"}
              disabled={busy || disabled}
              onChange={() => setVisibility("private")}
            />
            <span>
              {t(
                "Private: new chapters are visible only to you. Chapters you already shared keep their visibility.",
              )}
            </span>
          </label>
          <label className="export-ack">
            <input
              type="radio"
              name="sagarnet-visibility"
              checked={visibility === "public"}
              disabled={busy || disabled || publicBlocked}
              onChange={() => setVisibility("public")}
            />
            <span>
              {publicBlocked
                ? t("Public: unavailable because the book owner keeps this book private.")
                : t("Public: every uploaded chapter is published for anyone to read.")}
            </span>
          </label>
          {visibility === "public" && bookPrivate && !publicBlocked ? (
            <label className="export-ack">
              <input
                type="checkbox"
                checked={publishBook}
                disabled={busy || disabled}
                onChange={(event) => setPublishBook(event.target.checked)}
              />
              <span>{t("Also make this book public on SagarNet.")}</span>
            </label>
          ) : null}
        </fieldset>
        {translating ? (
          <StatusNotice kind="info">
            {t("Translate all is running for this book. Upload when it finishes.")}
          </StatusNotice>
        ) : null}
        {uploadActive && upload ? (
          <StatusNotice kind="info">
            {t("Another book is being uploaded:")} {upload.projectTitle}
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
            {busy ? t("Starting…") : t("Upload all")}
          </button>
        </div>
      </>
    );
  }

  return (
    <div className="book-settings__instructions">
      <section
        className="book-settings__batch"
        aria-labelledby="book-settings-upload-heading"
      >
        <h3 className="book-settings__section-title" id="book-settings-upload-heading">
          <Icon name="cloudUpload" />
          {t("Upload all chapters to SagarNet")}
        </h3>
        {body}
        {error ? <StatusNotice kind="error">{t(error)}</StatusNotice> : null}
        <p className="dialog__muted">
          {t(
            "Uploads the latest saved translation of each linked chapter, one at a time in reading order. Book images are sent inside the chapter, because SagarNet stores images only that way. Unchanged chapters are skipped, and chapters changed on SagarNet since your last upload are not overwritten. You can stop at any time.",
          )}
        </p>
      </section>
    </div>
  );
}
