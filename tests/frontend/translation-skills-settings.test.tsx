import { expect, mock, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import type { TranslationSkillSummary } from "../../src/types/project";

const builtin: TranslationSkillSummary = {
  id: "sagar-fiction",
  label: "Fiction",
  description: "Novels and stories.",
  kind: "genre",
  target: null,
  version: "1",
  builtin: true,
};
const mine: TranslationSkillSummary = {
  id: "house-style",
  label: "House style",
  description: "Our rules.",
  kind: "language",
  target: "my",
  version: "1",
  builtin: false,
};
const copy: TranslationSkillSummary = {
  ...builtin,
  id: "fiction-copy",
  label: "Fiction (copy)",
  builtin: false,
};
let skills: TranslationSkillSummary[] = [builtin, mine];
const calls: Array<[string, Record<string, unknown> | undefined]> = [];
let failSave = true;
mock.module("@tauri-apps/plugin-dialog", () => ({
  open: async () => "/picked/SKILL.md",
  save: async () => null,
}));
mock.module("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    calls.push([command, args]);
    switch (command) {
      case "list_translation_skills":
        return skills;
      case "get_translation_skill_source":
        return `---\nname: ${args?.skillId}\n---\nRules for ${args?.skillId}`;
      case "import_translation_skill":
        if (!args?.replace)
          throw {
            code: "skill_exists",
            message: "A skill with this name already exists.",
            details: { id: "house-style", label: "House style" },
          };
        return mine;
      case "duplicate_translation_skill":
        skills = [...skills, copy];
        return copy;
      case "save_translation_skill":
        if (failSave) {
          failSave = false;
          throw { code: "invalid_skill", message: "The skill needs a description." };
        }
        return mine;
      case "translation_skill_usage":
        return 2;
      case "delete_translation_skill":
        skills = skills.filter((skill) => skill.id !== args?.skillId);
        return ["Qoder"];
      case "list_skill_installs":
        return [];
    }
    throw new Error(command);
  },
}));
const { TranslationSkillsSettings } = await import(
  "../../src/features/settings/TranslationSkillsSettings"
);

test("Translation skills settings import, duplicate, edit and delete skills", async () => {
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
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const button = (name: string) => {
    const value = Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
      .filter((b) => !b.closest("[hidden]"))
      .find(
        (b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
      );
    if (!value) throw new Error(`Missing button ${name}`);
    return value;
  };
  const has = (name: string) =>
    Array.from(container.querySelectorAll("button"))
      .filter((b) => !b.closest("[hidden]"))
      .some(
        (b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
      );
  const row = (label: string) =>
    Array.from(
      container.querySelectorAll<HTMLButtonElement>(".provider-row__toggle"),
    ).find(
      (toggle) => toggle.querySelector("strong")?.textContent === label,
    ) as HTMLButtonElement;
  try {
    await act(async () =>
      root.render(createElement(TranslationSkillsSettings, { enabled: true })),
    );
    expect(container.querySelectorAll(".provider-list > li").length).toBe(2);
    expect(container.textContent).toContain("Built-in");
    expect(container.textContent).toContain("Language · my");

    // Import asks before replacing a skill with the same name.
    await act(async () => button("Import skill").click());
    expect(container.textContent).toContain(
      "A skill with this name already exists. Replace it?",
    );
    await act(async () => button("Replace").click());
    expect(
      calls.filter(([c]) => c === "import_translation_skill").map(([, a]) => a),
    ).toEqual([
      { path: "/picked/SKILL.md", replace: false },
      { path: "/picked/SKILL.md", replace: true },
    ]);
    expect(container.textContent).toContain("Imported House style");
    expect(row("House style").getAttribute("aria-expanded")).toBe("true");

    // Built-in skills can be viewed and duplicated, not edited or deleted.
    await act(async () => row("Fiction").click());
    expect(container.querySelector(".skills-settings__source")?.textContent).toContain(
      "Rules for sagar-fiction",
    );
    expect(has("Edit")).toBe(false);
    await act(async () => button("Duplicate Fiction").click());
    expect(container.querySelectorAll(".provider-list > li").length).toBe(3);
    expect(container.textContent).toContain("Created Fiction (copy)");

    // Editing keeps the text after a failed save.
    await act(async () => row("House style").click());
    await act(async () => button("Edit").click());
    const editor = () =>
      container.querySelector<HTMLTextAreaElement>(".skills-settings__editor");
    expect(editor()?.value).toBe("---\nname: house-style\n---\nRules for house-style");
    await act(async () => button("Save").click());
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "The skill needs a description.",
    );
    expect(editor()?.value).toContain("Rules for house-style");
    await act(async () => button("Save").click());
    expect(editor()).toBeNull();
    expect(container.textContent).toContain("Saved House style");

    // Deleting says how many books will go back to no skill.
    await act(async () => button("Delete").click());
    expect(container.textContent).toContain(
      "Books that use it will go back to no skill: 2",
    );
    await act(async () => button("Confirm delete").click());
    expect(
      calls.some(
        ([c, a]) => c === "delete_translation_skill" && a?.skillId === "house-style",
      ),
    ).toBe(true);
    expect(container.querySelectorAll(".provider-list > li").length).toBe(2);
    expect(container.textContent).toContain(
      "Deleted House style. A changed copy was kept in: Qoder",
    );
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});
