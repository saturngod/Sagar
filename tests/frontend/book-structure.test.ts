import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { DOMParser as ProseMirrorParser, DOMSerializer } from "@tiptap/pm/model";
import Image from "@tiptap/extension-image";
import { Table, TableRow } from "@tiptap/extension-table";
import {
  BookStructure,
  createBookStructure,
  BookTableCell,
  BookTableHeader,
} from "../../src/features/translator/bookStructure";
import { BookAttributes } from "../../src/features/translator/bookAttributes";
import { BookAnchor } from "../../src/features/translator/bookAnchor";
import { comparableDocument, documentFragment, normalizeRichDocument } from "../../src/features/translator/richText";
import { runSlashCommand } from "../../src/features/translator/useSlashMenu";
import { bookImageView } from "../../src/features/translator/bookImages";

const window = new Window();
Object.assign(globalThis, {
  window, document: window.document, navigator: window.navigator,
  DOMParser: window.DOMParser, Node: window.Node, Element: window.Element,
  HTMLElement: window.HTMLElement,
  getComputedStyle: window.getComputedStyle.bind(window),
  requestAnimationFrame: window.requestAnimationFrame.bind(window),
  cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
});

const ChapterTable = Table.extend({
  renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
    return ["table", HTMLAttributes, ["tbody", 0]];
  },
});

function createEditor(html: string) {
  return new Editor({
    extensions: [ ...BookStructure, BookAttributes, BookAnchor,
      ChapterTable.configure({ resizable: false }),
      TableRow, BookTableCell, BookTableHeader,
      StarterKit.configure({ trailingNode: false, link: { openOnClick: false, autolink: false } }),
      Image.configure({ allowBase64: true }),
    ],
    content: html,
    parseOptions: { preserveWhitespace: true },
  });
}

test("chapter data-parent and heading groups survive Rich editing and reload", async () => {
  const source = await Bun.file(new URL("../fixtures/editor/chapter-heading-group.xhtml", import.meta.url)).text();
  const normalized = normalizeRichDocument(source);
  expect(normalized).toContain('data-parent="part-1"');
  expect(normalized).toContain('<hgroup class="has-h3">');
  const editor = createEditor(normalized);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(normalized));
    expect(editor.view.dom.querySelector("hgroup h3")?.textContent).toBe("I");
    expect(editor.view.dom.querySelector("abbr")?.textContent).toBe("Mr.");
    let titlePosition = 0;
    editor.state.doc.descendants((node, position) => {
      if (node.isText && node.text === "Mr.") titlePosition = position;
    });
    editor.commands.insertContentAt(titlePosition, "မြန်မာ ");
    const saved = documentFragment(editor.getHTML());
    expect(saved).toContain('data-parent="part-1"');
    expect(saved).toContain('<hgroup class="has-h3">');
    expect(saved).toContain('epub:type="z3998:name-title"');
    expect(saved).toContain("မြန်မာ Mr.");
    editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
  } finally { editor.destroy(); }
});

test("Borderless verse tables keep styles and EPUB semantics through typing and reload", async () => {
  const source = await Bun.file(new URL("../fixtures/editor/borderless-table.xhtml", import.meta.url)).text();
  const normalized = normalizeRichDocument(source);
  const editor = createEditor(normalized);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(normalized));
    expect(editor.view.dom.querySelector("table")?.getAttribute("data-book-borderless")).toBe("true");
    let firstText = 0;
    editor.state.doc.descendants((node, position) => {
      if (node.isText && !firstText) firstText = position;
    });
    editor.commands.insertContentAt(firstText, "မင်္ဂလာပါ ");
    const saved = documentFragment(editor.getHTML());
    expect(saved).toMatch(/style="border: 0(?:px)?; ?"/);
    expect(saved).toContain('class="simplelist"');
    expect(saved).toContain('epub:type="list"');
    expect(saved).toContain("မင်္ဂလာပါ ");
    expect(saved).not.toContain("data-book-borderless");
    editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
    expect(editor.view.dom.querySelector("table[data-book-borderless]")).not.toBeNull();
    // Normal table editing remains available on these imported layout tables.
    editor.commands.setTextSelection(firstText);
    expect(editor.commands.addRowAfter()).toBe(true);
    expect(editor.view.dom.querySelector("table[data-book-borderless]")).not.toBeNull();
  } finally { editor.destroy(); }
});

