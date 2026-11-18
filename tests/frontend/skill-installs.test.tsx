import { expect, mock, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import type { SkillInstall, SkillInstallStatus } from "../../src/types/project";

const state: Record<string, SkillInstallStatus> = {
  codex: "notInstalled",
  opencode: "installed",
  claude_code: "updateAvailable",
  agy: "conflict",
  qoder: "modified",
};
const detected = new Set(["codex", "opencode", "claude_code", "qoder"]);
const labels: Record<string, string> = {
  codex: "Codex",
  opencode: "OpenCode",
  claude_code: "Claude Code",
  agy: "Antigravity (agy)",
  qoder: "Qoder",
};
const calls: string[] = [];
mock.module("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    const id = String(args?.providerId ?? "");
    if (command === "list_skill_installs")
      return Object.keys(state).map(
        (providerId): SkillInstall => ({
          providerId,
          label: labels[providerId],
          folder: `~/.${providerId}/skills`,
          detected: detected.has(providerId),
          status: state[providerId],
        }),
      );
    if (command === "install_skill") {
      calls.push(`install ${id} ${args?.force}`);
      state[id] = "installed";
      return "installed";
    }
    if (command === "uninstall_skill") {
      calls.push(`uninstall ${id} ${args?.force}`);
      state[id] = "notInstalled";
      return "notInstalled";
    }
    throw new Error(command);
  },
}));
const { SkillInstalls } = await import("../../src/features/settings/SkillInstalls");

test("skill installs show each CLI's status, install everywhere and confirm changed copies", async () => {
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
    const value = Array.from(
      container.querySelectorAll<HTMLButtonElement>("button"),
    ).find(
      (b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name,
    );
    if (!value) throw new Error(`Missing button ${name}`);
    return value;
  };
  const statusOf = (label: string) =>
    Array.from(container.querySelectorAll(".skill-installs__row"))
      .find((row) => row.querySelector("strong")?.textContent === label)
      ?.querySelector(".provider-status")?.textContent;
  try {
    await act(async () =>
      root.render(
        createElement(SkillInstalls, { skillId: "sagar-wuxia", revision: "1" }),
      ),
    );
    expect(statusOf("Codex")).toBe("Not installed");
    expect(statusOf("Claude Code")).toBe("Update available");
    expect(statusOf("Antigravity (agy)")).toBe("Not Sagar's folder");
    expect(statusOf("Qoder")).toBe("Changed outside Sagar");
    expect(container.textContent).toContain("~/.codex/skills");
    expect(container.textContent).toContain("wasn't installed by Sagar");
    expect(container.textContent).toContain("may appear twice in OpenCode");

    // Install everywhere only touches found CLIs that are missing or outdated.
    await act(async () => button("Install in all found CLIs").click());
    expect(calls).toEqual(["install codex false", "install claude_code false"]);
    expect(statusOf("Codex")).toBe("Installed");

    // A copy changed outside Sagar needs confirmation before it is removed.
    await act(async () => button("Uninstall · Qoder").click());
    expect(calls.length).toBe(2);
    expect(container.textContent).toContain("Changes made outside Sagar will be lost.");
    const confirm = Array.from(
      container.querySelectorAll<HTMLButtonElement>(".skill-installs__note button"),
    ).find((b) => b.textContent?.trim() === "Uninstall");
    await act(async () => confirm?.click());
    expect(calls[2]).toBe("uninstall qoder true");
    expect(statusOf("Qoder")).toBe("Not installed");

    // An unchanged install is removed straight away.
    await act(async () => button("Uninstall · OpenCode").click());
    expect(calls[3]).toBe("uninstall opencode false");
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});
