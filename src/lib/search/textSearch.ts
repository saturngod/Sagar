/**
 * In-chapter text search over rendered DOM text.
 *
 * Matching only reads text nodes and returns DOM Ranges; it never mutates the
 * searched content, so it is safe over the read-only preview, highlighted
 * code and the live rich editor alike.
 */

/** Elements whose boundaries separate words, so matches never span them. */
const BLOCK_SELECTOR =
  "address,article,aside,blockquote,caption,dd,div,dl,dt,figcaption,figure,footer,h1,h2,h3,h4,h5,h6,header,hr,li,main,nav,ol,p,pre,section,table,tbody,td,tfoot,th,thead,tr,ul";

/** Text inside these never renders as chapter text. */
const SKIPPED_SELECTOR = "script,style,template,textarea,[hidden]";

/** Never appears in a single-line query, so it reliably breaks matches. */
const BOUNDARY = "\n";

const SHOW_ELEMENT = 1;
const SHOW_TEXT = 4;

/**
 * Lowercases without changing string length, so match offsets computed on the
 * folded text stay valid for the original text nodes.
 */
export function foldCase(text: string): string {
  const lowered = text.toLowerCase();
  if (lowered.length === text.length) return lowered;
  let out = "";
  for (const unit of text.split("")) {
    const lower = unit.toLowerCase();
    out += lower.length === 1 ? lower : unit;
  }
  return out;
}

type Segment = { node: Text; start: number };

/**
 * Finds every case-insensitive occurrence of `query` in the text rendered
 * under `root`, in document order. Matches may cross inline markup
 * (`hel<em>lo</em>`) but not block boundaries or line breaks.
 */
export function findTextMatches(root: Node, query: string): Range[] {
  const doc = root.ownerDocument;
  if (!doc || query === "") return [];
  const needle = foldCase(query);

  const segments: Segment[] = [];
  let haystack = "";
  let lastBlock: Element | null | undefined;
  let pendingBreak = false;
  const walker = doc.createTreeWalker(root, SHOW_ELEMENT | SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.nodeType === 1) {
      if ((node as Element).localName === "br") pendingBreak = true;
      continue;
    }
    const text = node as Text;
    const parent = text.parentElement;
    if (!parent || text.data === "" || parent.closest(SKIPPED_SELECTOR)) continue;
    const block = parent.closest(BLOCK_SELECTOR);
    if (segments.length > 0 && (pendingBreak || block !== lastBlock)) {
      haystack += BOUNDARY;
    }
    pendingBreak = false;
    lastBlock = block;
    segments.push({ node: text, start: haystack.length });
    haystack += foldCase(text.data);
  }

  const matches: Range[] = [];
  let segment = 0;
  for (
    let at = haystack.indexOf(needle);
    at !== -1;
    at = haystack.indexOf(needle, at + needle.length)
  ) {
    const end = at + needle.length;
    while (segment + 1 < segments.length && segments[segment + 1].start <= at) {
      segment += 1;
    }
    let last = segment;
    while (last + 1 < segments.length && segments[last + 1].start < end) last += 1;
    const first = segments[segment];
    const final = segments[last];
    const range = doc.createRange();
    range.setStart(first.node, Math.min(at - first.start, first.node.data.length));
    range.setEnd(final.node, Math.min(end - final.start, final.node.data.length));
    matches.push(range);
  }
  return matches;
}

/** Nearest ancestor that actually scrolls vertically. */
function scrollParent(node: Node): HTMLElement | null {
  let element = node instanceof HTMLElement ? node : node.parentElement;
  while (element) {
    const overflow = getComputedStyle(element).overflowY;
    if (
      (overflow === "auto" || overflow === "scroll") &&
      element.scrollHeight > element.clientHeight
    ) {
      return element;
    }
    element = element.parentElement;
  }
  return null;
}

/** Index of the first match at or below the top of the visible area. */
export function firstVisibleMatch(matches: Range[]): number {
  if (matches.length === 0) return 0;
  const scroller = scrollParent(matches[0].startContainer);
  if (!scroller) return 0;
  const top = scroller.getBoundingClientRect().top;
  const index = matches.findIndex((range) => range.getBoundingClientRect().bottom > top);
  return index === -1 ? 0 : index;
}

/** Scrolls the pane only when the match is outside the comfortable area. */
export function scrollMatchIntoView(range: Range) {
  const scroller = scrollParent(range.startContainer);
  if (!scroller) return;
  const rect = range.getBoundingClientRect();
  const view = scroller.getBoundingClientRect();
  const margin = Math.min(48, view.height / 4);
  if (rect.top >= view.top + margin && rect.bottom <= view.bottom - margin) return;
  scroller.scrollTop += rect.top - view.top - view.height / 3;
}

const MATCH_HIGHLIGHT = "sagar-search";
const CURRENT_HIGHLIGHT = "sagar-search-current";

function highlightRegistry(name: string, priority: number): Highlight | null {
  if (
    typeof CSS === "undefined" ||
    typeof Highlight === "undefined" ||
    !("highlights" in CSS)
  ) {
    return null;
  }
  let highlight = CSS.highlights.get(name);
  if (!highlight) {
    highlight = new Highlight();
    highlight.priority = priority;
    CSS.highlights.set(name, highlight);
  }
  return highlight;
}

/**
 * Paints matches with the CSS Custom Highlight API (no DOM changes). Both
 * panes share the two named highlights; each caller removes only its own
 * ranges through the returned cleanup.
 */
export function paintMatches(matches: Range[], current: Range | null): () => void {
  const all = highlightRegistry(MATCH_HIGHLIGHT, 0);
  const active = highlightRegistry(CURRENT_HIGHLIGHT, 1);
  if (!all || !active) {
    // Older WebViews: show at least the current match through the selection.
    if (!current) return () => {};
    const selection = current.startContainer.ownerDocument?.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(current.cloneRange());
    return () => {};
  }
  for (const range of matches) all.add(range);
  if (current) active.add(current);
  return () => {
    for (const range of matches) all.delete(range);
    if (current) active.delete(current);
  };
}
