/** Only the restricted editor's inline nodes reach the Rust validator. */
export function plainFragment(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/\n/g, "<br/>");
}
export function hasContent(html: string): boolean {
  const doc = new DOMParser().parseFromString(html, "text/html");
  return (
    (doc.body.textContent ?? "").trim() !== "" ||
    doc.querySelector("img, svg, table") !== null
  );
}
export function editorContent(fragment: string): string {
  return `<p>${fragment}</p>`;
}
export function inlineFragment(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const serialize = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) return plainFragment(node.textContent ?? "");
    if (!(node instanceof Element)) return "";
    const children = Array.from(node.childNodes).map(serialize).join("");
    if (node.tagName === "P") return children;
    const tag = node.tagName.toLowerCase();
    if (tag === "br") return "<br/>";
    if (tag === "img")
      return `<img src="${plainFragment(node.getAttribute("src") ?? "")}" alt="${plainFragment(node.getAttribute("alt") ?? "")}"/>`;
    if (["strong", "em", "u", "s"].includes(tag)) return `<${tag}>${children}</${tag}>`;
    if (tag === "a")
      return `<a href="${plainFragment(node.getAttribute("href") ?? "")}">${children}</a>`;
    return children;
  };
  return Array.from(doc.body.childNodes).map(serialize).join("<br/><br/>");
}

/**
 * Canonical document XHTML: retain the reader's block structure (including
 * original div/figure/section wrappers) and SVG cover artwork so the
 * rich-text preservation check can tell lossy conversions apart instead of
 * silently dropping them.
 */
const SVG_TAGS = [
  "svg",
  "g",
  "image",
  "path",
  "rect",
  "circle",
  "ellipse",
  "line",
  "polyline",
  "polygon",
  "title",
  "desc",
  "text",
  "tspan",
];
const SVG_ATTRS = [
  "href",
  "xlink:href",
  "viewBox",
  "preserveAspectRatio",
  "width",
  "height",
  "x",
  "y",
  "x1",
  "x2",
  "y1",
  "y2",
  "cx",
  "cy",
  "r",
  "rx",
  "ry",
  "d",
  "points",
  "transform",
  "opacity",
  "fill",
  "stroke",
  "stroke-width",
  "font-size",
  "text-anchor",
  "version",
];
export function documentFragment(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const serialize = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) {
      const escaped = plainFragment(node.textContent ?? "");
      return node.parentElement?.closest("pre")
        ? escaped.split("<br/>").join("\n")
        : escaped;
    }
    if (!(node instanceof Element)) return "";
    const tag = node.tagName.toLowerCase();
    const allowed = [
      "p",
      "table",
      "thead",
      "tbody",
      "tfoot",
      "tr",
      "td",
      "th",
      "caption",
      "h1",
      "h2",
      "h3",
      "h4",
      "h5",
      "h6",
      "ul",
      "ol",
      "li",
      "blockquote",
      "div",
      "section",
      "hgroup",
      "article",
      "main",
      "header",
      "footer",
      "aside",
      "figure",
      "figcaption",
      "pre",
      "dl",
      "dt",
      "dd",
      "time",
      "span",
      "small",
      "abbr",
      "cite",
      "dfn",
      "kbd",
      "samp",
      "var",
      "mark",
      "b",
      "i",
      "code",
      "sup",
      "sub",
      "ruby",
      "rt",
      "rp",
      "strong",
      "em",
      "u",
      "s",
      "img",
      "br",
      "hr",
      "a",
      "picture",
      "source",
      ...SVG_TAGS,
    ];
    const children = Array.from(node.childNodes).map(serialize).join("");
    if (!allowed.includes(tag)) return children;
    const names = [
      "class",
      "id",
      "style",
      "title",
      "lang",
      "dir",
      "role",
      "data-parent",
      "epub:type",
      "xmlns:epub",
      "xml:lang",
      ...(tag === "div" ? ["align"] : []),
      ...(tag === "code" ? ["data-highlighted"] : []),
      ...(tag === "img"
        ? ["src", "srcset", "sizes", "alt", "width", "height", "loading", "decoding"]
        : tag === "time"
          ? ["datetime"]
          : tag === "source"
            ? ["src", "srcset", "sizes", "media", "type", "width", "height"]
            : tag === "a"
              ? ["href"]
              : tag === "td" || tag === "th"
                ? [
                    "colspan",
                    "rowspan",
                    "width",
                    "height",
                    "align",
                    "valign",
                    "scope",
                    "abbr",
                    "headers",
                  ]
                : tag === "table"
                  ? ["border", "cellpadding", "cellspacing", "width", "align"]
                  : tag === "ol" || tag === "ul"
                    ? ["start", "type"]
                    : tag === "li"
                      ? ["value"]
                      : []),
      ...(SVG_TAGS.includes(tag) ? [...SVG_ATTRS, "xmlns", "xmlns:xlink"] : []),
    ].sort();
    names.push(
      ...Array.from(node.attributes, (attr) => attr.name).filter((name) =>
        name.startsWith("aria-"),
      ),
    );
    const values = new Map<string, string>();
    for (const name of names) {
      // The rich editor stores imported CSS under data-book-style so the
      // browser does not apply it. Saved/exported XHTML restores `style`.
      const value =
        name === "style"
          ? (node.getAttribute("style") ?? node.getAttribute("data-book-style"))
          : node.getAttribute(name);
      if (value !== null) values.set(name, value);
    }
    const attributes = Array.from(
      values,
      ([name, value]) => ` ${name}="${plainFragment(value)}"`,
    ).join("");
    if (
      [
        "br",
        "img",
        "hr",
        "source",
        "image",
        "path",
        "rect",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
      ].includes(tag)
    )
      return `<${tag}${attributes}/>`;
    return `<${tag}${attributes}>${children}</${tag}>`;
  };
  return Array.from(doc.body.childNodes).map(serialize).join("");
}