test("Imported inline styles stay inert in the Rich editor and save verbatim", () => {
  const html = '<div class="body s1" style="white-space:pre-wrap; line-break:strict;"><p style="margin:0; color:red">Text</p></div>';
  const editor = createEditor(html);
  try {
    expect(editor.view.dom.querySelector("[style]")).toBeNull();
    expect(
      editor.view.dom.querySelector("div")?.getAttribute("data-book-style"),
    ).toBe("white-space:pre-wrap; line-break:strict;");
    expect(editor.view.dom.querySelector("p")?.getAttribute("data-book-style")).toBe(
      "margin:0; color:red",
    );
    const saved = documentFragment(editor.getHTML());
    expect(saved).toContain('style="white-space:pre-wrap; line-break:strict;"');
    expect(saved).toContain('style="margin:0; color:red"');
    expect(saved).not.toContain("data-book-style");
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
    editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
  } finally { editor.destroy(); }
});

test("Border hints respect CSS precedence, legacy borders, protected and nested tables", () => {
  for (const [attributes, borderless] of [
    ['style="border: none"', true],
    ['style="border-width: 0px"', true],
    ['style="border-style: hidden"', true],
    ['border="0"', true],
    ['border="0" style="border: 1px solid red"', false],
    ['style="border: 0; border-bottom: 1px solid red"', false],
    ['style="border: 1px solid red; border-width: 0"', true],
    ['', false],
  ] as const) {
    const editor = createEditor(`<table ${attributes}><tbody><tr><td><p>Text</p></td></tr></tbody></table>`);
    try {
      expect(editor.view.dom.querySelector("table")?.hasAttribute("data-book-borderless")).toBe(borderless);
    } finally { editor.destroy(); }
  }
  const html = '<table style="border:0"><caption>Verse</caption><tbody><tr><td><p>မြန်မာ</p><table><tbody><tr><td><p>Grid</p></td></tr></tbody></table></td></tr></tbody></table>';
  const editor = createEditor(html);
  try {
    const tables = editor.view.dom.querySelectorAll("table");
    expect(tables[0].hasAttribute("data-book-borderless")).toBe(true);
    expect(tables[1].hasAttribute("data-book-borderless")).toBe(false);
    expect(editor.view.dom.querySelector("[style]")).toBeNull();
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
  } finally { editor.destroy(); }
});

test("local inline, standalone and protected-table images display without changing saved resource references", () => {
  const html = '<p>မြန်မာ <img src="images/river.png" alt="Inline"/> စာ</p><img src="images/river.png" alt="Standalone"/><table><caption>Table</caption><tbody><tr><td><img src="images/river.png" alt="Table image"/></td></tr></tbody></table>';
  const base = "epubres://p/OEBPS/chapter.xhtml";
  const editor = new Editor({
    extensions: [...createBookStructure(base), BookAttributes, BookAnchor,
      ChapterTable.configure({ resizable: false }), TableRow, BookTableCell, BookTableHeader,
      StarterKit.configure({ trailingNode: false, link: { openOnClick: false, autolink: false } }),
      Image.extend({ addNodeView: () => bookImageView(base) }).configure({ allowBase64: true })],
    content: html, parseOptions: { preserveWhitespace: true },
  });
  try {
    const images = [...editor.view.dom.querySelectorAll("img")];
    expect(images.length).toBe(3);
    expect(images.map((img) => img.alt)).toEqual(["Inline", "Standalone", "Table image"]);
    for (const image of images) expect(image.getAttribute("src")).toContain("p/OEBPS/images/river.png");
    const saved = editor.getHTML();
    expect(saved.match(/src="images\/river.png"/g)?.length).toBe(3);
    expect(saved).not.toContain("epubres://");
    expect(saved).not.toContain("epubres.localhost");
    expect(comparableDocument(saved)).toBe(comparableDocument(html));
  } finally { editor.destroy(); }
});

test("Rich normalization drops empty decoration but preserves anchors, semantics and whitespace", () => {
  const input = '<h3 class="subtitle"><em></em></h3><div class="empty"><span></span></div><p>မြန်မာ <em>စာ</em>။</p>';
  const cleaned = normalizeRichDocument(input);
  expect(cleaned).toBe('<p>မြန်မာ <em>စာ</em>။</p>');
  expect(normalizeRichDocument(cleaned)).toBe(cleaned);
  expect(normalizeRichDocument('<header/><h1>Title</h1><footer/>')).toBe('<h1>Title</h1>');
  for (const retained of [
    '<h3 id="note"></h3>', '<h3 epub:type="subtitle"></h3>',
    '<h3 role="heading"></h3>', '<h3 aria-label="Subtitle"></h3>',
    '<h3 style="margin:1em"></h3>', '<h3> </h3>', '<p></p>',
    '<a id="anchor"></a>', '<pre>  \n </pre>', '<p><img src="book.png"/></p>',
  ]) {
    expect(normalizeRichDocument(retained)).toContain(retained);
  }
});

