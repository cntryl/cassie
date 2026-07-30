import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { observeEditorLayout } from "@/shared/editor-resize-observer";

describe("editor layout observer", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("should_coalesce_dimension_changes_and_ignore_duplicate_sizes", () => {
    // Arrange
    let callback: ResizeObserverCallback = () => undefined;
    const disconnect = vi.fn();
    const observe = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(next: ResizeObserverCallback) {
          callback = next;
        }
        observe = observe;
        disconnect = disconnect;
      },
    );
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback) => {
      frames.push(frame);
      return frames.length;
    });
    const layout = vi.fn();
    const element = document.createElement("div");
    observeEditorLayout(element, layout);
    const entry = (width: number, height: number) =>
      ({ contentRect: { width, height } }) as ResizeObserverEntry;

    // Act
    callback([entry(400, 300)], {} as ResizeObserver);
    callback([entry(420, 300)], {} as ResizeObserver);
    callback([entry(420, 300)], {} as ResizeObserver);
    frames[0]?.(0);

    // Assert
    expect(observe).toHaveBeenCalledWith(element);
    expect(frames).toHaveLength(1);
    expect(layout).toHaveBeenCalledOnce();
  });

  it("should_disconnect_and_cancel_pending_layout_work", () => {
    // Arrange
    let callback: ResizeObserverCallback = () => undefined;
    const disconnect = vi.fn();
    const cancelAnimationFrame = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(next: ResizeObserverCallback) {
          callback = next;
        }
        observe() {}
        disconnect = disconnect;
      },
    );
    vi.stubGlobal("requestAnimationFrame", () => 42);
    vi.stubGlobal("cancelAnimationFrame", cancelAnimationFrame);
    const observer = observeEditorLayout(document.createElement("div"), vi.fn());
    callback(
      [{ contentRect: { width: 1, height: 1 } } as ResizeObserverEntry],
      {} as ResizeObserver,
    );

    // Act
    observer.disconnect();

    // Assert
    expect(disconnect).toHaveBeenCalledOnce();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(42);
  });
});
