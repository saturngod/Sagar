import { expect, mock, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import type {
  BookSettings,
  ProjectSummary,
  TranslationSkillSummary,
} from "../../src/types/project";

const project: ProjectSummary = {
  id: "book",
  title: "A long book title for the settings header",
  author: "Author",
  sourceLanguage: "en",
  targetLanguage: "my",
  reviewedBlocks: 0,
  totalBlocks: 2,
  unsupportedBlocks: 0,
  createdAt: "",
  updatedAt: "",
};
const original: BookSettings = {
  title: project.title,
  author: "Author",
  publisher: "",
  description: "",
  sourceLanguage: "en",
  targetLanguage: "my",
  instructions: "Keep names consistent. မြန်မာ",
  customCover: "data:image/png;base64,c2FtcGxl",
};
const skills: TranslationSkillSummary[] = [
  {
    id: "sagar-xianxia",
    label: "Xianxia 仙侠",
    description: "Cultivation novels.",
    kind: "genre",
    target: null,
    version: "1",
    builtin: true,
  },
  {
    id: "sagar-target-my",
    label: "Myanmar",
    description: "Natural Myanmar.",
    kind: "language",
    target: "my",
    version: "1",
    builtin: true,
  },
];
let settingsOverride: Partial<BookSettings> = {};
const saved: BookSettings[] = [];
let failSave = true;
mock.module("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    if (command === "get_book_settings") return { ...original, ...settingsOverride };
    if (command === "list_translation_skills") return skills;
    if (command === "get_translation_skill")
      return {
        ...skills.find((s) => s.id === args?.skillId),
        body: "Keep realms fixed.",
      };
    if (command === "get_project") return project;
    if (command === "save_book_settings") {
      saved.push({ ...(args?.settings as BookSettings) });
      if (failSave) {
        failSave = false;
        throw { code: "save_failed", message: "Please retry saving." };
      }
      return;
    }
    throw new Error(command);
  },
}));
const { BookSettingsDialog } = await import(
  "../../src/features/workspace/BookSettingsDialog"
);

