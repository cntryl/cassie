import { afterEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import { ResizableSplit } from "@/components/query/resizable-split";

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

// jsdom has no real layout engine, so getBoundingClientRect() always returns
// zeros — stub it with a fixed rect so percent-from-pointer math is testable.
function stubRect(
  el: HTMLElement,
  rect: { top: number; left: number; width: number; height: number },
) {
  el.getBoundingClientRect = () =>
    ({
      top: rect.top,
      left: rect.left,
      width: rect.width,
      height: rect.height,
      right: rect.left + rect.width,
      bottom: rect.top + rect.height,
      x: rect.left,
      y: rect.top,
      toJSON() {
        return {};
      },
    }) as DOMRect;
}

// jsdom doesn't implement the Pointer Capture API; no-op stubs are enough
// since these components only call them, they never rely on the capture
// actually taking effect within a test.
function stubPointerCapture(el: HTMLElement) {
  el.setPointerCapture = () => {};
  el.releasePointerCapture = () => {};
}

async function mountVerticalSplit() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      {
        path: "/",
        handler: () => (
          <ResizableSplit
            orientation="vertical"
            initialSize={50}
            min={20}
            max={80}
            first={<div>first pane</div>}
            second={<div>second pane</div>}
          />
        ),
      },
    ],
  });

  await flushUi();
  return root;
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
});

describe("ResizableSplit pointer drag", () => {
  it("should_describe_a_top_bottom_split_as_a_horizontal_separator", async () => {
    // Arrange
    const root = await mountVerticalSplit();

    // Act
    const handle = root.querySelector('[role="separator"]');

    // Assert
    expect(handle?.getAttribute("aria-orientation")).toBe("horizontal");
  });

  it("resizes the primary pane live during drag and commits the final value on release", async () => {
    const root = await mountVerticalSplit();

    const container = root.querySelector(
      '[data-testid="query-resizable-split-vertical"]',
    ) as HTMLElement;
    const handle = container.querySelector('[role="separator"]') as HTMLElement;
    const pane = container.querySelector(".cassie-resizable-split-pane") as HTMLElement;

    stubRect(container, { top: 0, left: 0, width: 200, height: 400 });
    stubPointerCapture(handle);

    expect(pane.style.blockSize).toBe("50%");

    handle.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, clientX: 100, clientY: 200, pointerId: 1 }),
    );
    await flushUi();
    expect(pane.style.blockSize).toBe("50%");

    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientX: 100, clientY: 300, pointerId: 1 }),
    );
    await flushUi();
    expect(pane.style.blockSize).toBe("75%");
    expect(handle.getAttribute("aria-valuenow")).toBe("75");

    handle.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, clientX: 100, clientY: 300, pointerId: 1 }),
    );
    await flushUi();
    expect(pane.style.blockSize).toBe("75%");
    expect(handle.getAttribute("aria-valuenow")).toBe("75");
  });

  it("clamps the resized value to the configured min/max", async () => {
    const root = await mountVerticalSplit();

    const container = root.querySelector(
      '[data-testid="query-resizable-split-vertical"]',
    ) as HTMLElement;
    const handle = container.querySelector('[role="separator"]') as HTMLElement;
    const pane = container.querySelector(".cassie-resizable-split-pane") as HTMLElement;

    stubRect(container, { top: 0, left: 0, width: 200, height: 400 });
    stubPointerCapture(handle);

    handle.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, clientX: 100, clientY: 200, pointerId: 1 }),
    );
    await flushUi();

    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientX: 100, clientY: 400, pointerId: 1 }),
    );
    await flushUi();
    expect(pane.style.blockSize).toBe("80%");

    handle.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, clientX: 100, clientY: 400, pointerId: 1 }),
    );
    await flushUi();
    expect(pane.style.blockSize).toBe("80%");
  });

  it("should_finish_the_drag_given_a_cancelled_pointer_gesture", async () => {
    // Arrange
    const root = await mountVerticalSplit();
    const container = root.querySelector(
      '[data-testid="query-resizable-split-vertical"]',
    ) as HTMLElement;
    const handle = container.querySelector('[role="separator"]') as HTMLElement;
    const pane = container.querySelector(".cassie-resizable-split-pane") as HTMLElement;
    stubRect(container, { top: 0, left: 0, width: 200, height: 400 });
    stubPointerCapture(handle);

    // Act
    handle.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, clientY: 200, pointerId: 1 }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientY: 300, pointerId: 1 }),
    );
    await flushUi();
    handle.dispatchEvent(
      new PointerEvent("pointercancel", { bubbles: true, clientY: 300, pointerId: 1 }),
    );
    await flushUi();

    // Assert
    expect(container.getAttribute("data-dragging")).toBe(null);
    expect(pane.style.blockSize).toBe("75%");

    // Act: a stray move after cancellation must not keep resizing.
    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientY: 100, pointerId: 1 }),
    );
    await flushUi();

    // Assert
    expect(pane.style.blockSize).toBe("75%");
  });
});
