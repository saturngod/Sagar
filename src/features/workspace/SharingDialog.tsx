import { invoke } from "@tauri-apps/api/core";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { getBookSettings } from "../../lib/tauri/commands";
import { toCommandError } from "../../lib/tauri/errors";
import type { ChapterSummary, ProjectSummary } from "../../types/project";
import { sanitizePreviewHtml } from "../translator/richText";

type Availability = { available: boolean; host: string | null };
type Account = { id: number; email: string };
type Visibility = "public" | "private";
type Book = {
  id: number;
  title: string;
  author: string | null;
  visibility: Visibility;
  /** Relative cover path on SagarNet, or null when the book has no cover. */
  cover_url?: string | null;
  /** Chapter count of the first edition. Present in search results and suggestions. */
  chapter_count?: number | null;
};
type Edition = { id: number; xhtml_spine_count: number };
type BookDetail = Book & { can_manage: boolean; editions: Edition[] };
type Chapter = {
  id: number;
  spine_position: number;
  label: string | null;
  public_translation_count: number;
};
type Match = {
  exact: { book_id: number; edition_id: number; book_visibility: Visibility }[];
  suggestions: Book[];
};
type Cover = { media_type: string; base64: string };
type Page<T> = { data: T[]; current_page: number; last_page: number };
type Share = { id: number; revision: number; visibility: Visibility };
type Language = { code: string; name: string };
type SharedContent = { content: string; content_format_version: number };
type LocalPreview = { content: string; revision: number };
export type SharingMode = "submit" | "browse";
type Version = { id: number; contributor_name: string; target_language: string };
type Link = {
  book: number;
  edition: number;
  exact: boolean;
  chapters: Record<string, number>;
};
type Props = {
  mode: SharingMode;
  project: ProjectSummary;
  chapters: ChapterSummary[];
  activeChapterId: string | null;
  initialMatch: Discovery | null;
  onClose: () => void;
  /** Called after a finished action; the parent closes the dialog and shows the message. */
  onComplete: (message: string) => void;
  onFlush: () => Promise<boolean>;
  onApply: (content: SharedContent) => Promise<boolean>;
};

export type Discovery = { match: Match };

const AUTO_MATCH_KEY = "sagarnet:auto-match";

function stored<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}

function persist(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Preferences are a convenience. Sharing still works without them.
  }
}

function read<T>(kind: string, fields: Record<string, unknown> = {}) {
  return invoke<T>("sharing_request", { operation: { kind, ...fields } });
}

export async function matchOpenedProject(project: string): Promise<Discovery | null> {
  if (!stored(AUTO_MATCH_KEY, false)) return null;
  const availability = await invoke<Availability>("sharing_availability");
  if (!availability.available) return null;
  return { match: await read<Match>("match", { project }) };
}

function label(chapter: Chapter) {
  return chapter.label || `Chapter ${chapter.spine_position + 1}`;
}

function SharedPreview({ value }: { value: SharedContent }) {
  if (value.content_format_version === 2) {
    return (
      <div
        className="sharing-preview"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: Shared HTML is validated natively and scrubbed before preview.
        dangerouslySetInnerHTML={{ __html: sanitizePreviewHtml(value.content) }}
      />
    );
  }
  return <p className="sharing-preview">{value.content}</p>;
}

type StepState = "done" | "current" | "pending";
const STEP_STATE_LABEL: Record<StepState, string> = {
  done: "Done",
  current: "Current step",
  pending: "Not started",
};

function Step({
  number,
  title,
  state,
  children,
}: {
  number: number;
  title: string;
  state: StepState;
  children?: ReactNode;
}) {
  return (
    <li
      className={`sharing-step sharing-step--${state}`}
      aria-current={state === "current" ? "step" : undefined}
    >
      <span className="sharing-step__marker" aria-hidden="true">
        {state === "done" ? <Icon name="check" size={14} /> : number}
      </span>
      <div className="sharing-step__body">
        <h3 className="sharing-step__title">
          {title}
          <span className="visually-hidden"> · {STEP_STATE_LABEL[state]}</span>
        </h3>
        {children}
      </div>
    </li>
  );
}

function BookCover({ src }: { src: string | null | undefined }) {
  // The title sits beside the cover, so the image itself is decorative.
  return src ? (
    <img className="sharing-cover" src={src} alt="" />
  ) : (
    <span className="sharing-cover sharing-cover--empty" aria-hidden="true">
      <Icon name="book" size={18} />
    </span>
  );
}

function VisibilityBadge({ value }: { value: Visibility }) {
  return (
    <span className={`sharing-badge sharing-badge--${value}`}>
      <Icon name={value === "public" ? "globe" : "lock"} size={12} />
      {value === "public" ? "Public" : "Private"}
    </span>
  );
}