/** Prepare validated XHTML for Rich editing without changing readable content.
 * Run on conversion only: never during typing or IME composition. */
export function normalizeRichDocument(html: string): string {
  const parser = new DOMParser();
  const doc = parser.parseFromString(html, "text/html");
  // XHTML permits self-closing containers; the HTML parser would treat
  // <header/> as an opening tag and swallow the following chapter into it.
  const xml = parser.parseFromString(
    `<w xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xmlns:xlink="http://www.w3.org/1999/xlink">${html}</w>`,
    "application/xhtml+xml",
  );
  if (!xml.querySelector("parsererror")) {
    doc.body.replaceChildren(
      ...Array.from(xml.documentElement.childNodes, (node) => doc.importNode(node, true)),
    );
  }
  const emptyDecorations =
    "em,strong,u,s,code,b,i,span,h1,h2,h3,h4,h5,h6,div,header,footer";
  for (const element of Array.from(
    doc.body.querySelectorAll(emptyDecorations),
  ).reverse()) {
    // Class names alone do not give an empty decoration content. IDs, language,
    // role, EPUB semantics, styles, labels and all other attributes protect it.
    if (
      element.children.length === 0 &&
      /^[\t\r\n ]*$/.test(element.textContent ?? "") &&
      Array.from(element.attributes).every((attr) => attr.name === "class") &&
      !element.closest("pre,svg,picture,table")
    )
      element.remove();
  }
  const blocks =
    "p,h1,h2,h3,h4,h5,h6,ul,ol,li,blockquote,hr,table,thead,tbody,tfoot,tr,td,th,caption,div,section,hgroup,article,main,header,footer,aside,figure,figcaption,pre,dl,dt,dd,svg,picture";
  for (const parent of Array.from(
    doc.querySelectorAll(
      "body,ul,ol,table,thead,tbody,tfoot,tr,div,section,hgroup,article,main,header,footer,aside,figure,figcaption,dd,li,blockquote,td,th",
    ),
  )) {
    if (Array.from(parent.children).some((child) => child.matches(blocks))) {
      for (const child of Array.from(parent.childNodes)) {
        const besideInline = [child.previousSibling, child.nextSibling].some(
          (sibling) => sibling instanceof Element && !sibling.matches(blocks),
        );
        if (
          child.nodeType === Node.TEXT_NODE &&
          !besideInline &&
          /^[\t\r\n ]*$/.test(child.textContent ?? "")
        )
          child.remove();
      }
    }
  }
  for (const parent of Array.from(
    doc.querySelectorAll(
      "body,div,section,hgroup,article,main,header,footer,aside,figure,figcaption,dd,li,blockquote,td,th",
    ),
  )) {
    if (
      parent.closest("svg,picture") ||
      parent.closest("table")?.querySelector("thead,tfoot,caption")
    )
      continue;
    const needsBlocks =
      parent.matches("body,li,blockquote,td,th") ||
      Array.from(parent.children).some((child) => child.matches(blocks));
    if (!needsBlocks) continue;
    let run: ChildNode[] = [];
    const wrap = () => {
      if (
        run.some(
          (node) =>
            node.nodeType === Node.ELEMENT_NODE ||
            !/^[\t\r\n ]*$/.test(node.textContent ?? ""),
        )
      ) {
        const paragraph = doc.createElement("p");
        run[0].before(paragraph);
        paragraph.append(...run);
      } else {
        for (const node of run) node.remove();
      }
      run = [];
    };
    for (const child of Array.from(parent.childNodes)) {
      if (child instanceof Element && child.matches(blocks)) wrap();
      else run.push(child);
    }
    wrap();
  }
  return documentFragment(doc.body.innerHTML);
}

