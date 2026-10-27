import { DOMSerializer, DOMParser as ProseMirrorParser } from "@tiptap/pm/model";
import type { Editor as TiptapEditor } from "@tiptap/react";
import { type RefObject, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { HtmlCodeEditor } from "../../components/HtmlCode";
import { Icon } from "../../components/Icon";
import { StatusNotice } from "../../components/StatusNotice";
import { formatChapterHtml } from "../../lib/html/format";
import { useTranslation } from "../../lib/i18n";
import { validateChapterHtml } from "../../lib/tauri/commands";
import type { ChapterContent } from "../../types/chapter";
import { AiFixHtmlDialog } from "./AiFixHtmlDialog";
import { AiGenerateDialog } from "./AiGenerateDialog";
import { useAiJobs } from "./aiJobs";
import { bookImageUrl } from "./bookImages";
import { ChapterEditor } from "./ChapterEditor";
import {
  comparableDocument,
  hasContent,
  normalizeRichDocument,
  sanitizePreviewHtml,
} from "./richText";
import type { useChapterEditor } from "./useChapterEditor";

/** Local validation normally resolves in milliseconds. Bound it so a hung
 * backend call can never leave both view buttons disabled with no feedback. */
const VALIDATE_TIMEOUT_MS = 20_000;

function validateChapterHtmlTimed(html: string): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const timer = setTimeout(
      () =>
        reject(
          new Error(
            "Validation took too long (over 20 seconds) and was stopped. Your code is kept; try the Rich view again.",
          ),
        ),
      VALIDATE_TIMEOUT_MS,
    );
    validateChapterHtml(html).then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

export function TargetPane({
  projectId,
  content,
  loading,
  error,
  onRetry,
  onReloadAfterConflict,
  editor,
  scrollRef,
  immersive,
  onEnterReadingMode,
  aiOpen,
  onOpenAi,
  onCloseAi,
}: {
  projectId: string;
  content: ChapterContent | null;
  loading: boolean;
  error: string | null;
  onRetry: () => void;
  onReloadAfterConflict: () => void;
  editor: ReturnType<typeof useChapterEditor>;
  scrollRef?: RefObject<HTMLDivElement | null>;
  immersive: boolean;
  onEnterReadingMode: (trigger: HTMLElement) => void;
  aiOpen: boolean;
  onOpenAi: () => void;
  onCloseAi: () => void;
}) {
  const { t } = useTranslation();
  const { jobs, markInserted } = useAiJobs();
  // Scope readiness to the chapter instead of clearing it in a parent effect:
  // child mount effects can report readiness before that effect runs.
  const [readyEditor, setReadyEditor] = useState<{
    chapterId: string | undefined;
    editor: TiptapEditor;
  } | null>(null);
  const chapterId = content?.chapterId;
  const activeEditor =
    readyEditor && readyEditor.chapterId === chapterId ? readyEditor.editor : null;
  const handleEditorReady = useCallback(
    (editor: TiptapEditor) => setReadyEditor({ chapterId, editor }),
    [chapterId],
  );
  const [, refreshToolbar] = useState(0);
  const [imageError, setImageError] = useState<string | null>(null);
  const [imagePicker, setImagePicker] = useState(false);
  const [fixConfirmOpen, setFixConfirmOpen] = useState(false);
  const [formattingOpen, setFormattingOpen] = useState(false);
  const [view, setView] = useState<"rich" | "html">("rich");
  const [viewError, setViewError] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);
  const viewRef = useRef(view);
  viewRef.current = view;
  const currentHtml = useRef("");
  const draftHtml = editor.draft?.html ?? content?.document.html ?? "";
  currentHtml.current = draftHtml;
  const readingHtml = useMemo(
    () =>
      immersive && content
        ? sanitizePreviewHtml(draftHtml, (source) =>
            bookImageUrl(source, `epubres://${projectId}/${content.resourceHref}`),
          )
        : "",
    [content, draftHtml, immersive, projectId],
  );
  const fileInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!activeEditor) return;
    const update = () => refreshToolbar((value) => value + 1);
    activeEditor.on("transaction", update);
    return () => {
      activeEditor.off("transaction", update);
    };
  }, [activeEditor]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset toolbar when navigating to another chapter.
  useEffect(() => {
    setImagePicker(false);
    setImageError(null);
    setView("rich");
    setViewError(null);
    setFixConfirmOpen(false);
    setFormattingOpen(false);
  }, [content?.chapterId]);
  const canEdit =
    view === "rich" &&
    !!activeEditor &&
    !activeEditor.isDestroyed &&
    activeEditor.isEditable;
  const aiRunning = jobs.some(
    (job) => job.chapterId === chapterId && job.state === "running" && !job.foreground,
  );
  const showRichText = useCallback(async () => {
    if (!activeEditor || activeEditor.isDestroyed) {
      // The chapter editor is still mounting (or was destroyed on chapter
      // switch). Surface feedback instead of silently ignoring the click,
      // which previously looked like the Rich button was dead.
      setViewError("The rich editor is still loading. Try again in a moment.");
      return;
    }
    const html = currentHtml.current;
    const destination = activeEditor;
    setSwitching(true);
    setViewError(null);
    try {
      const validated = normalizeRichDocument(await validateChapterHtmlTimed(html));
      if (currentHtml.current !== html || destination.isDestroyed) return;
      const wrapper = document.createElement("div");
      wrapper.innerHTML = validated;
      const parsed = ProseMirrorParser.fromSchema(destination.schema).parse(wrapper, {
        preserveWhitespace: true,
      });
      const roundTrip = document.createElement("div");
      roundTrip.append(
        DOMSerializer.fromSchema(destination.schema).serializeFragment(parsed.content),
      );
      if (
        validated.trim() !== "" &&
        comparableDocument(validated) !== comparableDocument(roundTrip.innerHTML)
      ) {
        setView("html");
        setViewError(
          "This HTML cannot be represented faithfully in rich text. Your code is kept; continue editing it here or adjust its structure.",
        );
        return;
      }
      // Apply the exact document that passed the preservation check, rather
      // than parsing the HTML a second time with different whitespace options.
      destination.commands.setContent(parsed.toJSON(), { emitUpdate: false });
      if (validated !== html) editor.setHtml(validated);
      setView("rich");
    } catch (error) {
      if (currentHtml.current !== html || destination.isDestroyed) return;
      setView("html");
      setViewError(
        error instanceof Error
          ? error.message
          : "Check your HTML before switching to rich text.",
      );
    } finally {
      setSwitching(false);
    }
  }, [activeEditor, editor.setHtml]);
  useEffect(() => {
    if (view !== "rich" || !activeEditor || activeEditor.isDestroyed) return;
    const displayed = activeEditor.getHTML();
    // Ordinary editor keystrokes already updated the draft. Do not reload
    // those transactions: doing so would reset the caret and undo history.
    if (draftHtml.trim() === "" && activeEditor.isEmpty) return;
    if (comparableDocument(draftHtml) === comparableDocument(displayed)) return;
    // AI insertion / external draft updates must refresh the mounted editor
    // through the same validation and preservation checks as HTML → Rich.
    void showRichText();
  }, [activeEditor, draftHtml, showRichText, view]);
  // A translation that finished while the dialog was in the background fills
  // an empty draft, exactly like a run that stayed in the foreground.
  useEffect(() => {
    if (!chapterId) return;
    const job = jobs.find(
      (entry) =>
        entry.kind === "chapter" &&
        entry.chapterId === chapterId &&
        entry.state === "done" &&
        entry.proposalHtml &&
        !entry.inserted &&
        !entry.foreground,
    );
    if (!job?.proposalHtml) return;
    const draft = editor.draft;
    if (!draft || draft.conflict) return;
    if (hasContent(draft.html)) return;
    editor.setHtml(job.proposalHtml);
    markInserted(job.jobId);
  }, [chapterId, editor.draft, editor.setHtml, jobs, markInserted]);

  const insertImage = (src: string, alt: string) => {
    if (canEdit) activeEditor.chain().focus().setImage({ src, alt }).run();
    setImagePicker(false);
  };
  const showHtml = useCallback(() => {
    // Rich edits and AI insertions save as a single line; normalize to one
    // block per line so the code view reads like the original source.
    // HTML typing itself is left alone to preserve the caret.
    const current = editor.draft?.html ?? "";
    const pretty = formatChapterHtml(current);
    if (editor.draft && pretty !== current) editor.setHtml(pretty);
    setImagePicker(false);
    setViewError(null);
    setView("html");
  }, [editor]);
  return (
    <section
      className={`pane pane--target${immersive ? " pane--immersive" : ""}`}
      aria-labelledby={immersive ? undefined : "target-pane-heading"}
      aria-label={immersive ? t("Translation chapter content") : undefined}
    >
      <div className="pane__header">
        <h2 className="pane__heading" id="target-pane-heading">
          {t("Translation")}
        </h2>
        <fieldset
          className="view-switch view-switch--icons"
          aria-label={t("Translation view")}
        >
          <button
            type="button"
            className="icon-button"
            aria-label={t("Rich text view")}
            title={
              !activeEditor ? t("Rich text view (editor loading)") : t("Rich text view")
            }
            aria-pressed={view === "rich"}
            disabled={
              switching || !editor.draft || editor.draft.conflict || !activeEditor
            }
            onClick={() => void showRichText()}
          >
            <Icon name="richText" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={t("HTML view")}
            title={t("HTML view")}
            aria-pressed={view === "html"}
            disabled={switching || !editor.draft}
            onClick={showHtml}
          >
            <Icon name="code" />
          </button>
        </fieldset>
        <button
          type="button"
          className="icon-button"
          disabled={!editor.draft || editor.draft.conflict}
          aria-label={
            aiRunning
              ? t("AI translation is running in the background")
              : t("Generate an AI translation proposal")
          }
          title={
            aiRunning
              ? t("AI translation is running in the background")
              : t("Generate an AI translation proposal")
          }
          onClick={onOpenAi}
        >
          <Icon name="sparkles" spin={aiRunning} />
        </button>
        {view === "rich" ? (
          <button
            type="button"
            className="icon-button"
            aria-label={t("Formatting tools")}
            title={t("Formatting tools")}
            aria-expanded={formattingOpen}
            aria-controls="translation-formatting"
            disabled={!editor.draft || editor.draft.conflict}
            onClick={() => setFormattingOpen((open) => !open)}
          >
            <Icon name="format" />
          </button>
        ) : null}
        <div className="chapter-review-actions">
          <button
            type="button"
            className="icon-button chapter-review-button"
            aria-pressed={editor.draft?.status === "reviewed"}
            aria-label={
              editor.draft?.status === "reviewed"
                ? t("Chapter reviewed")
                : t("Mark chapter reviewed")
            }
            title={
              editor.draft?.status === "reviewed"
                ? t("Chapter reviewed")
                : t("Mark chapter reviewed")
            }
            disabled={
              !editor.draft || !hasContent(editor.draft.html) || editor.draft.conflict
            }
            onClick={() => editor.setStatus("reviewed")}
          >
            <Icon name="check" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={
              editor.draft?.status === "skipped"
                ? t("Write translation")
                : t("Keep original chapter")
            }
            title={
              editor.draft?.status === "skipped"
                ? t("Write translation")
                : t("Keep original chapter")
            }
            disabled={!editor.draft || editor.draft.conflict}
            onClick={() =>
              editor.setStatus(editor.draft?.status === "skipped" ? "draft" : "skipped")
            }
          >
            <Icon name={editor.draft?.status === "skipped" ? "edit" : "keep"} />
          </button>
        </div>
        <button
          type="button"
          className="icon-button"
          aria-label={t("Read translation in full view")}
          title={t("Read translation in full view")}
          disabled={!content || !editor.draft || loading || !!error}
          onClick={(event) => onEnterReadingMode(event.currentTarget)}
        >
          <Icon name="expand" />
        </button>
      </div>
      <div
        id="translation-formatting"
        className="editor-toolbar"
        role="toolbar"
        aria-label={t("Translation formatting")}
        hidden={!formattingOpen || view !== "rich"}
      >
        {(
          [
            ["Bold", "bold", "B"],
            ["Italic", "italic", "I"],
            ["Underline", "underline", "U"],
            ["Strike", "strike", "S"],
          ] as const
        ).map(([label, mark]) => (
          <button
            type="button"
            key={mark}
            title={t(label)}
            aria-label={t(label)}
            aria-pressed={canEdit && activeEditor.isActive(mark)}
            disabled={!canEdit}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor?.chain().focus().toggleMark(mark).run()}
          >
            <Icon name={mark} size={16} />
          </button>
        ))}
        <span className="editor-toolbar__divider" />
        <button
          type="button"
          aria-label={t("Inline code")}
          className="icon-button"
          title={t("Inline code (Cmd/Ctrl + E)")}
          aria-pressed={canEdit && activeEditor.isActive("code")}
          disabled={!canEdit || !activeEditor.can().toggleCode()}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => activeEditor?.chain().focus().toggleCode().run()}
        >
          <Icon name="code" size={17} />
        </button>
        <button
          type="button"
          aria-label={t("Blockquote")}
          className="icon-button"
          title={t("Blockquote")}
          aria-pressed={canEdit && activeEditor.isActive("blockquote")}
          disabled={!canEdit || !activeEditor.can().toggleBlockquote()}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => activeEditor?.chain().focus().toggleBlockquote().run()}
        >
          <Icon name="blockquote" size={17} />
        </button>
        <button
          type="button"
          aria-label={t("Code block")}
          className="icon-button"
          title={t("Code block")}
          aria-pressed={canEdit && activeEditor.isActive("codeBlock")}
          disabled={!canEdit || !activeEditor.can().toggleCodeBlock()}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => activeEditor?.chain().focus().toggleCodeBlock().run()}
        >
          <Icon name="codeBlock" size={17} />
        </button>
        <span className="editor-toolbar__divider" />
        <button
          type="button"
          aria-label={t("Paragraph")}
          title={t("Paragraph (Cmd/Ctrl + Alt + 0)")}
          aria-pressed={canEdit && activeEditor.isActive("paragraph")}
          disabled={!canEdit}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => activeEditor?.chain().focus().setParagraph().run()}
        >
          <Icon name="paragraph" size={17} />
        </button>
        {([1, 2, 3] as const).map((level) => (
          <button
            key={level}
            type="button"
            className="editor-toolbar__heading"
            aria-label={`Heading ${level}`}
            title={`Heading ${level} (Cmd/Ctrl + Alt + ${level})`}
            aria-pressed={canEdit && activeEditor.isActive("heading", { level })}
            disabled={!canEdit}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor?.chain().focus().setHeading({ level }).run()}
          >
            <Icon name="richText" />H{level}
          </button>
        ))}
        <span className="editor-toolbar__divider" />
        <button
          type="button"
          disabled={!canEdit}
          aria-label={t("Bullets")}
          title={t("Bulleted list")}
          aria-pressed={canEdit && activeEditor.isActive("bulletList")}
          onClick={() => activeEditor?.chain().focus().toggleBulletList().run()}
        >
          <Icon name="list" size={17} />
        </button>
        <button
          type="button"
          disabled={!canEdit}
          aria-label={t("Numbered list")}
          title={t("Numbered list")}
          aria-pressed={canEdit && activeEditor.isActive("orderedList")}
          onClick={() => activeEditor?.chain().focus().toggleOrderedList().run()}
        >
          <Icon name="numbered" size={17} />
        </button>
        <span className="editor-toolbar__divider" />
        <button
          type="button"
          disabled={!canEdit || !activeEditor.can().undo()}
          aria-label={t("Undo")}
          title={t("Undo")}
          onClick={() => activeEditor?.chain().focus().undo().run()}
        >
          <Icon name="undo" size={17} />
        </button>
        <button
          type="button"
          disabled={!canEdit || !activeEditor.can().redo()}
          aria-label={t("Redo")}
          title={t("Redo")}
          onClick={() => activeEditor?.chain().focus().redo().run()}
        >
          <Icon name="redo" size={17} />
        </button>
        <span className="editor-toolbar__divider" />
        <button
          type="button"
          disabled={!canEdit}
          aria-label={t("Insert image")}
          title={t("Insert image")}
          onClick={() => setImagePicker(!imagePicker)}
        >
          <Icon name="image" size={17} />
        </button>
        <input
          ref={fileInput}
          type="file"
          hidden
          accept="image/png,image/jpeg,image/gif"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (!file) return;
            if (
              file.size > 8 * 1024 * 1024 ||
              !["image/png", "image/jpeg", "image/gif"].includes(file.type)
            ) {
              setImageError("Choose a PNG, JPEG or GIF under 8 MB.");
              return;
            }
            const destination = activeEditor;
            const reader = new FileReader();
            reader.onerror = () =>
              setImageError("The image could not be read. Try another file.");
            reader.onload = () => {
              if (
                viewRef.current !== "rich" ||
                !destination ||
                destination.isDestroyed ||
                !destination.isEditable ||
                typeof reader.result !== "string"
              )
                return;
              destination
                .chain()
                .focus()
                .setImage({
                  src: reader.result,
                  alt: file.name.replace(/\.[^.]+$/, ""),
                })
                .run();
              setImagePicker(false);
              setImageError(null);
            };
            reader.readAsDataURL(file);
          }}
        />
      </div>
      {canEdit && activeEditor.isActive("table") ? (
        <div className="table-toolbar" role="toolbar" aria-label={t("Table editing")}>
          <span>{t("Table")}</span>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Add row below")}
            title={t("Add row below")}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor.chain().focus().addRowAfter().run()}
          >
            <Icon name="rowAdd" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Add column after")}
            title={t("Add column after")}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor.chain().focus().addColumnAfter().run()}
          >
            <Icon name="columnAdd" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Delete row")}
            title={t("Delete row")}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor.chain().focus().deleteRow().run()}
          >
            <Icon name="rowRemove" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Delete column")}
            title={t("Delete column")}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor.chain().focus().deleteColumn().run()}
          >
            <Icon name="columnRemove" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={t("Delete table")}
            title={t("Delete table")}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => activeEditor.chain().focus().deleteTable().run()}
          >
            <Icon name="trash" />
          </button>
        </div>
      ) : null}
      {imagePicker ? (
        <div className="image-picker">
          <div className="image-picker__heading">
            <strong>{t("Insert into your translation")}</strong>
            <button
              type="button"
              className="btn btn--small"
              onClick={() => setImagePicker(false)}
            >
              <Icon name="close" />
              {t("Close")}
            </button>
          </div>
          <button
            type="button"
            className="btn"
            onClick={() => fileInput.current?.click()}
          >
            <Icon name="upload" />
            {t("Import image from computer…")}
          </button>
          <p className="dialog__muted">{t("Images from this chapter")}</p>
          {content?.images.length ? (
            <div className="image-picker__grid">
              {content.images.map((image) => (
                <button
                  type="button"
                  key={image.src}
                  onClick={() => insertImage(image.src, image.alt)}
                >
                  <Icon name="image" />
                  <img src={image.src} alt={image.alt} />
                  <span>{image.alt || "Chapter image"}</span>
                </button>
              ))}
            </div>
          ) : (
            <p className="dialog__muted">
              {t("No supported PNG, JPEG or GIF images in this chapter.")}
            </p>
          )}
        </div>
      ) : null}
      {imageError ? <StatusNotice kind="error">{imageError}</StatusNotice> : null}
      <div className="pane__scroll" ref={scrollRef}>
        {immersive && !loading && !error && content && editor.draft ? (
          hasContent(draftHtml) ? (
            <article
              className="book-text workspace__reading-copy workspace__reading-copy--translation"
              aria-label={t("Translation chapter content")}
              data-reading-content
              tabIndex={-1}
            >
              {/* Reading mode renders the draft through the same active-content scrubber as proposal previews. */}
              {/* biome-ignore lint/security/noDangerouslySetInnerHtml: read-only HTML has been scrubbed for presentation */}
              <div dangerouslySetInnerHTML={{ __html: readingHtml }} />
            </article>
          ) : (
            <p
              className="workspace__reading-empty"
              data-reading-content
              tabIndex={-1}
              role="status"
            >
              {t("No translation has been written for this chapter yet.")}
            </p>
          )
        ) : null}
        {loading ? (
          <StatusNotice kind="loading">{t("Loading chapter…")}</StatusNotice>
        ) : null}
        {error ? (
          <StatusNotice kind="error" onRetry={onRetry}>
            {error}
          </StatusNotice>
        ) : null}
        {!loading && !error && content ? (
          <div className="chapter-document" hidden={immersive}>
            {editor.draft?.error ? (
              <StatusNotice
                kind="error"
                onRetry={editor.draft.conflict ? onReloadAfterConflict : editor.retry}
              >
                {editor.draft.error}
              </StatusNotice>
            ) : null}
            {editor.draft?.status === "skipped" ? (
              <StatusNotice kind="info">
                {t("This chapter will keep the original. Your draft remains saved.")}
              </StatusNotice>
            ) : null}
            {view === "html" ? (
              <>
                {viewError ? (
                  <StatusNotice
                    kind="error"
                    action={
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={t("Fix with AI")}
                        title={t("Fix with AI")}
                        disabled={!editor.draft || editor.draft.conflict}
                        onClick={() => setFixConfirmOpen(true)}
                      >
                        <Icon name="sparkles" />
                      </button>
                    }
                  >
                    {viewError}
                  </StatusNotice>
                ) : null}
                <HtmlCodeEditor
                  key={content.chapterId}
                  value={editor.draft?.html ?? content.document.html ?? ""}
                  disabled={(editor.draft?.conflict ?? false) || editor.appending}
                  onChange={(value) => {
                    setViewError(null);
                    editor.setHtml(value);
                  }}
                />
              </>
            ) : null}
            <div hidden={view === "html"}>
              <ChapterEditor
                imageBase={`epubres://${projectId}/${content.resourceHref}`}
                key={content.chapterId}
                html={editor.draft?.html ?? content.document.html ?? ""}
                disabled={(editor.draft?.conflict ?? false) || editor.appending}
                onChange={editor.setHtml}
                onReady={handleEditorReady}
                onImage={() => setImagePicker(true)}
                active={view === "rich"}
              />
            </div>
          </div>
        ) : null}
        {!immersive && !loading && !error && !content ? (
          <StatusNotice kind="info">{t("No chapter selected.")}</StatusNotice>
        ) : null}
      </div>
      {fixConfirmOpen && content ? (
        <AiFixHtmlDialog
          chapterId={content.chapterId}
          html={editor.draft?.html ?? content.document.html ?? ""}
          onApply={(fixedHtml) => {
            setFixConfirmOpen(false);
            setViewError(null);
            editor.setHtml(formatChapterHtml(fixedHtml));
            // Keep the corrected code visible. Rich conversion remains an
            // explicit action with the usual validation/preservation checks.
            setView("html");
          }}
          onClose={() => setFixConfirmOpen(false)}
        />
      ) : null}
      {aiOpen && content ? (
        <AiGenerateDialog
          projectId={projectId}
          chapterId={content.chapterId}
          chapterTitle={content.title}
          editor={editor}
          onClose={onCloseAi}
        />
      ) : null}
    </section>
  );
}
