import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement, useState } from "react";
import { createRoot } from "react-dom/client";
import { PaneVisibilityMenu } from "../../src/features/workspace/PaneVisibilityMenu";

test("pane menu toggles checked views, retains one pane, and supports keyboard dismissal", async () => {
  const window = new Window();
  const previous = {
    window: globalThis.window,
    document: globalThis.document,
    navigator: globalThis.navigator,
    Node: globalThis.Node,
  };
  Object.assign(globalThis, {
    window,
    document: window.document,
    navigator: window.navigator,
    Node: window.Node,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  function Harness() {
    const [visible, setVisible] = useState({ original: true, translation: true });
    return (
      <PaneVisibilityMenu
        visible={visible}
        onToggle={(pane) =>
          setVisible((current) => ({ ...current, [pane]: !current[pane] }))
        }
      />
    );
  }
  try {
    await act(async () => root.render(createElement(Harness)));
    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-haspopup="menu"]',
    )!;
    await act(async () => trigger.click());
    const items = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="menuitemcheckbox"]'),
    );
    expect(items.map((item) => item.textContent)).toEqual([
      "Show Original",
      "Show Translation",
    ]);
    expect(items.map((item) => item.getAttribute("aria-checked"))).toEqual([
      "true",
      "true",
    ]);
    expect(document.activeElement).toBe(items[0]);
    await act(async () => items[0].click());
    expect(items[0].getAttribute("aria-checked")).toBe("false");
    expect(items[1].getAttribute("aria-disabled")).toBe("true");
    await act(async () => items[1].click());
    expect(items[1].getAttribute("aria-checked")).toBe("true");
    await act(async () =>
      items[0].dispatchEvent(
        new window.KeyboardEvent("keydown", {
          key: "ArrowDown",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(document.activeElement).toBe(items[1]);
    await act(async () =>
      items[1].dispatchEvent(
        new window.KeyboardEvent("keydown", {
          key: "Escape",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(container.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    await act(async () =>
      trigger.dispatchEvent(
        new window.KeyboardEvent("keydown", {
          key: "ArrowUp",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(document.activeElement?.textContent).toBe("Show Translation");
    await act(async () =>
      document.body.dispatchEvent(
        new window.PointerEvent("pointerdown", { bubbles: true }),
      ),
    );
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    window.happyDOM.abort();
    Object.assign(globalThis, previous);
  }
});
