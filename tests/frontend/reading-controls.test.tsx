import { expect, test } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import { ReadingControls } from "../../src/features/workspace/ReadingControls";

const window = new Window();
Object.assign(globalThis, {
  window,
  document: window.document,
  navigator: window.navigator,
  Node: window.Node,
  Element: window.Element,
  HTMLElement: window.HTMLElement,
  requestAnimationFrame: window.requestAnimationFrame.bind(window),
  cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
  IS_REACT_ACT_ENVIRONMENT: true,
});

function Harness({
  previous = true,
  next = true,
  navigationBusy = false,
  onPrevious = () => {},
  onNext = () => {},
  onClose = () => {},
}: {
  previous?: boolean;
  next?: boolean;
  navigationBusy?: boolean;
  onPrevious?: () => void;
  onNext?: () => void;
  onClose?: () => void;
}) {
  const workspaceRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  return (
    <div ref={workspaceRef}>
      <div className="workspace__body">
        <div ref={scrollRef} className="reading-scroller">
          <article data-reading-content tabIndex={-1}>
            Reading text
          </article>
          <a href="#note">Note</a>
        </div>
      </div>
      <ReadingControls
        workspaceRef={workspaceRef}
        scrollRef={scrollRef}
        canGoPrevious={previous}
        canGoNext={next}
        navigationBusy={navigationBusy}
        onPrevious={onPrevious}
        onNext={onNext}
        onClose={onClose}
      />
    </div>
  );
}

test("reading controls toggle on click and hide on scroll without responding to mouse movement", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(createElement(Harness)));
    const controls = container.querySelector(".workspace__reading-controls")!;
    const scroller = container.querySelector(".reading-scroller")!;
    const text = container.querySelector("article")!;
    const link = container.querySelector("a")!;
    expect(controls.getAttribute("aria-hidden")).toBe("false");

    await act(async () =>
      text.dispatchEvent(new window.MouseEvent("click", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    expect(controls.hasAttribute("inert")).toBe(true);
    await act(async () =>
      scroller.dispatchEvent(new window.MouseEvent("mousemove", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    await act(async () =>
      link.dispatchEvent(
        new window.MouseEvent("click", { bubbles: true, cancelable: true }),
      ),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    await act(async () =>
      text.dispatchEvent(new window.MouseEvent("click", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("false");
    await act(async () => scroller.dispatchEvent(new window.Event("scroll")));
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    await act(async () =>
      text.dispatchEvent(new window.MouseEvent("click", { bubbles: true })),
    );
    await act(async () =>
      scroller.dispatchEvent(new window.WheelEvent("wheel", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");

    await act(async () => {
      window.dispatchEvent(
        new window.KeyboardEvent("keydown", { key: "Tab", cancelable: true }),
      );
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(controls.getAttribute("aria-hidden")).toBe("false");
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "Close reading view (Esc)",
    );
    await act(async () =>
      scroller.dispatchEvent(new window.WheelEvent("wheel", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    expect(document.activeElement).toBe(text);
    await act(async () => text.click());
    const closeButton = controls.querySelector("button")!;
    await act(async () =>
      closeButton.dispatchEvent(new window.WheelEvent("wheel", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
    await act(async () => text.click());
    await act(async () =>
      closeButton.dispatchEvent(new window.Event("touchmove", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});

test("chapter controls respect boundaries and Escape closes the reading view", async () => {
  let previous = 0;
  let next = 0;
  let closed = 0;
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () =>
      root.render(
        createElement(Harness, {
          previous: false,
          onPrevious: () => previous++,
          onNext: () => next++,
          onClose: () => closed++,
        }),
      ),
    );
    const previousButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Previous chapter"]',
    )!;
    const nextButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Next chapter"]',
    )!;
    expect(previousButton.disabled).toBe(true);
    expect(nextButton.disabled).toBe(false);
    await act(async () => {
      previousButton.click();
      nextButton.click();
      window.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(previous).toBe(0);
    expect(next).toBe(1);
    expect(closed).toBe(1);
    expect(
      container
        .querySelector(".workspace__reading-controls")
        ?.getAttribute("aria-hidden"),
    ).toBe("false");
    await act(async () =>
      root.render(
        createElement(Harness, {
          next: false,
          onPrevious: () => previous++,
          onNext: () => next++,
          onClose: () => closed++,
        }),
      ),
    );
    expect(previousButton.disabled).toBe(false);
    expect(nextButton.disabled).toBe(true);
    await act(async () => {
      previousButton.click();
      nextButton.click();
    });
    expect(previous).toBe(1);
    expect(next).toBe(1);
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});

test("chapter scroll resets do not hide controls, but a reader scroll still does", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(createElement(Harness)));
    const controls = container.querySelector(".workspace__reading-controls")!;
    const scroller = container.querySelector(".reading-scroller")!;
    const nextButton = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Next chapter"]',
    )!;
    await act(async () => {
      nextButton.click();
      root.render(createElement(Harness, { navigationBusy: true }));
    });
    await act(async () => scroller.dispatchEvent(new window.Event("scroll")));
    expect(controls.getAttribute("aria-hidden")).toBe("false");
    await act(async () =>
      scroller.dispatchEvent(new window.WheelEvent("wheel", { bubbles: true })),
    );
    expect(controls.getAttribute("aria-hidden")).toBe("true");
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});