test("Rich normalization drops bare empty anchors so the chapter round-trips", () => {
  const input =
    '<p class="calibre_13">“ပြီးတော့, <a></a>မစ္စတာ ကာ့ရှ်၊ <span class="italic">သင်၏</span> ခန့်မှန်းချက်။”</p>';
  const cleaned = normalizeRichDocument(input);
  expect(cleaned).toBe(
    '<p class="calibre_13">“ပြီးတော့, မစ္စတာ ကာ့ရှ်၊ <span class="italic">သင်၏</span> ခန့်မှန်းချက်။”</p>',
  );
  expect(normalizeRichDocument(cleaned)).toBe(cleaned);
  const editor = createEditor(cleaned);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(cleaned));
  } finally { editor.destroy(); }
  for (const retained of ['<a id="p12"></a>', '<a class="x"></a>', '<a href="#n1"></a>', '<a> </a>']) {
    expect(normalizeRichDocument(`<p>Text ${retained}more</p>`)).toContain(retained);
  }
});

test("Rich normalizes mixed book containers, bare lists and plain table cells", () => {
  const examples = [
    '<div>Before <em>မြန်မာ</em><p>Middle</p>After <strong>စာ</strong></div>',
    '<div><span>မြန်မာ</span> <strong>စာ</strong><p>Paragraph</p></div>',
    '<ul><li>မြန်မာ <small>စာ</small></li><li>Two</li></ul>',
    '<blockquote>Quoted <cite>မြန်မာစာ</cite></blockquote>',
    '<table><tbody><tr><td>မြန်မာ <abbr title="Meaning">စာ</abbr></td></tr></tbody></table>',
    '<p aria-describedby="note">Text <small>small</small> <kbd>key</kbd><samp>output</samp><var>x</var><dfn>term</dfn><mark>mark</mark></p>',
  ];
  for (const html of examples) {
    const cleaned = normalizeRichDocument(html);
    expect(normalizeRichDocument(cleaned)).toBe(cleaned);
    const before = new DOMParser().parseFromString(html, "text/html");
    const after = new DOMParser().parseFromString(cleaned, "text/html");
    expect(after.body.textContent).toBe(before.body.textContent);
    const editor = createEditor(cleaned);
    try {
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(cleaned));
      const saved = documentFragment(editor.getHTML());
      editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(cleaned));
    } finally { editor.destroy(); }
  }
});

test("Protected tables retain assets and links without mounting them in the editor", () => {
  const html = '<p>Before</p><table><caption>Caption</caption><thead><tr><th id="heading">မြန်မာ</th></tr></thead><tbody><tr><td><a href="https://example.com">Link</a><img src="image.png" alt="Asset"/></td></tr></tbody><tfoot><tr><td>Foot</td></tr></tfoot></table><p>After</p>';
  const editor = createEditor(html);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
    expect(editor.view.dom.querySelector("img,a[href],script")).toBeNull();
    expect(editor.view.dom.textContent).toContain("မြန်မာ");
    editor.commands.insertContentAt(1, "မင်္ဂလာပါ ");
    expect(editor.getHTML()).toContain("မင်္ဂလာပါ Before");
    const saved = documentFragment(editor.getHTML());
    editor.commands.setContent(saved);
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
    expect(saved).toContain('src="image.png"');
    expect(saved).toContain('id="heading"');
  } finally { editor.destroy(); }
});

test("Translated titlepage links remain editable without added paragraphs", async () => {
  // Rust serializes empty XHTML containers with explicit closing tags.
  const html = (await Bun.file(new URL("../fixtures/ai/link-target.xhtml", import.meta.url)).text())
    .replace("<header/>", "<header></header>")
    .replace("<footer/>", "<footer></footer>");
  const editor = createEditor(html);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
    expect(editor.view.dom.querySelector("a[href]")).toBeNull();
    const saved = documentFragment(editor.getHTML());
    expect(saved).not.toContain("target=");
    expect(saved).toContain('href="https://example.com/book"');
    expect(saved).toContain('class="link"');
    let position = 0;
    editor.state.doc.descendants((node, pos) => {
      if (node.type.name === "book_block_link") position = pos + 1;
    });
    expect(position).toBeGreaterThan(0);
    editor.commands.insertContentAt(position, "မြန်မာ ");
    expect(editor.getHTML()).toContain("မြန်မာ Book website");
    const edited = documentFragment(editor.getHTML());
    editor.commands.setContent(edited, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(edited));
  } finally {
    editor.destroy();
  }
});

