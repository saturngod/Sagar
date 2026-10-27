import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { bookImageUrl } from "../../src/features/translator/bookImages";
import { sanitizePreviewHtml } from "../../src/features/translator/richText";

test("reading a translation resolves local images and keeps active content inert", () => {
  const window = new Window();
  const previousWindow = globalThis.window;
  const previousParser = globalThis.DOMParser;
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      convertFileSrc: (path: string, protocol: string) => `${protocol}://local/${path}`,
    },
  });
  Object.assign(globalThis, { window, DOMParser: window.DOMParser });
  try {
    const base = "epubres://project/OEBPS/chapter.xhtml";
    const html = [
      '<p>မြန်မာ <img src="images/river.png" alt="River" onerror="bad()"/></p>',
      '<svg xmlns="http://www.w3.org/2000/svg"><image href="images/cover.png"/></svg>',
      '<img src="https://example.com/remote.png" alt="Remote"/>',
      '<img src="file:///private/secret.png" alt="File"/>',
      '<img src="data:image/png;base64,AA" alt="Local"/>',
      '<script>bad()</script><a href="javascript:bad()">link</a>',
      '<p contenteditable="true" tabindex="0" autofocus>Read only</p>',
    ].join("");
    const safe = sanitizePreviewHtml(html, (source) => bookImageUrl(source, base));
    const doc = new window.DOMParser().parseFromString(safe, "text/html");
    expect(doc.querySelector('img[alt="River"]')?.getAttribute("src")).toContain(
      "project/OEBPS/images/river.png",
    );
    expect(doc.querySelector("svg image")?.getAttribute("href")).toContain(
      "project/OEBPS/images/cover.png",
    );
    expect(doc.querySelector('img[alt="Remote"]')?.hasAttribute("src")).toBe(false);
    expect(doc.querySelector('img[alt="File"]')?.hasAttribute("src")).toBe(false);
    expect(doc.querySelector('img[alt="Local"]')?.getAttribute("src")).toBe(
      "data:image/png;base64,AA",
    );
    expect(safe).toContain("မြန်မာ");
    expect(safe).not.toContain("onerror");
    expect(safe).not.toContain("<script");
    expect(safe).not.toContain("javascript:");
    expect(safe).toContain("Read only");
    expect(doc.querySelector("[contenteditable], [tabindex], [autofocus]")).toBeNull();
  } finally {
    Object.assign(globalThis, { window: previousWindow, DOMParser: previousParser });
    window.happyDOM.abort();
  }
});