export function SharingDialog({
  mode,
  project,
  chapters,
  activeChapterId,
  initialMatch,
  onClose,
  onComplete,
  onFlush,
  onApply,
}: Props) {
  const submitting = mode === "submit";
  const [availability, setAvailability] = useState<Availability | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [checkedAccount, setCheckedAccount] = useState(false);
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState("");
  const [signingIn, setSigningIn] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [autoMatch, setAutoMatch] = useState(() => stored(AUTO_MATCH_KEY, false));
  const [matched, setMatched] = useState(false);
  const [title, setTitle] = useState(project.title.slice(0, 255));
  const [author, setAuthor] = useState((project.author ?? "").slice(0, 255));
  const [language, setLanguage] = useState(project.targetLanguage.toLowerCase());
  const [languages, setLanguages] = useState<Language[]>([]);
  const [customLanguage, setCustomLanguage] = useState("");
  const targetLanguage = submitting
    ? project.targetLanguage.toLowerCase()
    : language === "custom"
      ? customLanguage.trim().toLowerCase()
      : language;
  const [description, setDescription] = useState("");
  const [metadataLoaded, setMetadataLoaded] = useState(false);
  const [publicBook, setPublicBook] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [query, setQuery] = useState(project.title.slice(0, 255));
  const [books, setBooks] = useState<Book[]>(initialMatch?.match.suggestions ?? []);
  const [selection, setSelection] = useState<BookDetail | null>(null);
  const [link, setLink] = useState<Link | null>(null);
  const [remoteChapters, setRemoteChapters] = useState<Chapter[]>([]);
  const [catalogPage, setCatalogPage] = useState<Page<Book> | null>(null);
  const [chapterPage, setChapterPage] = useState<Page<Chapter> | null>(null);
  const [versions, setVersions] = useState<Page<Version> | null>(null);
  const [remoteId, setRemoteId] = useState<number | null>(null);
  const [remoteChoice, setRemoteChoice] = useState("");
  const [existing, setExisting] = useState<Share | null>(null);
  const [preview, setPreview] = useState<
    ({ chapter: number; version: number } & SharedContent) | null
  >(null);
  const [localPreview, setLocalPreview] = useState<LocalPreview | null>(null);
  const [draftMissing, setDraftMissing] = useState(false);
  const [shareVisibility, setShareVisibility] = useState<Visibility>("private");
  const [conflictCopy, setConflictCopy] = useState<
    ({ title: string } & SharedContent) | null
  >(null);
  // Loaded covers by book ID: a data URL, or null when missing or unreadable.
  const [covers, setCovers] = useState<Record<number, string | null>>({});
  const coverRequests = useRef(new Set<number>());
  const started = useRef(false);
  const modal = useRef<HTMLDialogElement>(null);
  const feedbackRef = useRef<HTMLDivElement>(null);
  const local = chapters.find((chapter) => chapter.id === activeChapterId);
  const key = `sagarnet:link:${availability?.host ?? ""}:${project.id}`;

  useEffect(() => {
    const dialog = modal.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);

  // Errors and conflicts appear above the steps; bring them into view when they
  // come from an action lower in the dialog.
  useEffect(() => {
    if (error || conflictCopy)
      feedbackRef.current?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  }, [error, conflictCopy]);

  // Start once: confirm this build can reach SagarNet, then find the book.
  // biome-ignore lint/correctness/useExhaustiveDependencies: Runs once when the dialog opens.
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    run(async () => {
      const result = await invoke<Availability>("sharing_availability");
      setAvailability(result);
      if (result.available) await matchBook(result);
    }, "Connecting to SagarNet…");
  }, []);

  // Load covers one at a time in the background. Failures only leave a placeholder.
  useEffect(() => {
    const pending = [...books, ...(selection ? [selection] : [])].filter(
      (book) => book.cover_url && !coverRequests.current.has(book.id),
    );
    if (!pending.length) return;
    for (const book of pending) coverRequests.current.add(book.id);
    void (async () => {
      for (const book of pending) {
        const cover = await read<Cover | null>("cover", { book: book.id }).catch(
          () => null,
        );
        const src = cover ? `data:${cover.media_type};base64,${cover.base64}` : null;
        setCovers((current) => ({ ...current, [book.id]: src }));
      }
    })();
  }, [books, selection]);

  function run(action: () => Promise<void>, working = "Working…") {
    if (busy) return;
    setBusy(true);
    setBusyLabel(working);
    setNotice("");
    setError("");
    setConflictCopy(null);
    void action()
      .catch((cause: unknown) => {
        const problem = toCommandError(cause);
        if (problem.code === "sharing_unauthorized") setAccount(null);
        setError(problem.message);
      })
      .finally(() => setBusy(false));
  }

  function resetChapter() {
    setRemoteId(null);
    setVersions(null);
    setRemoteChoice("");
    setPreview(null);
    setExisting(null);
  }

  async function openEdition(
    book: BookDetail,
    edition: number,
    saved: Link | null,
    signedIn = account,
  ) {
    const first = await read<Page<Chapter>>("chapters", {
      book: book.id,
      edition,
      page: 1,
    });
    if (!submitting && !languages.length) {
      const catalog = await read<{ data: Language[] }>("languages");
      setLanguages(catalog.data);
    }
    setSelection(book);
    setLink(saved ?? { book: book.id, edition, exact: false, chapters: {} });
    setRemoteChapters(first.data);
    setChapterPage(first);
    resetChapter();
    if (!local) return;
    if (!saved) {
      // Suggest the chapter at the same reading position; the user confirms the link.
      if (submitting) {
        const guess = first.data.find(
          (chapter) => chapter.spine_position === local.spineIndex,
        );
        if (guess) setRemoteChoice(String(guess.id));
      }
      return;
    }
    let chapterId: number | undefined = saved.chapters[local.id];
    if (saved.exact && !chapterId) {
      const page = Math.floor(local.spineIndex / 100) + 1;
      const matchingPage =
        page === 1
          ? first
          : await read<Page<Chapter>>("chapters", { book: book.id, edition, page });
      const candidate = matchingPage.data.find(
        (chapter) => chapter.spine_position === local.spineIndex,
      );
      if (candidate && page !== 1) setRemoteChapters([...first.data, candidate]);
      chapterId = candidate?.id;
    }
    if (chapterId) {
      setRemoteChoice(String(chapterId));
      await browseChapter(chapterId, 1, signedIn);
      if (submitting && signedIn) await loadLocalPreview();
    }
  }

  async function signIn() {
    setSigningIn(true);
    setBusyLabel("Continue in your browser to sign in to SagarNet…");
    try {
      return await invoke<Account>("sharing_sign_in");
    } finally {
      setSigningIn(false);
    }
  }

  async function matchBook(current = availability, allowSignIn = false) {
    let signedIn = await read<Account | null>("account");
    if (!signedIn && allowSignIn) signedIn = await signIn();
    setAccount(signedIn);
    setCheckedAccount(true);
    if (submitting && !signedIn) return;
    const linkKey = `sagarnet:link:${current?.host ?? ""}:${project.id}`;
    const storedLink = stored<Link | null>(linkKey, null);
    if (storedLink) {
      const book = await read<BookDetail>("book", { book: storedLink.book });
      await openEdition(book, storedLink.edition, storedLink, signedIn);
      return;
    }
    const result = await read<Match>("match", { project: project.id });
    setBooks(result.suggestions);
    setShowCreate(result.suggestions.length === 0);
    setCatalogPage(null);
    setMatched(true);
    const exact = result.exact[0];
    if (exact) {
      const book = await read<BookDetail>("book", { book: exact.book_id });
      await openEdition(
        book,
        exact.edition_id,
        { book: book.id, edition: exact.edition_id, exact: true, chapters: {} },
        signedIn,
      );
      return;
    }
    setSelection(null);
    setLink(null);
    resetChapter();
    if (submitting && !metadataLoaded) {
      const metadata = await getBookSettings(project.id);
      setDescription(metadata.description.slice(0, 10000));
      setMetadataLoaded(true);
    }
  }

  async function browseChapter(
    id: number,
    page = 1,
    signedIn = account,
    filter = targetLanguage,
  ) {
    if (!filter) return;
    const result = submitting
      ? null
      : await read<Page<Version>>("versions", {
          chapter: id,
          page,
          target_language: filter,
        });
    if (id !== remoteId) setPreview(null);
    setRemoteId(id);
    setVersions(result);
    const mine =
      signedIn && submitting
        ? await read<Share | null>("mine", {
            chapter: id,
            target_language: targetLanguage,
          })
        : null;
    setExisting(mine);
    if (mine) setShareVisibility(mine.visibility);
  }

  async function loadLocalPreview() {
    if (!local) return;
    if (!(await onFlush()))
      throw new Error("Save the current chapter before uploading it.");
    const draft = await read<LocalPreview | null>("local_preview", {
      project: project.id,
      local_chapter: local.id,
    });
    setLocalPreview(draft);
    setDraftMissing(!draft);
  }

  async function confirmMapping(chapterId: number) {
    if (!selection || !link || !local) return;
    if (
      Object.entries(link.chapters).some(
        ([localId, remote]) => localId !== local.id && remote === chapterId,
      )
    ) {
      throw new Error("This SagarNet chapter is already linked to another chapter.");
    }
    if (account) await read("link", { book: selection.id });
    const updated = { ...link, chapters: { ...link.chapters, [local.id]: chapterId } };
    persist(key, updated);
    setLink(updated);
    await browseChapter(chapterId);
    if (submitting) await loadLocalPreview();
    setNotice(`Linked to ${local.title}.`);
  }

  async function chooseBook(book: Book) {
    const detail = await read<BookDetail>("book", { book: book.id });
    const edition =
      detail.editions.find((item) => item.xhtml_spine_count === chapters.length) ??
      detail.editions[0];
    if (edition) await openEdition(detail, edition.id, null);
  }

  async function searchBooks(page: number) {
    const result = await read<Page<Book>>("search", { query, page });
    setBooks(page === 1 ? result.data : [...books, ...result.data]);
    setCatalogPage(result);
  }

  function chooseAnotherBook() {
    persist(key, null);
    setSelection(null);
    setLink(null);
    setRemoteChapters([]);
    setChapterPage(null);
    resetChapter();
    setLocalPreview(null);
    setDraftMissing(false);
  }

  async function createBook() {
    const created = await read<{ book_id: number; edition_id: number }>("create", {
      project: project.id,
      title,
      author,
      description,
      public: publicBook,
    });
    const book = await read<BookDetail>("book", { book: created.book_id });
    await openEdition(book, created.edition_id, {
      book: book.id,
      edition: created.edition_id,
      exact: true,
      chapters: {},
    });
    setNotice("Book added to SagarNet.");
  }

  async function showServerCopy(chapter: number, chapterTitle: string) {
    const latest = await read<Share | null>("mine", {
      chapter,
      target_language: targetLanguage,
    });
    if (!latest) return;
    const detail = await read<SharedContent>("translation", { translation: latest.id });
    setConflictCopy({ title: chapterTitle, ...detail });
    setExisting(latest);
  }

  async function upload(mapped: number) {
    if (!local || !localPreview) return;
    const publicShare = shareVisibility === "public";
    try {
      await read("save", {
        project: project.id,
        local_chapter: local.id,
        chapter: mapped,
        public: publicShare,
        existing: revision,
        local_revision: localPreview.revision,
        target_language: targetLanguage,
      });
    } catch (cause) {
      if (toCommandError(cause).code === "sharing_conflict") {
        await showServerCopy(mapped, local.title);
      }
      throw cause;
    }
    onComplete(
      publicShare
        ? `${local.title} is published on SagarNet.`
        : `${local.title} is uploaded. Only you can see it.`,
    );
  }

  function changeLanguage(selected: string) {
    setLanguage(selected);
    setVersions(null);
    setPreview(null);
    if (remoteChoice && selected !== "custom") {
      run(() => browseChapter(Number(remoteChoice), 1, account, selected));
    }
  }

  const exactChapter = link?.exact
    ? remoteChapters.find((chapter) => chapter.spine_position === local?.spineIndex)?.id
    : undefined;
  const mapped = local && link ? (link.chapters[local.id] ?? exactChapter) : undefined;
  const revision = existing
    ? { translation_id: existing.id, revision: existing.revision }
    : undefined;
  const unavailable = availability?.available === false;
  const languageName = (code: string) =>
    languages.find((item) => item.code === code)?.name ?? code;

  const accountBar = (
    <div className="sharing-account">
      <Icon name="user" size={16} />
      <span className="sharing-account__text">
        {account ? (
          <>
            Signed in as <strong>{account.email}</strong>
          </>
        ) : checkedAccount ? (
          "Not signed in"
        ) : (
          "Checking account…"
        )}
        {availability?.host ? (
          <span className="dialog__muted"> · {availability.host}</span>
        ) : null}
      </span>
      {signingIn ? (
        <button
          type="button"
          className="btn btn--small"
          onClick={() =>
            void invoke("sharing_cancel_sign_in").catch((cause: unknown) =>
              setError(toCommandError(cause).message),
            )
          }
        >
          <Icon name="stop" size={16} />
          Cancel sign-in
        </button>
      ) : account ? (
        <button
          type="button"
          className="btn btn--small"
          disabled={busy}
          onClick={() =>
            run(async () => {
              await read("disconnect");
              setAccount(null);
              setExisting(null);
              setLocalPreview(null);
              if (submitting) {
                setSelection(null);
                resetChapter();
              }
            }, "Signing out…")
          }
        >
          <Icon name="unlink" size={16} />
          Sign out
        </button>
      ) : checkedAccount && !submitting ? (
        <button
          type="button"
          className="btn btn--small"
          disabled={busy}
          onClick={() =>
            run(async () => {
              setAccount(await signIn());
            })
          }
        >
          <Icon name="link" size={16} />
          Sign in
        </button>
      ) : null}
    </div>
  );

  const feedback = (
    <div ref={feedbackRef} className="sharing-feedback">
      {busy && <StatusNotice kind="loading">{busyLabel}</StatusNotice>}
      {error && <StatusNotice kind="error">{error}</StatusNotice>}
      {notice && !busy && <StatusNotice kind="success">{notice}</StatusNotice>}
      {conflictCopy && (
        <section className="sharing-card">
          <strong>SagarNet has a newer copy of {conflictCopy.title}</strong>
          <p className="dialog__muted">
            Compare it with your chapter, update your translation if needed, then upload
            again.
          </p>
          <SharedPreview value={conflictCopy} />
        </section>
      )}
    </div>
  );

  const bookCard = selection && (
    <div className="sharing-card sharing-book">
      <BookCover src={covers[selection.id]} />
      <div className="sharing-book__text">
        <strong>{selection.title}</strong>
        {selection.author ? (
          <span className="dialog__muted">{selection.author}</span>
        ) : null}
      </div>
      <VisibilityBadge value={selection.visibility} />
    </div>
  );

  const chapterCountLabel = (count: number) =>
    `${count} ${count === 1 ? "chapter" : "chapters"}`;
  const editionCount = selection?.editions.find(
    (edition) => edition.id === link?.edition,
  )?.xhtml_spine_count;

  // Edition picker, chapter-count check and "Choose another book" for a book the user picked.
  const chosenBookControls = selection && link && (
    <>
      {!link.exact && editionCount !== undefined && editionCount !== chapters.length && (
        <p className="dialog__muted">
          This SagarNet copy has {chapterCountLabel(editionCount)}; your book has{" "}
          {chapterCountLabel(chapters.length)}. Check each chapter before you link it.
        </p>
      )}
      <div className="button-row">
        {selection.editions.length > 1 && (
          <label className="form-field sharing-inline-field">
            <span>Edition</span>
            <select
              value={link.edition}
              disabled={busy}
              onChange={(event) => {
                const edition = Number(event.target.value);
                run(() => openEdition(selection, edition, null));
              }}
            >
              {selection.editions.map((edition, index) => (
                <option key={edition.id} value={edition.id}>
                  Edition {index + 1} · {chapterCountLabel(edition.xhtml_spine_count)}
                </option>
              ))}
            </select>
          </label>
        )}
        {!link.exact && (
          <button
            type="button"
            className="btn btn--small"
            disabled={busy}
            onClick={chooseAnotherBook}
          >
            <Icon name="search" size={16} />
            Choose another book
          </button>
        )}
      </div>
    </>
  );

  // Title/author search of SagarNet, seeded with match suggestions.
  const bookSearch = (
    <>
      <form
        className="sharing-search"
        onSubmit={(event) => {
          event.preventDefault();
          run(() => searchBooks(1), "Searching…");
        }}
      >
        <label className="form-field">
          <span className="visually-hidden">Search books on SagarNet</span>
          <input
            type="search"
            value={query}
            maxLength={255}
            placeholder="Title or author"
            onChange={(event) => setQuery(event.target.value)}
            disabled={busy}
          />
        </label>
        <button type="submit" className="btn" disabled={busy || !query.trim()}>
          <Icon name="search" />
          Search
        </button>
      </form>
      {catalogPage && books.length === 0 && (
        <p className="dialog__muted">No books found. Try a different title or author.</p>
      )}
      {books.length > 0 && (
        <ul className="sharing-list" aria-label="Books on SagarNet">
          {books.map((book) => (
            <li key={book.id} className="sharing-list__item">
              <BookCover src={covers[book.id]} />
              <span className="sharing-list__text">
                <strong>{book.title}</strong>
                <span className="dialog__muted">
                  {[
                    book.author,
                    typeof book.chapter_count === "number"
                      ? chapterCountLabel(book.chapter_count)
                      : null,
                    book.visibility === "private" ? "Private" : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </span>
              <button
                type="button"
                className="btn btn--small"
                aria-label={`Choose ${book.title}`}
                disabled={busy}
                onClick={() => run(() => chooseBook(book))}
              >
                <Icon name="check" size={16} />
                Choose
              </button>
            </li>
          ))}
        </ul>
      )}
      {catalogPage && catalogPage.current_page < catalogPage.last_page && (
        <button
          type="button"
          className="btn btn--small"
          disabled={busy}
          onClick={() => run(() => searchBooks(catalogPage.current_page + 1))}
        >
          <Icon name="plus" size={16} />
          More books
        </button>
      )}
    </>
  );

  // ----- Upload steps -----
  const signInStep: StepState = account ? "done" : "current";
  const bookStep: StepState = !account
    ? "pending"
    : selection && link
      ? "done"
      : "current";
  const uploadStep: StepState =
    bookStep !== "done" ? "pending" : existing ? "done" : "current";

  const submitSteps = (
    <ol className="sharing-steps">
      <Step number={1} title="Sign in to SagarNet" state={signInStep}>
        {!account && checkedAccount && (
          <>
            <p className="dialog__muted">
              Your browser opens so you can sign in or create an account. Sagar keeps you
              signed in on this device.
            </p>
            <div className="button-row">
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy}
                onClick={() => run(() => matchBook(availability, true), "Signing in…")}
              >
                <Icon name="link" />
                Sign in with browser
              </button>
            </div>
          </>
        )}
      </Step>

      <Step number={2} title="Find this book on SagarNet" state={bookStep}>
        {account && selection && link && (
          <>
            {bookCard}
            {chosenBookControls}
            {selection.visibility === "private" && (
              <div className="sharing-hint">
                <p className="dialog__muted sharing-hint__text">
                  This book is private, so its chapters can only be uploaded privately.
                  {selection.can_manage
                    ? " Make it public to let others find your translations."
                    : ""}
                </p>
                {selection.can_manage && (
                  <button
                    type="button"
                    className="btn btn--small"
                    disabled={busy}
                    onClick={() => {
                      if (
                        !window.confirm(
                          `Make “${selection.title}” visible to everyone on SagarNet?`,
                        )
                      )
                        return;
                      run(async () => {
                        await read("publish_book", { book: selection.id });
                        setSelection(
                          await read<BookDetail>("book", { book: selection.id }),
                        );
                        setNotice("The book is now public on SagarNet.");
                      });
                    }}
                  >
                    <Icon name="globe" size={16} />
                    Make book public
                  </button>
                )}
              </div>
            )}
          </>
        )}
        {account && matched && !selection && (
          <p className="dialog__muted">
            {books.length > 0 && !catalogPage
              ? "No exact copy of this book is on SagarNet, but these look similar. Choose one if it’s the same book, or search by title or author."
              : "No exact copy of this book is on SagarNet. Search by title or author in case someone added another copy."}
          </p>
        )}
        {account && matched && !selection && bookSearch}
        {account && matched && !selection && !showCreate && (
          <div className="sharing-hint">
            <p className="dialog__muted sharing-hint__text">Not on SagarNet yet?</p>
            <button
              type="button"
              className="btn btn--small"
              aria-expanded={false}
              aria-controls="sagarnet-create-book"
              disabled={busy}
              onClick={() => setShowCreate(true)}
            >
              <Icon name="plus" size={16} />
              Add as a new book
            </button>
          </div>
        )}
        {account && matched && !selection && showCreate && (
          <form
            id="sagarnet-create-book"
            className="sharing-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (!publicBook || window.confirm("Make this book visible to everyone?"))
                run(createBook, "Adding the book…");
            }}
          >
            <p className="dialog__muted">
              Or add it as a new book. Check the details first. The cover and chapter
              titles are included.
            </p>
            <label className="form-field">
              <span>Title</span>
              <input
                value={title}
                maxLength={255}
                required
                disabled={busy}
                onChange={(event) => setTitle(event.target.value)}
              />
            </label>
            <label className="form-field">
              <span>Author (optional)</span>
              <input
                value={author}
                maxLength={255}
                disabled={busy}
                onChange={(event) => setAuthor(event.target.value)}
              />
            </label>
            <label className="form-field">
              <span>Description (optional)</span>
              <textarea
                value={description}
                maxLength={10000}
                rows={3}
                disabled={busy}
                onChange={(event) => setDescription(event.target.value)}
              />
            </label>
            <label className="sharing-check">
              <input
                type="checkbox"
                checked={publicBook}
                onChange={(event) => setPublicBook(event.target.checked)}
                disabled={busy}
              />
              Make the book public so others can find it
            </label>
            <div className="button-row">
              <button
                type="submit"
                className="btn btn--primary"
                disabled={busy || !title.trim()}
              >
                <Icon name="plus" />
                Add book to SagarNet
              </button>
            </div>
          </form>
        )}
      </Step>

      <Step number={3} title="Upload this chapter" state={uploadStep}>
        {bookStep === "done" && !local && (
          <p className="dialog__muted">Open a chapter in the workspace to upload it.</p>
        )}
        {bookStep === "done" && local && !mapped && link?.exact && (
          <p className="dialog__muted">
            {local.title} isn’t part of this book on SagarNet.
          </p>
        )}
        {bookStep === "done" && local && !mapped && link && !link.exact && (
          <form
            className="sharing-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (remoteChoice)
                run(() => confirmMapping(Number(remoteChoice)), "Linking…");
            }}
          >
            <p className="dialog__muted">
              Choose the SagarNet chapter that matches {local.title}. Sagar remembers this
              link for this book.
            </p>
            <div className="sharing-search">
              <label className="form-field">
                <span>Chapter on SagarNet</span>
                <select
                  value={remoteChoice}
                  disabled={busy}
                  onChange={(event) => setRemoteChoice(event.target.value)}
                >
                  <option value="">Choose a chapter</option>
                  {remoteChapters.map((chapter) => (
                    <option key={chapter.id} value={chapter.id}>
                      {chapter.spine_position + 1}. {label(chapter)}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="submit"
                className="btn btn--primary"
                disabled={busy || !remoteChoice}
              >
                <Icon name="link" />
                Link chapter
              </button>
            </div>
            {chapterPage && chapterPage.current_page < chapterPage.last_page && (
              <div className="button-row">
                <button
                  type="button"
                  className="btn btn--small"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      const result = await read<Page<Chapter>>("chapters", {
                        book: link.book,
                        edition: link.edition,
                        page: chapterPage.current_page + 1,
                      });
                      setRemoteChapters([
                        ...remoteChapters,
                        ...result.data.filter(
                          (chapter) =>
                            !remoteChapters.some((current) => current.id === chapter.id),
                        ),
                      ]);
                      setChapterPage(result);
                    })
                  }
                >
                  <Icon name="plus" size={16} />
                  More chapters
                </button>
              </div>
            )}
          </form>
        )}
        {bookStep === "done" && local && mapped && mapped === remoteId && (
          <div className="sharing-form">
            <p className="sharing-status">
              <Icon name={existing ? "check" : "upload"} size={16} />
              {existing ? (
                <span>
                  Uploaded · {existing.visibility === "public" ? "public" : "only you"}{" "}
                  can see it. Uploading again replaces it.
                </span>
              ) : (
                <span>Not uploaded yet.</span>
              )}
            </p>
            <div className="sharing-preview-header">
              <span className="dialog__muted">
                Your saved translation of {local.title}
              </span>
              <button
                type="button"
                className="icon-button"
                aria-label={`Reload saved translation of ${local.title}`}
                title="Reload saved translation"
                disabled={busy}
                onClick={() => run(loadLocalPreview, "Loading your translation…")}
              >
                <Icon name="refresh" />
              </button>
            </div>
            {localPreview ? (
              <SharedPreview value={{ ...localPreview, content_format_version: 2 }} />
            ) : draftMissing ? (
              <p className="sharing-preview sharing-preview--empty">
                This chapter has no saved translation yet. Write it in the workspace, then
                reload.
              </p>
            ) : null}
            <fieldset className="sharing-choice" disabled={busy}>
              <legend>Who can see it</legend>
              <label>
                <input
                  type="radio"
                  name="sagarnet-visibility"
                  checked={shareVisibility === "private"}
                  onChange={() => setShareVisibility("private")}
                />
                <span className="sharing-choice__text">
                  <strong>Only me</strong>
                  <span className="dialog__muted">Keep a private copy on SagarNet.</span>
                </span>
              </label>
              <label>
                <input
                  type="radio"
                  name="sagarnet-visibility"
                  checked={shareVisibility === "public"}
                  disabled={selection?.visibility !== "public"}
                  onChange={() => setShareVisibility("public")}
                />
                <span className="sharing-choice__text">
                  <strong>Everyone</strong>
                  <span className="dialog__muted">
                    {selection?.visibility === "public"
                      ? "Anyone reading this book on SagarNet can use it."
                      : "Available when the book is public."}
                  </span>
                </span>
              </label>
            </fieldset>
            <div className="button-row sharing-row-end">
              {revision && (
                <button
                  type="button"
                  className="btn"
                  disabled={busy}
                  onClick={() => {
                    if (
                      !window.confirm(
                        "Remove your upload from SagarNet? Your translation stays on this device.",
                      )
                    )
                      return;
                    run(async () => {
                      await read("delete", { chapter: mapped, existing: revision });
                      onComplete(
                        `Your upload of ${local.title} was removed from SagarNet.`,
                      );
                    }, "Removing…");
                  }}
                >
                  <Icon name="trash" />
                  Remove upload
                </button>
              )}
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy || !localPreview}
                onClick={() => {
                  if (
                    shareVisibility === "public" &&
                    !window.confirm(`Publish ${local.title} for everyone on SagarNet?`)
                  )
                    return;
                  run(() => upload(mapped), "Uploading…");
                }}
              >
                <Icon name="upload" />
                {existing ? "Upload changes" : "Upload chapter"}
              </button>
            </div>
          </div>
        )}
      </Step>
    </ol>
  );

  // ----- Download steps -----
  const chooseBookStep: StepState = selection ? "done" : "current";
  const chapterStep: StepState = !selection ? "pending" : remoteId ? "done" : "current";
  const versionStep: StepState = !versions ? "pending" : preview ? "done" : "current";

  const browseSteps = (
    <ol className="sharing-steps">
      <Step number={1} title="Choose the book" state={chooseBookStep}>
        {selection && link ? (
          <>
            {bookCard}
            {chosenBookControls}
          </>
        ) : matched ? (
          <>
            <p className="dialog__muted">
              No exact copy of this book is on SagarNet. Search for it by title or author.
            </p>
            {bookSearch}
          </>
        ) : null}
      </Step>

      <Step number={2} title="Choose a chapter and language" state={chapterStep}>
        {selection && link && (
          <>
            <div className="sharing-grid">
              <label className="form-field">
                <span>Chapter on SagarNet</span>
                <select
                  value={remoteChoice}
                  disabled={busy}
                  onChange={(event) => {
                    const value = event.target.value;
                    resetChapter();
                    setRemoteChoice(value);
                    if (value && targetLanguage) run(() => browseChapter(Number(value)));
                  }}
                >
                  <option value="">Choose a chapter</option>
                  {remoteChapters.map((chapter) => (
                    <option key={chapter.id} value={chapter.id}>
                      {label(chapter)} · {chapter.public_translation_count} translations
                    </option>
                  ))}
                </select>
              </label>
              <label className="form-field">
                <span>Language</span>
                <select
                  value={language}
                  disabled={busy}
                  onChange={(event) => changeLanguage(event.target.value)}
                >
                  {!languages.some(
                    (item) => item.code === project.targetLanguage.toLowerCase(),
                  ) && (
                    <option value={project.targetLanguage.toLowerCase()}>
                      {project.targetLanguage} (this book)
                    </option>
                  )}
                  {languages.map((item) => (
                    <option key={item.code} value={item.code}>
                      {item.name}
                    </option>
                  ))}
                  <option value="custom">Other language tag…</option>
                </select>
              </label>
            </div>
            {language === "custom" && (
              <form
                className="sharing-search"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (remoteChoice) run(() => browseChapter(Number(remoteChoice)));
                }}
              >
                <label className="form-field">
                  <span>Language tag</span>
                  <input
                    value={customLanguage}
                    maxLength={63}
                    disabled={busy}
                    placeholder="my-Mymr-MM"
                    onChange={(event) => {
                      setCustomLanguage(event.target.value);
                      setVersions(null);
                      setPreview(null);
                    }}
                  />
                </label>
                <button
                  type="submit"
                  className="btn"
                  disabled={busy || !remoteChoice || !customLanguage.trim()}
                >
                  <Icon name="search" />
                  Find translations
                </button>
              </form>
            )}
            <div className="button-row">
              {chapterPage && chapterPage.current_page < chapterPage.last_page && (
                <button
                  type="button"
                  className="btn btn--small"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      const result = await read<Page<Chapter>>("chapters", {
                        book: link.book,
                        edition: link.edition,
                        page: chapterPage.current_page + 1,
                      });
                      setRemoteChapters([
                        ...remoteChapters,
                        ...result.data.filter(
                          (chapter) =>
                            !remoteChapters.some((current) => current.id === chapter.id),
                        ),
                      ]);
                      setChapterPage(result);
                    })
                  }
                >
                  <Icon name="plus" size={16} />
                  More chapters
                </button>
              )}
              {mapped && mapped !== remoteId && (
                <button
                  type="button"
                  className="btn btn--small"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      setRemoteChoice(String(mapped));
                      await browseChapter(mapped);
                    })
                  }
                >
                  <Icon name="link" size={16} />
                  Go to {local?.title}
                </button>
              )}
            </div>
          </>
        )}
      </Step>

      <Step number={3} title="Pick a translation to use" state={versionStep}>
        {versions && (
          <>
            {versions.data.length === 0 ? (
              <p className="dialog__muted">
                No one has shared this chapter in {languageName(targetLanguage)} yet.
              </p>
            ) : (
              <ul className="sharing-list">
                {versions.data.map((version) => (
                  <li
                    key={version.id}
                    className={`sharing-list__item${
                      preview?.version === version.id
                        ? " sharing-list__item--selected"
                        : ""
                    }`}
                  >
                    <span className="sharing-list__text">
                      <strong>{version.contributor_name}</strong>
                      <span className="dialog__muted">
                        {languageName(version.target_language)}
                      </span>
                    </span>
                    <button
                      type="button"
                      className="btn btn--small"
                      aria-label={`Preview translation by ${version.contributor_name}`}
                      aria-pressed={preview?.version === version.id}
                      disabled={busy}
                      onClick={() =>
                        run(async () => {
                          if (!remoteId) return;
                          const result = await read<
                            SharedContent & { chapter_id: number }
                          >("translation", { translation: version.id });
                          if (result.chapter_id === remoteId)
                            setPreview({
                              chapter: result.chapter_id,
                              version: version.id,
                              ...result,
                            });
                        }, "Loading translation…")
                      }
                    >
                      <Icon name="book" size={16} />
                      Preview
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {remoteId && versions.current_page < versions.last_page && (
              <button
                type="button"
                className="btn btn--small"
                disabled={busy}
                onClick={() =>
                  run(() => browseChapter(remoteId, versions.current_page + 1))
                }
              >
                <Icon name="right" size={16} />
                More translations
              </button>
            )}
            {preview && (
              <div className="sharing-form">
                <SharedPreview value={preview} />
                {preview.chapter === mapped && local ? (
                  <div className="button-row sharing-row-end">
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={busy}
                      onClick={() => {
                        if (
                          !window.confirm(
                            `Replace your translation of ${local.title} with this one? Your current text will be overwritten.`,
                          )
                        )
                          return;
                        run(async () => {
                          if (!(await onApply(preview)))
                            throw new Error("Could not save the current chapter.");
                          onComplete(`Translation copied into ${local.title}.`);
                        }, "Saving…");
                      }}
                    >
                      <Icon name="download" />
                      Use in {local.title}
                    </button>
                  </div>
                ) : local && remoteId ? (
                  <div className="sharing-hint">
                    <p className="dialog__muted sharing-hint__text">
                      This SagarNet chapter isn’t linked to {local.title}. Link it to copy
                      the translation into your chapter.
                    </p>
                    <button
                      type="button"
                      className="btn btn--small"
                      disabled={busy}
                      onClick={() => run(() => confirmMapping(remoteId))}
                    >
                      <Icon name="link" size={16} />
                      Link to {local.title}
                    </button>
                  </div>
                ) : (
                  <p className="dialog__muted">
                    Open a chapter in the workspace to use this translation.
                  </p>
                )}
              </div>
            )}
          </>
        )}
      </Step>
    </ol>
  );

  return (
    <dialog
      className="sharing-modal"
      ref={modal}
      aria-labelledby="sagarnet-heading"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <section className="dialog sharing">
        <div className="panel__header">
          <div>
            <h2 id="sagarnet-heading" className="panel__heading">
              {submitting ? "Upload to SagarNet" : "Download from SagarNet"}
            </h2>
            <p className="dialog__muted">
              {submitting
                ? "Share your translation of this chapter."
                : "Use a translation someone shared for this book."}
            </p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="Close"
            title="Close"
            onClick={onClose}
            disabled={busy}
          >
            <Icon name="close" />
          </button>
        </div>

        <dl className="sharing-context">
          <div>
            <dt>Book</dt>
            <dd>{project.title}</dd>
          </div>
          <div>
            <dt>Chapter</dt>
            <dd>{local?.title ?? "None open"}</dd>
          </div>
          {submitting && (
            <div>
              <dt>Language</dt>
              <dd>{project.targetLanguage}</dd>
            </div>
          )}
        </dl>

        {unavailable ? (
          <StatusNotice kind="info">
            SagarNet sharing is not set up in this version of Sagar.
          </StatusNotice>
        ) : (
          <>
            {accountBar}
            {feedback}
            {submitting ? submitSteps : browseSteps}
            <label className="sharing-check sharing-footer">
              <input
                type="checkbox"
                checked={autoMatch}
                onChange={(event) => {
                  setAutoMatch(event.target.checked);
                  persist(AUTO_MATCH_KEY, event.target.checked);
                }}
              />
              Check SagarNet for each book when I open it
            </label>
          </>
        )}
      </section>
    </dialog>
  );
}