test("Empty subtitle formatting survives Rich parsing, typing and reload", async () => {
  const html = await Bun.file(new URL("../fixtures/editor/empty-subtitle.xhtml", import.meta.url)).text();
  const editor = createEditor(html);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
    expect(editor.getHTML()).toContain('<h3 class="subtitle"><em></em></h3>');
    let textPosition = 0;
    editor.state.doc.descendants((node, position) => {
      if (node.isText && textPosition === 0) textPosition = position;
    });
    editor.commands.insertContentAt(textPosition, "မင်္ဂလာပါ ");
    const saved = documentFragment(editor.getHTML());
    expect(saved).toContain("မင်္ဂလာပါ မြန်မာပုံပြင်");
    expect(saved).toContain('<h3 class="subtitle"><em></em></h3>');
    editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
    for (const tag of ["em", "strong", "u", "s", "code"]) {
      const fragment = `<p>မြန်မာ<${tag} id="note" class="empty" title="Anchor"></${tag}>စာ <${tag}>text</${tag}></p>`;
      editor.commands.setContent(fragment);
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(fragment));
    }
  } finally {
    editor.destroy();
  }
});

for (const [label, html] of [
  ["imprint", '<section class="imprint" id="imprint" epub:type="imprint" xmlns:epub="http://www.idpf.org/2007/ops"><header><h2 epub:type="title">ထုတ်ဝေမှု</h2><img src="logo.png" srcset="logo-2x.png 2x, logo.png 1x" sizes="100vw" alt="Logo"/></header><p>မြန်မာ <a href="uncopyright.xhtml">စာ</a></p></section>'],
  ["inline semantics", '<p><span class="small"><b>မြန်မာ</b> <i>စာ</i></span><sup id="note">1</sup><sub>2</sub><code>text</code><ruby>字<rt>ji</rt><rp>(</rp></ruby></p>'],
  ["inline image", '<p>Before <img src="a.png" alt="A" srcset="a.png 1x, b.png 2x"/> after</p>'],
  ["blockquote and code", '<blockquote id="quote"><p>မြန်မာ <code class="sample">x &lt; y</code></p></blockquote><pre id="sample"><code>one\n  two &amp; three</code></pre>'],
  ["definition list", '<dl><dt>Term</dt><dd><p>Meaning</p></dd></dl><pre>one\n  two</pre>'],
  ["SVG", '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><g><title>Cover</title><image xlink:href="cover.png"/><path d="M0 0L10 10"/></g></svg><p>မြန်မာ</p>'],
  ["responsive image", '<figure><picture><source srcset="a.webp" type="image/webp"/><img src="a.png" alt="A"/></picture><figcaption>Caption</figcaption></figure>'],
  ["table cell align", '<table border="1" cellpadding="4"><tbody><tr><td align="center" valign="top"><p>Hi</p></td></tr></tbody></table>'],
  ["myanmar table", '<table><tbody><tr><th><p>ခေါင်းစဉ်</p></th></tr><tr><td><p>မြန်မာ စာသား</p></td></tr></tbody></table>'],
]) {
  test(`Rich preserves ${label} through editing and reloading`, () => {
    const editor = createEditor(html);
    try {
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
      const saved = documentFragment(editor.getHTML());
      editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
      const textPosition: number[] = [];
      editor.state.doc.descendants((node, position) => {
        if (node.isText && textPosition.length === 0) textPosition.push(position);
      });
      if (textPosition.length) {
        editor.commands.insertContentAt(textPosition[0], "မင်္ဂလာပါ ");
        expect(editor.getHTML()).toContain("မင်္ဂလာပါ ");
      }
    } finally { editor.destroy(); }
  });
}

test("SVG node view never mounts imported executable content", () => {
  const editor = createEditor('<svg><script>alert(1)</script><path d="M0 0"/></svg>');
  try {
    expect(editor.view.dom.querySelector("svg, script")).toBeNull();
    expect(editor.getHTML()).not.toContain("<script");
    expect(editor.view.dom.textContent).toContain("SVG artwork");
  } finally { editor.destroy(); }
});

