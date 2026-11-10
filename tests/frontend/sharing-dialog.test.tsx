import { expect, mock, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";

const calls: Record<string, unknown>[] = [];
const completed: string[] = [];
const book = {
  id: 7,
  title: "Sample",
  visibility: "public",
  editions: [{ id: 12, xhtml_spine_count: 3 }],
};
let suggestions: Record<string, unknown>[] = [];
const remote = {
  id: 42,
  spine_position: 0,
  label: "Chapter 1",
  public_translation_count: 1,
};
let signedIn = false;
let missing = false;
let visibility = "public";
let available = true;

mock.module("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args: { operation?: Record<string, unknown> } = {}) => {
    if (command === "get_book_settings") return { description: "Original description" };
    if (command === "sharing_availability") {
      calls.push({ kind: "availability", args });
      return { available, host: "127.0.0.1:8000" };
    }
    if (command === "sharing_sign_in") {
      calls.push({ kind: "sign_in" });
      signedIn = true;
      return { email: "reader@example.test" };
    }
    const operation = args.operation ?? {};
    calls.push(operation);
    switch (operation.kind) {
      case "account":
        return signedIn ? { email: "reader@example.test" } : null;
      case "match":
        return {
          exact: missing
            ? []
            : [{ book_id: 7, edition_id: 12, book_visibility: "public" }],
          suggestions: missing ? suggestions : [],
        };
      case "search":
        return { data: suggestions, last_page: 1, current_page: 1 };
      case "link":
        return null;
      case "cover":
        return operation.book === 7
          ? { media_type: "image/png", base64: "iVBORw0K" }
          : null;
      case "create":
        missing = false;
        visibility = operation.public ? "public" : "private";
        return { book_id: 7, edition_id: 12 };
      case "book":
        return { ...book, visibility };
      case "chapters":
        return { data: [remote], last_page: 1, current_page: 1 };
      case "languages":
        return { data: [{ code: "ja", name: "Japanese" }] };
      case "versions":
        return {
          data: [
            {
              id: 2,
              contributor_name: "Reader",
              target_language: operation.target_language,
            },
          ],
          last_page: 1,
          current_page: 1,
        };
      case "mine":
        return null;
      case "local_preview":
        return operation.local_chapter === "draft"
          ? { content: "<p><strong>မြန်မာ</strong></p>", revision: 4 }
          : null;
      case "save":
        return {};
      default:
        throw new Error(String(operation.kind));
    }
  },
}));
const { SharingDialog } = await import("../../src/features/workspace/SharingDialog");