test("Book Settings retains edits across tabs and failed saves, then saves both tabs together", async () => {
  const window = new Window();
  Object.assign(globalThis, {
    window,
    document: window.document,
    navigator: window.navigator,
    HTMLElement: window.HTMLElement,
    Node: window.Node,
    Element: window.Element,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const trigger = document.createElement("button");
  document.body.append(trigger);
  trigger.focus();
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  let closes = 0;
  let updated: ProjectSummary | null = null;
  const button = (name: string) => {
    const value = Array.from(
      container.querySelectorAll<HTMLButtonElement>("button"),
    ).find(
      (b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
    );
    if (!value) throw new Error(`Missing button ${name}`);
    return value;
  };
  try {
    await act(async () =>
      root.render(
        createElement(BookSettingsDialog, {
          project,
          onClose: () => {
            closes++;
          },
          onSaved: (value) => {
            updated = value;
          },
        }),
      ),
    );
    expect(container.querySelector("h2")?.textContent?.trim()).toBe("Book settings");
    expect(container.querySelector(".book-settings__book-title")?.textContent).toBe(
      project.title,
    );
    await act(async () => button("Use original cover").click());
    await act(async () => button("AI instructions").click());
    expect(container.querySelector("textarea")?.value).toBe(original.instructions);
    expect(button("AI instructions").getAttribute("aria-selected")).toBe("true");
    expect(saved.length).toBe(0);
    await act(async () => button("Save").click());
    expect(
      container.querySelector('.settings-footer [role="alert"]')?.textContent,
    ).toContain("Please retry saving.");
    expect(closes).toBe(0);
    expect(saved[0].customCover).toBeNull();
    expect(saved[0].instructions).toBe(original.instructions);
    await act(async () => button("Metadata").click());
    expect(container.querySelector(".book-cover-settings__preview")).toBeNull();
    await act(async () => button("Save").click());
    expect(saved[1]).toEqual(saved[0]);
    expect(updated).toEqual(project);
    expect(closes).toBe(1);
  } finally {
    await act(async () => root.unmount());
    expect(document.activeElement).toBe(trigger);
    container.remove();
    trigger.remove();
  }
});

test("Book Settings selects translation skills, previews rules and flags a language mismatch", async () => {
  const window = new Window();
  Object.assign(globalThis, {
    window,
    document: window.document,
    navigator: window.navigator,
    HTMLElement: window.HTMLElement,
    Node: window.Node,
    Element: window.Element,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  settingsOverride = { genreSkillId: null, languageSkillId: "sagar-target-my" };
  saved.length = 0;
  failSave = false;
  const container = document.createElement("div");
  document.body.append(container);
  let root = createRoot(container);
  const button = (name: string) => {
    const value = Array.from(
      container.querySelectorAll<HTMLButtonElement>("button"),
    ).find(
      (b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
    );
    if (!value) throw new Error(`Missing button ${name}`);
    return value;
  };
  const choose = async (element: HTMLSelectElement | HTMLInputElement, value: string) => {
    await act(async () => {
      // Bypass React's value tracker so the change is seen as user input.
      let proto: object | null = Object.getPrototypeOf(element);
      let setter: ((v: string) => void) | undefined;
      while (proto && !setter) {
        setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
        proto = Object.getPrototypeOf(proto);
      }
      setter?.call(element, value);
      element.dispatchEvent(
        new window.Event("input", { bubbles: true }) as unknown as Event,
      );
      element.dispatchEvent(
        new window.Event("change", { bubbles: true }) as unknown as Event,
      );
    });
  };
  try {
    await act(async () =>
      root.render(
        createElement(BookSettingsDialog, {
          project,
          onClose: () => {},
          onSaved: () => {},
        }),
      ),
    );
    await act(async () => button("AI instructions").click());
    const selects = () => Array.from(container.querySelectorAll("select"));
    expect(selects().length).toBe(2);
    const [genre, language] = selects();
    expect(Array.from(genre.options).map((o) => o.textContent)).toEqual([
      "None",
      "Xianxia 仙侠",
    ]);
    expect(language.value).toBe("sagar-target-my");
    expect(container.textContent).toContain("Natural Myanmar.");
    await choose(genre, "sagar-xianxia");
    expect(container.textContent).toContain("Cultivation novels.");
    const rules = container.querySelector<HTMLDetailsElement>(".skill-picker__rules");
    expect(rules).not.toBeNull();
    await act(async () => {
      if (rules) {
        rules.open = true;
        rules.dispatchEvent(new window.Event("toggle") as unknown as Event);
      }
    });
    expect(container.querySelector(".skill-picker__body")?.textContent).toBe(
      "Keep realms fixed.",
    );
    expect(selects()[1].getAttribute("aria-invalid")).toBeNull();
    await act(async () => root.unmount());
    // The translation language changed elsewhere (for example the Language dialog).
    settingsOverride = {
      genreSkillId: "sagar-xianxia",
      languageSkillId: "sagar-target-my",
      targetLanguage: "th",
    };
    root = createRoot(container);
    await act(async () =>
      root.render(
        createElement(BookSettingsDialog, {
          project,
          onClose: () => {},
          onSaved: () => {},
        }),
      ),
    );
    await act(async () => button("AI instructions").click());
    expect(selects()[1].getAttribute("aria-invalid")).toBe("true");
    expect(container.textContent).toContain(
      "This language skill doesn't match the translation language.",
    );
    await choose(selects()[1], "");
    expect(selects()[1].getAttribute("aria-invalid")).toBeNull();
    await act(async () => button("Save").click());
    expect(saved[0].genreSkillId).toBe("sagar-xianxia");
    expect(saved[0].languageSkillId).toBeNull();
    expect(saved[0].targetLanguage).toBe("th");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    settingsOverride = {};
  }
});
