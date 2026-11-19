import {
  forwardRef,
  type ReactNode,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
} from "react";
import { highlightCss } from "../../lib/html/highlight";

export type SearchMatch = { start: number; end: number };

export type CssCodeEditorHandle = {
  focus: () => void;
  /** Focuses the textarea and selects the given range (e.g. a search match). */
  select: (match: SearchMatch) => void;
};

/** Case-insensitive plain-text matches; non-overlapping, in document order. */
export function findMatches(text: string, query: string): SearchMatch[] {
  if (query === "") return [];
  const haystack = text.toLocaleLowerCase();
  const needle = query.toLocaleLowerCase();
  // Lower-casing can change string length for a few scripts; fall back to
  // exact matching then so offsets always point into the original text.
  const exact = haystack.length !== text.length || needle.length !== query.length;
  const source = exact ? text : haystack;
  const target = exact ? query : needle;
  const matches: SearchMatch[] = [];
  let from = 0;
  while (matches.length < 10_000) {
    const index = source.indexOf(target, from);
    if (index < 0) break;
    matches.push({ start: index, end: index + target.length });
    from = index + target.length;
  }
  return matches;
}

/**
 * A native textarea owns all editing (caret, IME, undo). Two inert layers sit
 * behind it: syntax colors and search-match marks, kept in the same font and
 * wrapping so they line up with the text.
 */
export const CssCodeEditor = forwardRef<
  CssCodeEditorHandle,
  {
    value: string;
    label: string;
    disabled: boolean;
    matches: SearchMatch[];
    currentMatch: number;
    onChange: (value: string) => void;
  }
>(function CssCodeEditor(
  { value, label, disabled, matches, currentMatch, onChange },
  ref,
) {
  const textarea = useRef<HTMLTextAreaElement>(null);
  const marks = useRef<HTMLPreElement>(null);
  const highlighted = useMemo(() => highlightCss(value), [value]);

  useImperativeHandle(ref, () => ({
    focus: () => textarea.current?.focus(),
    select: ({ start, end }) => {
      textarea.current?.focus({ preventScroll: true });
      textarea.current?.setSelectionRange(start, end);
    },
  }));

  const markNodes = useMemo(() => {
    const nodes: ReactNode[] = [];
    let cursor = 0;
    matches.forEach((match, index) => {
      nodes.push(value.slice(cursor, match.start));
      nodes.push(
        <mark
          key={match.start}
          className={index === currentMatch ? "is-current" : undefined}
          data-current={index === currentMatch ? "" : undefined}
        >
          {value.slice(match.start, match.end)}
        </mark>,
      );
      cursor = match.end;
    });
    nodes.push(`${value.slice(cursor)}\n`);
    return nodes;
  }, [value, matches, currentMatch]);

  // Bring the current match into view inside the scrolling editor.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the marked element moves whenever the match list changes.
  useEffect(() => {
    if (currentMatch < 0) return;
    marks.current
      ?.querySelector("mark[data-current]")
      ?.scrollIntoView({ block: "center", inline: "nearest" });
  }, [currentMatch, matches]);

  return (
    <div className="css-code-editor">
      <pre ref={marks} className="css-code-editor__marks" aria-hidden="true">
        {markNodes}
      </pre>
      <pre className="css-code-editor__highlight css-syntax" aria-hidden="true">
        {/* biome-ignore lint/security/noDangerouslySetInnerHtml: highlightCss escapes all stylesheet text; only generated token spans are HTML. */}
        <code dangerouslySetInnerHTML={{ __html: `${highlighted}\n` }} />
      </pre>
      <textarea
        ref={textarea}
        className="css-code-editor__input"
        aria-label={label}
        value={value}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        autoComplete="off"
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
});
