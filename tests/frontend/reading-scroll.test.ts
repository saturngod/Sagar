import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { useSyncedScroll } from "../../src/features/workspace/useSyncedScroll";

test("returning from reading preserves the focused pane before linked scrolling resumes", async () => {
  const window = new Window();
  const previous = {
    window: globalThis.window,
    document: globalThis.document,
    navigator: globalThis.navigator,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
    IS_REACT_ACT_ENVIRONMENT: (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean })
      .IS_REACT_ACT_ENVIRONMENT,
  };
  Object.assign(globalThis, {
    window,
    document: window.document,
    navigator: window.navigator,
    requestAnimationFrame: window.requestAnimationFrame.bind(window),
    cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const source = document.createElement("div");
  const target = document.createElement("div");
  Object.defineProperties(source, {
    clientHeight: { value: 100 },
    scrollHeight: { value: 1100 },
  });
  Object.defineProperties(target, {
    clientHeight: { value: 100 },
    scrollHeight: { value: 2100 },
  });
  const sourceRef = { current: source };
  const targetRef = { current: target };
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);

  function Harness({ active, linked }: { active: boolean; linked: boolean }) {
    useSyncedScroll(sourceRef, targetRef, "chapter", active, linked);
    return null;
  }

  try {
    await act(async () =>
      root.render(createElement(Harness, { active: true, linked: true })),
    );
    source.scrollTop = 400;
    target.scrollTop = 800;
    await act(async () =>
      root.render(createElement(Harness, { active: false, linked: true })),
    );
    target.scrollTop = 1400;
    await act(async () =>
      root.render(createElement(Harness, { active: true, linked: true })),
    );
    expect(target.scrollTop).toBe(1400);

    target.dispatchEvent(new window.Event("scroll"));
    expect(source.scrollTop).toBe(700);

    await act(async () =>
      root.render(createElement(Harness, { active: false, linked: false })),
    );
    source.scrollTop = 200;
    target.scrollTop = 1400;
    await act(async () =>
      root.render(createElement(Harness, { active: true, linked: true })),
    );
    expect(target.scrollTop).toBe(400);
  } finally {
    await act(async () => root.unmount());
    container.remove();
    window.happyDOM.abort();
    Object.assign(globalThis, previous);
  }
});