/** Elements that must never render inside an AI proposal preview. */
const PREVIEW_REMOVED_TAGS = new Set([
  "script",
  "style",
  "iframe",
  "frame",
  "frameset",
  "object",
  "embed",
  "applet",
  "base",
  "link",
  "meta",
  "noscript",
  "template",
  "form",
  "input",
  "button",
  "select",
  "textarea",
  "option",
  "optgroup",
  "audio",
  "video",
  "track",
  "foreignobject",
  "use",
  "animate",
  "animatemotion",
  "animatetransform",
  "set",
]);

const PREVIEW_URL_ATTRS = new Set([
  "href",
  "xlink:href",
  "src",
  "poster",
  "action",
  "formaction",
  "data",
]);

function previewUrlIsSafe(name: string, value: string): boolean {
  // Browsers ignore embedded whitespace/control characters in schemes.
  const normalized = Array.from(value.toLowerCase())
    .filter((character) => {
      const code = character.charCodeAt(0);
      return code > 0x20 && code !== 0x7f;
    })
    .join("");
  if (normalized.startsWith("javascript:") || normalized.startsWith("vbscript:"))
    return false;
  if (normalized.startsWith("data:")) {
    return (
      (name === "src" || name === "xlink:href") && normalized.startsWith("data:image/")
    );
  }
  if (name === "href" || name === "xlink:href") {
    // Only in-page anchors are safe; anything else could navigate the app.
    return normalized.startsWith("#");
  }
  if (name === "src" || name === "poster") {
    // Never let the preview fetch the network. Book-relative paths are inert.
    return (
      !normalized.startsWith("http:") &&
      !normalized.startsWith("https:") &&
      !normalized.startsWith("//")
    );
  }
  return false;
}

/**
 * Scrub AI-generated HTML before it is shown in the proposal preview. The raw
 * result is still inserted into the HTML draft on confirmation; this function
 * only protects the rendered preview, which may contain arbitrary unsupported
 * or malformed markup. Scripts, embedded browsing contexts, event handlers,
 * network fetches and active CSS are removed while ordinary tags and text stay
 * visible so the user can review what will be inserted.
 */
export function sanitizePreviewHtml(
  html: string,
  resolveImage?: (source: string) => string | null,
): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const element of Array.from(doc.body.querySelectorAll("*"))) {
    const tag = element.tagName.toLowerCase();
    if (PREVIEW_REMOVED_TAGS.has(tag)) {
      element.remove();
      continue;
    }
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      if (
        name.startsWith("on") ||
        name === "srcdoc" ||
        name === "style" ||
        name === "srcset" ||
        name === "sizes" ||
        name === "contenteditable" ||
        name === "tabindex" ||
        name === "autofocus"
      ) {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (
        resolveImage &&
        ((tag === "img" && name === "src") ||
          (tag === "image" && (name === "href" || name === "xlink:href")))
      ) {
        // The resolver must return only an app-owned resource or a safe image data URL.
        const resolved = resolveImage(attribute.value);
        if (resolved) attribute.value = resolved;
        else element.removeAttributeNode(attribute);
        continue;
      }
      if (PREVIEW_URL_ATTRS.has(name) && !previewUrlIsSafe(name, attribute.value)) {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (
        ["fill", "stroke", "filter", "clip-path", "mask"].includes(name) &&
        /url\s*\(/i.test(attribute.value)
      ) {
        element.removeAttribute(attribute.name);
      }
    }
  }
  return doc.body.innerHTML;
}

/** Compare structural content without code indentation between block elements. */
export function comparableDocument(html: string): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  // Rich-editor DOMs keep imported CSS inert under data-book-style; compare
  // them as the `style` attribute that documentFragment saves and exports.
  for (const element of document.querySelectorAll("[data-book-style]")) {
    if (!element.hasAttribute("style"))
      element.setAttribute("style", element.getAttribute("data-book-style") ?? "");
    element.removeAttribute("data-book-style");
  }
  for (const element of document.querySelectorAll("[style]")) {
    if (element instanceof HTMLElement)
      element.setAttribute("style", element.style.cssText);
  }
  for (const element of document.querySelectorAll(
    "body, ul, ol, blockquote, table, thead, tbody, tfoot, tr, div, section, hgroup, article, main, header, footer, aside, figure, figcaption, dl, svg, g",
  )) {
    for (const node of Array.from(element.childNodes)) {
      if (node.nodeType === Node.TEXT_NODE && !(node.textContent ?? "").trim())
        node.remove();
    }
  }
  return documentFragment(document.body.innerHTML);
}