test("Cell align attributes round-trip verbatim without injected styles", () => {
  const editor = createEditor(
    '<table border="1" cellpadding="4"><tbody><tr><td align="center" valign="top"><p>Hi</p></td></tr></tbody></table>',
  );
  try {
    // Regression: the stock cell parsed align="center" into a style
    // ("text-align: center"), so AI-translated chapters with aligned cells
    // failed the HTML → Rich preservation check and trapped users in HTML.
    expect(editor.getHTML()).not.toContain("text-align");
    expect(comparableDocument(editor.getHTML())).toBe(
      comparableDocument(
        '<table border="1" cellpadding="4"><tbody><tr><td align="center" valign="top"><p>Hi</p></td></tr></tbody></table>',
      ),
    );
    editor.commands.setTextSelection(5);
    expect(editor.chain().focus().addRowAfter().run()).toBe(true);
  } finally {
    editor.destroy();
  }
});

test("Table sections remain intact as protected content while surrounding text is editable", () => {
  for (const html of [
    '<table><thead><tr><th><p>H</p></th></tr></thead><tbody><tr><td><p>C</p></td></tr></tbody></table>',
    '<table><caption>Cap</caption><tbody><tr><td><p>C</p></td></tr></tbody></table>',
  ]) {
    const editor = createEditor(html);
    try {
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(html));
      expect(editor.view.dom.textContent).toContain("Table preserved");
      expect(editor.view.dom.querySelector(".book-protected-content table")).not.toBeNull();
      expect(editor.isActive("table")).toBe(false);
    } finally {
      editor.destroy();
    }
  }
});

for (const [command, query, nodeName] of [
  ["blockquote", "/blockquote", "blockquote"],
  ["code", "/code", "codeBlock"],
] as const) {
  test(`Slash ${command} removes the query, preserves text, and undoes as one edit`, () => {
    const editor = createEditor(`<p>${query} မြန်မာ</p>`);
    try {
      editor.commands.setTextSelection(query.length + 1);
      expect(runSlashCommand(editor, command, { from: 1, to: query.length + 1 }, () => {})).toBe(true);
      expect(editor.isActive(nodeName)).toBe(true);
      expect(editor.state.doc.textContent).toBe(" မြန်မာ");
      const saved = documentFragment(editor.getHTML());
      editor.commands.undo();
      expect(editor.state.doc.textContent).toBe(`${query} မြန်မာ`);
      editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
    } finally { editor.destroy(); }
  });
}

test("Toolbar inline code toggles selected Myanmar text and preserves escaping", () => {
  const editor = createEditor("<p>မြန်မာ &lt;code&gt;</p>");
  try {
    editor.commands.selectAll();
    expect(editor.commands.toggleCode()).toBe(true);
    expect(editor.getHTML()).toContain("<code>မြန်မာ &lt;code&gt;</code>");
    expect(editor.commands.toggleCode()).toBe(true);
    expect(editor.getHTML()).not.toContain("<code>");
    editor.commands.toggleBlockquote();
    expect(editor.isActive("blockquote")).toBe(true);
    editor.commands.setTextSelection(3);
    editor.commands.toggleBlockquote();
    expect(editor.isActive("blockquote")).toBe(false);
    editor.commands.toggleCodeBlock();
    expect(editor.isActive("codeBlock")).toBe(true);
    editor.commands.toggleCodeBlock();
    expect(editor.isActive("paragraph")).toBe(true);
  } finally { editor.destroy(); }
});

test("Slash inline code formats subsequent input and ignores a disabled editor", () => {
  const editor = createEditor("<p>/inline code</p>");
  try {
    editor.commands.setTextSelection(13);
    expect(runSlashCommand(editor, "inlineCode", { from: 1, to: 13 }, () => {})).toBe(true);
    editor.view.dispatch(editor.state.tr.insertText("မြန်မာ"));
    expect(editor.getHTML()).toContain("<code>မြန်မာ</code>");
    editor.commands.setContent("<p>/code</p>");
    editor.setEditable(false);
    expect(runSlashCommand(editor, "code", { from: 1, to: 6 }, () => {})).toBe(false);
    expect(editor.state.doc.textContent).toBe("/code");
  } finally { editor.destroy(); }
});