async function exercise(
  mode: "submit" | "browse",
  verify: (
    container: HTMLElement,
    click: (name: string) => Promise<void>,
  ) => Promise<void>,
) {
  const window = new Window({ url: "http://localhost" });
  Object.assign(globalThis, {
    window,
    document: window.document,
    localStorage: window.localStorage,
    DOMParser: window.DOMParser,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  window.confirm = () => true;
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const click = async (name: string) => {
    const button = [...container.querySelectorAll("button")].find(
      (item) => item.textContent?.trim() === name,
    );
    expect(button).toBeDefined();
    expect(button?.disabled).toBe(false);
    await act(async () => button?.click());
  };
  try {
    await act(async () =>
      root.render(
        createElement(SharingDialog, {
          mode,
          initialMatch: null,
          activeChapterId: "draft",
          project: {
            id: "book",
            title: "Sample",
            author: "Author",
            sourceLanguage: "en",
            targetLanguage: "my-Mymr-MM",
            totalBlocks: 2,
            reviewedBlocks: 0,
            unsupportedBlocks: 0,
            createdAt: "",
            updatedAt: "",
          },
          chapters: ["draft", "empty"].map((id, spineIndex) => ({
            id,
            spineIndex,
            title: `Chapter ${spineIndex + 1}`,
            resourceHref: `${id}.xhtml`,
            status: "draft",
            totalBlocks: 1,
            reviewedBlocks: 0,
            unsupportedBlocks: 0,
          })),
          onClose: () => {},
          onComplete: (message: string) => completed.push(message),
          onFlush: async () => true,
          onApply: async () => true,
        }),
      ),
    );
    await verify(container, click);
  } finally {
    await act(async () => root.unmount());
  }
}

test("Submit lists similar books with chapter counts and links a chosen chapter before upload", async () => {
  calls.length = 0;
  completed.length = 0;
  signedIn = true;
  missing = true;
  visibility = "public";
  suggestions = [
    {
      id: 7,
      title: "Sample",
      author: "Author",
      visibility: "public",
      chapter_count: 3,
      cover_url: "/api/v1/books/7/cover",
    },
    {
      id: 8,
      title: "Sample notes",
      author: null,
      visibility: "private",
      cover_url: null,
    },
  ];
  try {
    await exercise("submit", async (container, click) => {
      expect(container.textContent).toContain("these look similar");
      expect(container.textContent).toContain("Author · 3 chapters");
      const covers = [...container.querySelectorAll(".sharing-list .sharing-cover")];
      expect(covers.map((cover) => cover.getAttribute("src"))).toEqual([
        "data:image/png;base64,iVBORw0K",
        null,
      ]);
      expect(calls.filter((call) => call.kind === "cover")).toEqual([
        { kind: "cover", book: 7 },
      ]);
      expect(container.querySelector("textarea")).toBeNull();
      const choose = container.querySelector<HTMLButtonElement>(
        'button[aria-label="Choose Sample"]',
      );
      await act(async () => choose?.click());
      expect(container.textContent).toContain(
        "This SagarNet copy has 3 chapters; your book has 2 chapters.",
      );
      const select = [...container.querySelectorAll("label")]
        .find((item) => item.textContent?.startsWith("Chapter on SagarNet"))
        ?.querySelector("select") as HTMLSelectElement;
      expect(select.value).toBe("42");
      await click("Link chapter");
      expect(calls.some((call) => call.kind === "link" && call.book === 7)).toBe(true);
      await click("Upload chapter");
      expect(calls.find((call) => call.kind === "save")).toMatchObject({ chapter: 42 });
    });
  } finally {
    suggestions = [];
  }
});

test("Submit signs in, prefills a missing book, and sends only the current chapter in the project language", async () => {
  calls.length = 0;
  completed.length = 0;
  signedIn = false;
  missing = true;
  await exercise("submit", async (container, click) => {
    expect(calls.slice(0, 2).map((call) => call.kind)).toEqual([
      "availability",
      "account",
    ]);
    expect(calls.some((call) => call.kind === "sign_in")).toBe(false);
    expect(container.querySelector("input[type=url]")).toBeNull();
    await click("Sign in with browser");
    expect(calls.slice(2, 5).map((call) => call.kind)).toEqual([
      "account",
      "sign_in",
      "match",
    ]);
    expect(container.querySelector("textarea")?.value).toBe("Original description");
    await click("Add book to SagarNet");
    expect(calls.find((call) => call.kind === "create")).toMatchObject({
      title: "Sample",
      author: "Author",
      public: false,
    });
    expect(container.querySelector(".sharing-preview strong")?.textContent).toBe("မြန်မာ");
    const radios = [
      ...container.querySelectorAll<HTMLInputElement>("input[name=sagarnet-visibility]"),
    ];
    expect(radios.map((radio) => [radio.checked, radio.disabled])).toEqual([
      [true, false],
      [false, true],
    ]);
    await click("Upload chapter");
    expect(
      calls
        .filter((call) => call.kind === "local_preview")
        .map((call) => call.local_chapter),
    ).toEqual(["draft"]);
    expect(calls.filter((call) => call.kind === "save")).toEqual([
      {
        kind: "save",
        project: "book",
        local_chapter: "draft",
        chapter: 42,
        public: false,
        existing: undefined,
        local_revision: 4,
        target_language: "my-mymr-mm",
      },
    ]);
    expect(completed).toEqual(["Chapter 1 is uploaded. Only you can see it."]);
  });
});

test("Get translations matches the open book and changes the exact language filter", async () => {
  calls.length = 0;
  completed.length = 0;
  signedIn = false;
  missing = false;
  visibility = "public";
  await exercise("browse", async (container) => {
    expect(calls.some((call) => call.kind === "sign_in")).toBe(false);
    expect(calls.find((call) => call.kind === "versions")).toMatchObject({
      chapter: 42,
      target_language: "my-mymr-mm",
    });
    const select = [...container.querySelectorAll("label")]
      .find((item) => item.textContent?.startsWith("Language"))
      ?.querySelector("select") as HTMLSelectElement;
    await act(async () => {
      select.value = "ja";
      select.dispatchEvent(new window.Event("change", { bubbles: true }));
    });
    expect(calls.at(-1)).toMatchObject({
      kind: "versions",
      chapter: 42,
      target_language: "ja",
    });
    expect(container.textContent).toContain("ReaderJapanese");
  });
});

test("Submit validates a saved account and matches without opening OAuth again", async () => {
  calls.length = 0;
  completed.length = 0;
  signedIn = true;
  missing = false;
  visibility = "public";
  await exercise("submit", async (container) => {
    expect(calls.slice(0, 3).map((call) => call.kind)).toEqual([
      "availability",
      "account",
      "match",
    ]);
    expect(calls.some((call) => call.kind === "sign_in")).toBe(false);
    expect(container.textContent).toContain("Signed in as reader@example.test");
    expect(container.textContent).toContain("Upload chapter");
  });
});

test("Sharing explains when this build has no SagarNet configuration", async () => {
  calls.length = 0;
  completed.length = 0;
  available = false;
  try {
    await exercise("browse", async (container) => {
      expect(calls.map((call) => call.kind)).toEqual(["availability"]);
      expect(container.textContent).toContain("not set up in this version of Sagar");
      expect(container.querySelector("input")).toBeNull();
    });
  } finally {
    available = true;
  }
});
