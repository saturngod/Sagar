import { useState } from "react";
import { Icon } from "../../components/Icon";
import { useTranslation } from "../../lib/i18n";
import { sagarnetRequest } from "../../lib/sagarnet/link";
import { toCommandError } from "../../lib/tauri/errors";

/** What the backend did with a table of contents upload. */
export type TocResult =
  | "uploaded"
  | "present"
  | "not_owner"
  | "different_edition"
  | "none"
  | "too_large"
  | "failed";

/** Explains a TOC result that did not upload anything, or null when nothing needs saying. */
export function tocResultMessage(result: TocResult): string | null {
  switch (result) {
    case "uploaded":
    case "present":
      return null;
    case "none":
      return "This EPUB has no table of contents to upload.";
    case "not_owner":
      return "Only the book’s owner can change its table of contents.";
    case "different_edition":
      return "This SagarNet copy is a different edition, so its table of contents can’t come from your EPUB.";
    case "too_large":
      return "This table of contents has more than 5,000 entries, which SagarNet doesn’t accept.";
    case "failed":
      return "The book was added, but its table of contents couldn’t be uploaded. Try again from the book details.";
  }
}

type Props = {
  project: string;
  book: number;
  edition: number;
  /** From the edition on book detail; undefined when the server doesn't report it. */
  hasToc: boolean | undefined;
  canManage: boolean;
  disabled?: boolean;
  /** Shown until the next attempt, such as why the upload after creating the book failed. */
  initialError?: string | null;
};

/**
 * One line in the SagarNet book details showing whether the edition has a
 * table of contents, with Upload or Replace for the book's owner. Remount it
 * (via `key`) when the book or edition changes.
 */
export function TocStatus({
  project,
  book,
  edition,
  hasToc,
  canManage,
  disabled,
  initialError,
}: Props) {
  const { t } = useTranslation();
  const [present, setPresent] = useState(hasToc);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{
    kind: "info" | "error";
    text: string;
  } | null>(initialError ? { kind: "error", text: initialError } : null);

  if (present === undefined) return null;

  async function upload(replace: boolean) {
    if (
      replace &&
      !window.confirm(
        t(
          "Replace this book’s table of contents on SagarNet with the one from your EPUB?",
        ),
      )
    )
      return;
    setBusy(true);
    setMessage(null);
    try {
      const { toc } = await sagarnetRequest<{ toc: TocResult }>("sync_toc", {
        project,
        book,
        edition,
        replace,
      });
      if (toc === "uploaded" || toc === "present") {
        setPresent(true);
        setMessage({
          kind: "info",
          text: t(
            toc === "uploaded"
              ? "Table of contents uploaded."
              : "SagarNet already has a table of contents for this book.",
          ),
        });
      } else {
        setMessage({ kind: "error", text: t(tocResultMessage(toc) ?? "") });
      }
    } catch (cause) {
      setMessage({ kind: "error", text: toCommandError(cause).message });
    } finally {
      setBusy(false);
    }
  }

  const status = present
    ? t("Table of contents: uploaded")
    : canManage
      ? t(
          "Table of contents: not uploaded. SagarNet shows chapter names only, without volumes or nesting.",
        )
      : t("Table of contents: not uploaded. Only the book’s owner can add it.");

  return (
    <div className="sharing-hint toc-status" aria-busy={busy}>
      <p className="dialog__muted sharing-hint__text">
        <span className="toc-status__icon">
          <Icon name="list" size={16} />
        </span>
        {status}
      </p>
      {canManage && (
        <button
          type="button"
          className="btn btn--small"
          disabled={disabled || busy}
          onClick={() => void upload(Boolean(present))}
        >
          <Icon name={present ? "refresh" : "upload"} size={16} spin={busy && present} />
          {busy
            ? t("Uploading…")
            : present
              ? t("Replace")
              : message?.kind === "error"
                ? t("Retry")
                : t("Upload table of contents")}
        </button>
      )}
      {message && (
        <p
          className={`toc-status__message${message.kind === "error" ? " toc-status__message--error" : ""}`}
          role={message.kind === "error" ? "alert" : "status"}
        >
          {message.text}
        </p>
      )}
    </div>
  );
}
