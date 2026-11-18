import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { findTextMatches, foldCase } from "../../src/lib/search/textSearch";

function render(html: string) {
  const window = new Window();
  window.document.body.innerHTML = html;
  return { window, root: window.document.body as unknown as Node };
}

function texts(root: Node, query: string) {
  return findTextMatches(root, query).map((range) => range.toString());
}

test("matches are case-insensitive and reported in reading order", () => {
  const { window, root } = render("<h1>River Road</h1><p>The river ran to the RIVER.</p>");
  expect(texts(root, "river")).toEqual(["River", "river", "RIVER"]);
  expect(texts(root, "")).toEqual([]);
  expect(texts(root, "bridge")).toEqual([]);
  window.happyDOM.abort();
});

test("a match may cross inline markup but never a block or line break", () => {
  const { window, root } = render(
    "<p>hel<em>lo wor</em>ld</p><p>end</p><p>start</p><p>one<br/>two</p>",
  );
  const matches = findTextMatches(root, "hello world");
  expect(matches).toHaveLength(1);
  expect(matches[0].startContainer).not.toBe(matches[0].endContainer);
  expect(matches[0].toString()).toBe("hello world");
  expect(texts(root, "endstart")).toEqual([]);
  expect(texts(root, "onetwo")).toEqual([]);
  window.happyDOM.abort();
});

test("Myanmar text with combining marks is found without altering offsets", () => {
  const { window, root } = render("<p>မြန်မာ စာအုပ်</p><p>စာအုပ် ကောင်း</p>");
  const matches = findTextMatches(root, "စာအုပ်");
  expect(matches.map((range) => range.toString())).toEqual(["စာအုပ်", "စာအုပ်"]);
  expect(matches[0].startOffset).toBe("မြန်မာ ".length);
  window.happyDOM.abort();
});

test("highlighted code is searched as source text, across token spans", () => {
  const { window, root } = render(
    '<pre><code><span class="hljs-tag">&lt;<span class="hljs-name">p</span> <span class="hljs-attr">class</span>=<span class="hljs-string">"note"</span>&gt;</span>Text\n&lt;/p&gt;</code></pre>',
  );
  expect(texts(root, '<p class="note">')).toEqual(['<p class="note">']);
  expect(texts(root, "</P>")).toEqual(["</p>"]);
  window.happyDOM.abort();
});

test("hidden and non-rendered text is skipped", () => {
  const { window, root } = render(
    "<p>seen</p><div hidden><p>seen</p></div><textarea>seen</textarea><style>seen{}</style>",
  );
  expect(texts(root, "seen")).toEqual(["seen"]);
  window.happyDOM.abort();
});

test("case folding keeps string length for offset mapping", () => {
  expect(foldCase("ABC")).toBe("abc");
  expect(foldCase("İstanbul")).toHaveLength("İstanbul".length);
});
