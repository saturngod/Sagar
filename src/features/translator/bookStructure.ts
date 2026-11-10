import Image from "@tiptap/extension-image";
import { TableCell, TableHeader } from "@tiptap/extension-table";
import { Node } from "@tiptap/react";
import { bookImageUrl, bookImageView } from "./bookImages";
import { documentFragment } from "./richText";
import { tableAppearanceAttributes } from "./tableAppearance";

/** Structural XHTML containers remain in the document while their text is edited. */
const containers = [
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
  "dl",
  "dd",
];
const blockSelector =
  "p,h1,h2,h3,h4,h5,h6,ul,ol,li,blockquote,hr,table,div,section,hgroup,article,main,header,footer,aside,figure,figcaption,pre,dl,dt,dd,svg,picture,img";
/**
 * Gutenberg-style figures put an image beside a loose caption, e.g.
 * `<div><img/><span class="caption">…</span></div>`. Block-only containers
 * would wrap that caption in a new paragraph, so keep such containers as
 * inline text with the images inline instead. An image-only container stays
 * a block container.
 */
function mixesImagesWithInline(element: Element): boolean {
  const children = Array.from(element.children);
  const isImage = (child: Element) => child.matches("img[src]");
  return (
    children.some(isImage) &&
    children.every((child) => isImage(child) || !child.matches(blockSelector)) &&
    (children.some((child) => !isImage(child)) ||
      Array.from(element.childNodes).some(
        (node) =>
          node.nodeType === globalThis.Node.TEXT_NODE &&
          (node.textContent ?? "").trim() !== "",
      ))
  );
}
function hasBlockChildren(element: Element): boolean {
  return (
    !mixesImagesWithInline(element) &&
    Array.from(element.children).some((child) => child.matches(blockSelector))
  );
}
const inline = [
  "time",
  "span",
  "b",
  "i",
  "sup",
  "sub",
  "ruby",
  "rt",
  "rp",
  "small",
  "abbr",
  "cite",
  "dfn",
  "kbd",
  "samp",
  "var",
  "mark",
];

