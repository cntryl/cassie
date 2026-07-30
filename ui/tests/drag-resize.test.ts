import { describe, expect, it } from "vite-plus/test";

import { createDragResize } from "@/shared/drag-resize";

describe("drag resize behavior", () => {
  it("should_ignore_pointer_events_for_stale_pointer_ids", () => {
    // Arrange
    const applied: number[] = [];
    const committed: number[] = [];
    const resize = createDragResize({
      min: 0,
      max: 120,
      initialValue: 10,
      smallStep: 2,
      largeStep: 8,
      decreaseKeys: ["ArrowLeft"],
      increaseKeys: ["ArrowRight"],
      computeNextValue: (event, start) => start.value + (event.clientX - start.clientX),
      applyValue: (value) => {
        applied.push(value);
      },
      onCommit: (value) => {
        committed.push(value);
      },
    });
    const handle = document.createElement("button");
    (handle as HTMLElement & { setPointerCapture: (id: number) => void }).setPointerCapture =
      () => {};
    (
      handle as HTMLElement & { releasePointerCapture: (id: number) => void }
    ).releasePointerCapture = () => {};
    handle.onpointerdown = resize.onPointerDown;
    handle.onpointermove = resize.onPointerMove;
    handle.onpointerup = resize.onPointerUp;

    // Act
    handle.dispatchEvent(
      new PointerEvent("pointerdown", {
        bubbles: true,
        pointerId: 1,
        button: 0,
        clientX: 10,
      }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointermove", {
        bubbles: true,
        pointerId: 1,
        clientX: 24,
      }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointerup", {
        bubbles: true,
        pointerId: 2,
        clientX: 24,
      }),
    );

    // Assert
    expect(committed).toEqual([]);
    expect(applied).toEqual([24]);
    expect(resize.value()).toBe(24);

    // Act: finalize the active pointer and start a new drag gesture.
    handle.dispatchEvent(
      new PointerEvent("pointerup", {
        bubbles: true,
        pointerId: 1,
        clientX: 24,
      }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointerdown", {
        bubbles: true,
        pointerId: 3,
        button: 0,
        clientX: 30,
      }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointermove", {
        bubbles: true,
        pointerId: 3,
        clientX: 36,
      }),
    );
    handle.dispatchEvent(
      new PointerEvent("pointerup", {
        bubbles: true,
        pointerId: 3,
        clientX: 36,
      }),
    );

    // Assert
    expect(committed).toEqual([24, 30]);
    expect(resize.value()).toBe(30);
  });
});
