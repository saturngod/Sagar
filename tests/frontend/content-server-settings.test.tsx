import { expect, mock, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";

import type { ContentServerConfig, ContentServerStatus } from "../../src/lib/tauri/contentServer";

let status: ContentServerStatus = { running: false, port: null, authenticationRequired: false, urls: [] };
let failStatus = false;
let failStart = false;
let failStop = false;
let starts: ContentServerConfig[] = [];
let finishStart: (() => void) | null = null;
let deferStart = false;
mock.module("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    if (command === "content_server_status") {
      if (failStatus) throw { code: "server_error", message: "Status unavailable. Retry." };
      return { ...status };
    }
    if (command === "start_content_server") {
      const config = args?.config as ContentServerConfig;
      starts.push(config);
      if (failStart) throw { code: "server_bind_failed", message: "Port is occupied. Choose another port." };
      if (deferStart) await new Promise<void>((resolve) => { finishStart = resolve; });
      status = { running: true, port: config.port || 49210, authenticationRequired: Boolean(config.username), urls: ["http://192.168.1.20:49210"] };
      return { ...status };
    }
    if (command === "stop_content_server") {
      if (failStop) throw { code: "server_error", message: "Stop failed. Check status." };
      status = { running: false, port: null, authenticationRequired: false, urls: [] };
      return { ...status };
    }
    if (command === "list_ai_providers") return { cli: [], custom: [] };
    if (command === "list_system_fonts") return [];
    throw new Error(command);
  },
}));
const { ContentServerSettings } = await import("../../src/features/settings/ContentServerSettings");
const { AiSettingsDialog } = await import("../../src/features/settings/AiSettingsDialog");

function setup() {
  const window = new Window();
  Object.assign(globalThis, { window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Node: window.Node, Element: window.Element, IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  return { window, host, root };
}

test("content server validates inputs, retains errors, prevents duplicate starts, and restores running status", async () => {
  const { window, host, root } = setup();
  const mount = async () => { await act(async () => { root.render(createElement(ContentServerSettings, { enabled: true })); }); };
  failStatus = true;
  await mount();
  expect(host.textContent).toContain("Status unavailable");
  const button = (label: string) => Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent?.trim() === label)!;
  expect(button("Start server").disabled).toBe(true);
  failStatus = false;
  await act(async () => button("Check status").click());
  expect(host.textContent).toContain("Content server is stopped");
  const edit = async (id: string, value: string) => {
    const input = host.querySelector<HTMLInputElement>(`#content-server-${id}`)!;
    // React DOM can initialize before happy-dom in the combined suite.
    // Exercise the handler directly, as in the language-dialog regression tests.
    const propsKey = Object.keys(input).find((key) => key.startsWith("__reactProps"))!;
    const props = (input as unknown as Record<string, unknown>)[propsKey] as { onChange: (event: { target: { value: string } }) => void };
    await act(async () => props.onChange({ target: { value } }));
  };
  const submit = async () => { await act(async () => { host.querySelector("form")!.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true })); }); };
  for (const port of ["-1", "1.5", "65536", ""]) {
    await edit("port",port); await submit();
    expect(host.textContent).toContain("whole-number port");
    expect(document.activeElement).toBe(host.querySelector("#content-server-port"));
    expect(host.querySelector("#content-server-port")!.getAttribute("aria-invalid")).toBe("true");
  }
  expect(starts).toHaveLength(0);
  await edit("port","0"); await edit("username","reader"); await submit();
  expect(host.textContent).toContain("Enter both username and password");
  await edit("password","test-only");
  expect(host.querySelector<HTMLInputElement>("#content-server-password")!.type).toBe("password");
  failStart = true; await submit();
  expect(host.textContent).toContain("Port is occupied");
  expect(host.querySelector<HTMLInputElement>("#content-server-username")!.value).toBe("reader");
  expect(host.querySelector<HTMLInputElement>("#content-server-password")!.value).toBe("test-only");
  failStart = false; deferStart = true;
  await submit();
  expect(host.textContent).toContain("Starting content server");
  expect(button("Starting…").disabled).toBe(true);
  const count = starts.length; await submit(); expect(starts.length).toBe(count);
  await act(async () => { finishStart?.(); }); deferStart = false;
  expect(host.textContent).toContain("Content server is running");
  expect(host.textContent).toContain("http://192.168.1.20:49210");
  expect(host.textContent).toContain("Login required");
  expect(document.activeElement).toBe(button("Stop server"));
  expect(host.querySelector<HTMLInputElement>("#content-server-port")!.value).toBe("49210");
  expect(host.querySelector<HTMLInputElement>("#content-server-port")!.disabled).toBe(true);
  failStop = true; await act(async () => button("Stop server").click());
  expect(host.textContent).toContain("Stop failed");
  failStop = false;
  const stopClick = new window.MouseEvent("click", { bubbles: true, cancelable: true });
  await act(async () => { button("Stop server").dispatchEvent(stopClick); });
  expect(stopClick.defaultPrevented).toBe(true);
  expect(host.textContent).toContain("Content server is stopped");
  await edit("username",""); await edit("password",""); await submit();
  expect(host.textContent).toContain("No login required");
  await act(async () => root.unmount());
  const nextRoot = createRoot(host);
  await act(async () => nextRoot.render(createElement(ContentServerSettings, { enabled: true })));
  expect(host.textContent).toContain("Content server is running");
  expect(host.querySelector<HTMLInputElement>("#content-server-password")!.value).toBe("");
  await act(async () => nextRoot.unmount());
});

test("settings server tab works without providers and participates in keyboard navigation and dismissal", async () => {
  const { window, host, root } = setup();
  const trigger = document.createElement("button"); document.body.append(trigger); trigger.focus();
  let closed = 0;
  await act(async () => root.render(createElement(AiSettingsDialog,{ appearance: { theme:"light", textSize:18,onThemeChange:()=>{},onTextSizeChange:()=>{} },onClose:()=>{closed++;} })));
  const server = host.querySelector<HTMLButtonElement>("#ai-tab-server")!;
  await act(async () => server.click());
  expect(server.getAttribute("aria-selected")).toBe("true");
  expect(host.querySelector<HTMLElement>("#ai-panel-server")!.hidden).toBe(false);
  expect(host.querySelector<HTMLElement>("#ai-panel-appearance")!.hidden).toBe(true);
  expect(server.querySelector("svg")).not.toBeNull();
  await act(async () => server.dispatchEvent(new window.KeyboardEvent("keydown",{key:"ArrowRight",bubbles:true})));
  expect(host.querySelector("#ai-tab-appearance")!.getAttribute("aria-selected")).toBe("true");
  await act(async () => host.querySelector("#ai-tab-appearance")!.dispatchEvent(new window.KeyboardEvent("keydown",{key:"ArrowLeft",bubbles:true})));
  expect(server.getAttribute("aria-selected")).toBe("true");
  await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown",{key:"Escape",bubbles:true})));
  expect(closed).toBe(1);
  await act(async () => root.unmount());
  expect(document.activeElement).toBe(trigger);
});
