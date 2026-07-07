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

function clamp(value: number, min: number, max: number) {
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

  function setContainer(node: HTMLElement | null) {
    container = node;
  }

  function setSplit(nextPercent: number) {
    const clampedPercent = clamp(nextPercent, minPercent, maxPercent);
    setSizePercent(clampedPercent);
    onResize?.(clampedPercent);
  }

  function setSplitFromPointer(clientX: number, clientY: number) {
    const root = container;
    if (!root || !root.isConnected) {
      return;
    }

    const rect = root.getBoundingClientRect();
    const nextPercent = clamp(
      orientation === "horizontal"
        ? ((clientX - rect.left) / rect.width) * 100
        : ((clientY - rect.top) / rect.height) * 100,
      minPercent,
      maxPercent,
    );
    setSplit(nextPercent);
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
    setSplitFromPointer(event.clientX, event.clientY);
    event.preventDefault();
    target.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragging()) {
      return;
    }

    setSplitFromPointer(event.clientX, event.clientY);
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
      <div class="cassie-resizable-split-pane" style={primaryStyle}>
        {first}
      </div>
      <div
        class="cassie-resizable-split-handle"
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
