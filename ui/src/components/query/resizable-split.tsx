import { state } from "@askrjs/askr";

interface ResizableSplitProps {
  orientation: "horizontal" | "vertical";
  initialSize: number;
  min?: number;
  max?: number;
  onResize?: (size: number) => void;
  first: unknown;
  second: unknown;
}

export function clamp(value: number, min: number, max: number) {
  if (Number.isNaN(value)) {
    return min;
  }

  return Math.min(Math.max(value, min), max);
}

export function ResizableSplit({
  orientation,
  initialSize,
  min = 20,
  max = 80,
  onResize,
  first,
  second,
}: ResizableSplitProps) {
  const minPercent = min;
  const maxPercent = max;
  const [sizePercent, setSizePercent] = state(clamp(initialSize, minPercent, maxPercent));
  const [dragging, setDragging] = state(false);
  let container: HTMLElement | null = null;
  let primaryPane: HTMLElement | null = null;
  let handleEl: HTMLElement | null = null;

  function setContainer(node: HTMLElement | null) {
    container = node;
  }

  function setPrimaryPane(node: HTMLElement | null) {
    primaryPane = node;
  }

  function setHandleEl(node: HTMLElement | null) {
    handleEl = node;
  }

  // During drag, mutate the pane size and aria-valuenow directly instead of
  // going through state() on every pointermove — a raw pointermove stream can
  // fire dozens of times a second, and committing state() that often forces a
  // full component re-render per event, which is what made dragging feel
  // janky. state() is only committed once, at drag end (or per discrete
  // keyboard step), mirroring the sidebar width CSS-var technique in
  // _layout.tsx.
  function applyPercent(nextPercent: number) {
    if (primaryPane) {
      if (orientation === "horizontal") {
        primaryPane.style.inlineSize = `${nextPercent}%`;
      } else {
        primaryPane.style.blockSize = `${nextPercent}%`;
      }
    }
    handleEl?.setAttribute("aria-valuenow", String(Math.round(nextPercent)));
  }

  function setSplit(nextPercent: number) {
    const clampedPercent = clamp(nextPercent, minPercent, maxPercent);
    setSizePercent(clampedPercent);
    onResize?.(clampedPercent);
    return clampedPercent;
  }

  function percentFromPointer(clientX: number, clientY: number): number | null {
    const root = container;
    if (!root || !root.isConnected) {
      return null;
    }

    const rect = root.getBoundingClientRect();
    return clamp(
      orientation === "horizontal"
        ? ((clientX - rect.left) / rect.width) * 100
        : ((clientY - rect.top) / rect.height) * 100,
      minPercent,
      maxPercent,
    );
  }

  function onPointerDown(event: PointerEvent) {
    if (container === null) {
      return;
    }

    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    setDragging(true);
    const percent = percentFromPointer(event.clientX, event.clientY);
    if (percent !== null) {
      applyPercent(percent);
    }
    event.preventDefault();
    target.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragging()) {
      return;
    }

    const percent = percentFromPointer(event.clientX, event.clientY);
    if (percent !== null) {
      applyPercent(percent);
    }
    event.preventDefault();
  }

  function onPointerUp(event: PointerEvent) {
    if (!dragging()) {
      return;
    }

    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    const percent = percentFromPointer(event.clientX, event.clientY) ?? sizePercent();
    setSplit(percent);
    setDragging(false);
    target.releasePointerCapture(event.pointerId);
  }

  function onKeyDown(event: KeyboardEvent) {
    const largeStep = 10;
    const smallStep = 2;
    const step = event.shiftKey ? largeStep : smallStep;
    const current = sizePercent();

    const nextPercent = (() => {
      if (event.key === "Home") {
        return minPercent;
      }
      if (event.key === "End") {
        return maxPercent;
      }
      if (orientation === "horizontal" && event.key === "ArrowLeft") {
        return current - step;
      }
      if (orientation === "horizontal" && event.key === "ArrowRight") {
        return current + step;
      }
      if (orientation === "vertical" && event.key === "ArrowUp") {
        return current - step;
      }
      if (orientation === "vertical" && event.key === "ArrowDown") {
        return current + step;
      }

      return null;
    })();

    if (nextPercent === null) {
      return;
    }

    setSplit(nextPercent);
    event.preventDefault();
  }

  const split = sizePercent();
  const isDragging = dragging();
  const primaryStyle = {
    flex: "0 0 auto",
    ...(orientation === "horizontal"
      ? {
          inlineSize: `${split}%`,
          minInlineSize: `${minPercent}%`,
          maxInlineSize: `${maxPercent}%`,
        }
      : { blockSize: `${split}%`, minBlockSize: `${minPercent}%`, maxBlockSize: `${maxPercent}%` }),
  };
  const secondaryStyle = {
    flex: "1 1 auto",
  };
  const separatorAttributes = {
    "aria-label": `Resize ${orientation} split`,
    "aria-orientation": orientation,
    "aria-valuemax": maxPercent,
    "aria-valuemin": minPercent,
    "aria-valuenow": Math.round(split),
    role: "separator",
  };

  return (
    <div
      class={`cassie-resizable-split cassie-resizable-split-${orientation}`}
      ref={setContainer}
      data-dragging={isDragging ? "true" : undefined}
      data-testid={`query-resizable-split-${orientation}`}
    >
      <div class="cassie-resizable-split-pane" ref={setPrimaryPane} style={primaryStyle}>
        {first}
      </div>
      <div
        class="cassie-resizable-split-handle"
        ref={setHandleEl}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onKeyDown={onKeyDown}
        tabIndex={0}
        {...separatorAttributes}
      />
      <div class="cassie-resizable-split-pane" style={secondaryStyle}>
        {second}
      </div>
    </div>
  );
}