export function createBookStructure(imageBase = "") {
  return [
    Node.create({
      name: "book_preserved_table",
      group: "block",
      atom: true,
      addAttributes: () => ({ original: { default: "", rendered: false } }),
      parseHTML: () => [
        {
          tag: "table",
          priority: 130,
          getAttrs: (element) =>
            element.querySelector("thead,tfoot,caption")
              ? { original: documentFragment(element.outerHTML) }
              : false,
        },
      ],
      renderHTML: ({ node }) => {
        const holder = document.createElement("div");
        holder.innerHTML = node.attrs.original as string;
        return holder.firstElementChild ?? document.createElement("table");
      },
      addNodeView:
        () =>
        ({ node }) => {
          const dom = document.createElement("div");
          dom.className = "book-protected-content";
          dom.contentEditable = "false";
          const label = document.createElement("p");
          label.textContent = "Table preserved — edit in HTML view.";
          dom.append(label);
          const source = new DOMParser().parseFromString(
            node.attrs.original as string,
            "text/html",
          );
          const copy = (parent: HTMLElement, child: globalThis.Node) => {
            if (child.nodeType === globalThis.Node.TEXT_NODE) {
              parent.append(document.createTextNode(child.textContent ?? ""));
            } else if (child instanceof Element) {
              const tag = child.tagName.toLowerCase();
              if (tag === "img") {
                const src = bookImageUrl(child.getAttribute("src") ?? "", imageBase);
                if (src) {
                  const img = document.createElement("img");
                  img.src = src;
                  img.alt = child.getAttribute("alt") ?? "Book image";
                  parent.append(img);
                }
                return;
              }
              // Build an inert text/table preview, without links, images, CSS or
              // imported attributes that could fetch or execute in the WebView.
              const safe = document.createElement(
                [
                  "table",
                  "thead",
                  "tbody",
                  "tfoot",
                  "tr",
                  "td",
                  "th",
                  "caption",
                  "p",
                  "em",
                  "strong",
                  "br",
                ].includes(tag)
                  ? tag
                  : "span",
              );
              for (const name of ["colspan", "rowspan"]) {
                const value = child.getAttribute(name);
                if (value && /^\d+$/.test(value) && Number(value) <= 100)
                  safe.setAttribute(name, value);
              }
              if (tag === "table") {
                // Derive only a border hint; never mount imported CSS here.
                const appearance = tableAppearanceAttributes({
                  style: child.getAttribute("style"),
                  border: child.getAttribute("border"),
                });
                for (const [name, value] of Object.entries(appearance))
                  safe.setAttribute(name, value);
              }
              for (const nested of Array.from(child.childNodes)) copy(safe, nested);
              parent.append(safe);
            }
          };
          for (const child of Array.from(source.body.childNodes)) copy(dom, child);
          return { dom };
        },
    }),
    // Marks cannot represent empty elements. Keep empty source formatting as
    // inline nodes instead, including IDs and other book attributes.
    ...["em", "strong", "u", "s", "code"].map((tag) =>
      Node.create({
        name: `book_empty_${tag}`,
        group: "inline",
        inline: true,
        atom: true,
        selectable: false,
        parseHTML: () => [
          {
            tag,
            priority: 120,
            getAttrs: (element) => (element.childNodes.length === 0 ? {} : false),
          },
        ],
        renderHTML: ({ HTMLAttributes }) => [tag, HTMLAttributes],
      }),
    ),
    // EPUB containers can mix paragraphs and standalone links. Keep those
    // links as blocks so parsing does not silently add a paragraph wrapper.
    Node.create({
      name: "book_block_link",
      group: "block",
      content: "inline*",
      defining: true,
      addAttributes: () => ({
        href: {
          default: null,
          parseHTML: (element) => element.getAttribute("href"),
        },
      }),
      parseHTML: () => [
        {
          tag: "a[href]",
          priority: 120,
          getAttrs: (element) =>
            element.parentElement?.matches(containers.join(",")) &&
            Array.from(element.parentElement.children).some((child) =>
              child.matches(blockSelector),
            )
              ? {}
              : false,
        },
      ],
      renderHTML: ({ HTMLAttributes }) => ["a", HTMLAttributes, 0],
      // Preserve the URL in serialized XHTML, but never navigate from the editor.
      addNodeView: () => () => {
        const dom = document.createElement("a");
        return { dom, contentDOM: dom };
      },
    }),
    Image.extend({
      name: "book_inline_image",
      addNodeView: () => bookImageView(imageBase),
      parseHTML: () => [
        {
          tag: "img[src]",
          priority: 120,
          getAttrs: (element) => {
            const parent = element.parentElement;
            return parent?.closest(
              "p,h1,h2,h3,h4,h5,h6,span,a,pre,dt,sup,sub,ruby,rt,rp",
            ) ||
              (parent?.matches(containers.join(",")) && mixesImagesWithInline(parent))
              ? {}
              : false;
          },
        },
      ],
    }).configure({ inline: true, allowBase64: true }),
    ...["svg", "picture"].map((tag) =>
      Node.create({
        name: `book_${tag}`,
        group: "block",
        atom: true,
        addAttributes: () => ({
          original: { default: "", rendered: false },
        }),
        parseHTML: () => [
          {
            tag,
            priority: 110,
            getAttrs: (element) => ({ original: documentFragment(element.outerHTML) }),
          },
        ],
        renderHTML: ({ node }) => {
          const holder = document.createElement("div");
          holder.innerHTML = node.attrs.original as string;
          return holder.firstElementChild ?? document.createElement("div");
        },
        // Never execute or fetch embedded artwork in the privileged editor DOM.
        // Serialization retains its markup; the Rust validator owns export safety.
        addNodeView: () => () => {
          const dom = document.createElement("div");
          dom.className = "book-protected-content";
          dom.contentEditable = "false";
          dom.textContent =
            tag === "svg"
              ? "SVG artwork — preserved. Edit in HTML view."
              : "Responsive image — preserved. Edit in HTML view.";
          return { dom };
        },
      }),
    ),
    ...containers.map((tag) =>
      Node.create({
        name: `book_${tag}`,
        group: "block",
        content: "block*",
        defining: true,
        parseHTML: () => [
          {
            tag,
            priority: 110,
            getAttrs: (element) => (hasBlockChildren(element) ? {} : false),
          },
        ],
        renderHTML: ({ HTMLAttributes }) => [tag, HTMLAttributes, 0],
      }),
    ),
    ...containers.map((tag) =>
      Node.create({
        name: `book_text_${tag}`,
        group: "block",
        content: "inline*",
        defining: true,
        parseHTML: () => [
          {
            tag,
            priority: 111,
            getAttrs: (element) => (hasBlockChildren(element) ? false : {}),
          },
        ],
        renderHTML: ({ HTMLAttributes }) => [tag, HTMLAttributes, 0],
      }),
    ),
    ...["pre", "dt"].map((tag) =>
      Node.create({
        name: `book_${tag}`,
        group: "block",
        content: "inline*",
        whitespace: tag === "pre" ? "pre" : "normal",
        defining: true,
        parseHTML: () => [
          {
            tag,
            priority: 110,
            getAttrs: (element) =>
              tag === "pre" &&
              element.childNodes.length === 1 &&
              element.firstElementChild?.tagName === "CODE" &&
              // The stock code block discards attributes on its inner code.
              !element.firstElementChild.hasAttribute("data-highlighted")
                ? false
                : {},
          },
        ],
        renderHTML: ({ HTMLAttributes }) => [tag, HTMLAttributes, 0],
      }),
    ),
    ...inline.map((tag) =>
      Node.create({
        name: `book_${tag}`,
        group: "inline",
        inline: true,
        content: "inline*",
        addAttributes: () => (tag === "time" ? { datetime: { default: null } } : {}),
        parseHTML: () => [{ tag, priority: 110 }],
        renderHTML: ({ HTMLAttributes }) => [tag, HTMLAttributes, 0],
      }),
    ),
  ];

  /**
   * Table cells without the stock align → style normalization.
   *
   * Tiptap's TableCell/TableHeader parse legacy `align="center"` (and
   * `style="text-align: …"`) into an `align` attribute that always renders
   * `style="text-align: …"`. A backend-valid `<td align="center">` round-trips
   * as `<td align="center" style="text-align: center;">`, fails the
   * comparable-document check, and traps AI-translated chapters in HTML view.
   * The original `align`/`style` attributes are preserved verbatim by the
   * global BookAttributes instead; there is no cell-alignment UI to keep.
   *
   * Tables with thead/tfoot/caption use the protected table node above:
   * prosemirror-tables would rewrite their sections on ordinary transactions.
   * They remain intact in Rich view while other chapter text stays editable.
   */
}

export const BookStructure = createBookStructure();

export const BookTableCell = TableCell.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      align: {
        default: null,
        parseHTML: () => null,
        renderHTML: () => ({}),
      },
    };
  },
});

export const BookTableHeader = TableHeader.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      align: {
        default: null,
        parseHTML: () => null,
        renderHTML: () => ({}),
      },
    };
  },
});

export const bookStructureNames = BookStructure.map((extension) => extension.name);