test("aligned div retains Myanmar text, escaped angle brackets and alignment through Rich reload", async () => {
  const source = await Bun.file(new URL("../fixtures/editor/aligned-div.xhtml", import.meta.url)).text();
  for (const html of [source, `<div align="center"><p>မြန်မာ</p></div>`]) {
    const normalized = normalizeRichDocument(html);
    expect(normalized).toContain('align="');
    const editor = createEditor(normalized);
    try {
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(normalized));
      expect(editor.view.dom.querySelector("div[align]")).not.toBeNull();
      editor.commands.insertContentAt(1, "မင်္ဂလာပါ ");
      const saved = documentFragment(editor.getHTML());
      expect(saved).toContain('align="');
      expect(saved).toContain("မင်္ဂလာပါ");
      if (html === source) expect(saved).toContain("&lt; အခန်းအမှားတင်ပြရန် &gt;");
      editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
      expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
    } finally { editor.destroy(); }
  }
});

test("selected heading groups and time survive Rich typing and reload", async () => {
  const source = await Bun.file(new URL("../fixtures/editor/selection-time.xhtml", import.meta.url)).text();
  const body = new DOMParser().parseFromString(source, "text/html").body.innerHTML;
  const normalized = normalizeRichDocument(body);
  expect(normalized).toContain('<time datetime="1878">1878</time>');
  const editor = createEditor(normalized);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(normalized));
    let position = 0;
    editor.state.doc.descendants((node, pos) => {
      if (node.isText && node.text === "1878") position = pos;
    });
    editor.commands.insertContentAt(position, "မြန်မာ ");
    const saved = documentFragment(editor.getHTML());
    expect(saved).toContain('<time datetime="1878">မြန်မာ 1878</time>');
    expect(saved).toContain('<hgroup id="title">');
    editor.commands.setContent(saved);
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
  } finally { editor.destroy(); }
});

test("highlighted code metadata survives Rich conversion, editing and reload", async () => {
  const source = (await Bun.file(new URL("../fixtures/editor/highlighted-code.xhtml", import.meta.url)).text()).trim();
  const normalized = normalizeRichDocument(source);
  expect(normalized).toContain('data-highlighted="yes"');
  const editor = createEditor(normalized);
  try {
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(normalized));
    editor.commands.insertContentAt(1, "မြန်မာ ");
    const saved = documentFragment(editor.getHTML());
    expect(saved.match(/data-highlighted="yes"/g)).toHaveLength(2);
    expect(saved).toContain('<span class="hljs-keyword">const</span>');
    expect(saved).toContain('\n  next();');
    editor.commands.setContent(saved, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(saved));
  } finally {
    editor.destroy();
  }
});

test("figure div with an image and loose caption survives HTML → Rich, typing and reload", async () => {
  const source = await Bun.file(new URL("../fixtures/editor/figure-caption.xhtml", import.meta.url)).text();
  const normalized = normalizeRichDocument(source);
  // Mirrors TargetPane's HTML → Rich preservation check.
  const editor = createEditor("<p></p>");
  try {
    const wrapper = document.createElement("div");
    wrapper.innerHTML = normalized;
    const parsed = ProseMirrorParser.fromSchema(editor.schema).parse(wrapper, { preserveWhitespace: true });
    const roundTrip = document.createElement("div");
    roundTrip.append(DOMSerializer.fromSchema(editor.schema).serializeFragment(parsed.content));
    expect(comparableDocument(roundTrip.innerHTML)).toBe(comparableDocument(normalized));
    editor.commands.setContent(parsed.toJSON(), { emitUpdate: false });
    const saved = documentFragment(editor.getHTML());
    expect(saved).not.toContain("<p><span");
    expect(saved).toContain('<span class="caption" id="caption1">');
    expect(saved).toContain('<div class="figcenter"><img alt="Only image" src="images/only.jpg"/></div>');
    // Typing in the caption keeps the figure and its image intact.
    let captionPos = -1;
    editor.state.doc.descendants((node, pos) => {
      if (node.isText && node.text?.startsWith("မြန်မာအမျိုးသမီး")) captionPos = pos;
    });
    expect(captionPos).toBeGreaterThan(0);
    editor.commands.insertContentAt(captionPos, "ပုံ ");
    const edited = documentFragment(editor.getHTML());
    expect(edited).toContain('id="caption1">ပုံ မြန်မာအမျိုးသမီး');
    expect(edited).toContain('src="images/lady.jpg"');
    editor.commands.setContent(edited, { parseOptions: { preserveWhitespace: true } });
    expect(comparableDocument(editor.getHTML())).toBe(comparableDocument(edited));
  } finally { editor.destroy(); }
});
