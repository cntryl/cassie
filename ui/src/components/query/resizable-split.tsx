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

  function setSplitFromPointer(clientX: number, clientY: number) {
    const root = container;
    if (!root || !root.isConnected) {
      return;
    }

    const rect = root.getBoundingClientRect();
    const nextPercent = clamp(
      orientation === "horizontal" ? ((clientX - rect.left) / rect.width) * 100 : ((clientY - rect.top) / rect.height) * 100,
      minPercent,
      maxPercent,
    );
    setSizePercent(nextPercent);
    onResize?.(nextPercent);
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

  const split = sizePercent();
  const primaryStyle = {
    flex: "0 0 auto",
    ...(orientation === "horizontal"
      ? { inlineSize: `${split}%`, minInlineSize: `${minPercent}%`, maxInlineSize: `${maxPercent}%` }
      : { blockSize: `${split}%`, minBlockSize: `${minPercent}%`, maxBlockSize: `${maxPercent}%` }),
  };
  const secondaryStyle = {
    flex: "1 1 auto",
  };
  const separatorAttributes = {
    "aria-label": `Resize ${orientation} split`,
    "aria-orientation": orientation,
    role: "separator",
  };

  return (
    <div
      class={`cassie-resizable-split cassie-resizable-split-${orientation}`}
      ref={setContainer}
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
        tabIndex={0}
        {...separatorAttributes}
      />
      <div class="cassie-resizable-split-pane" style={secondaryStyle}>
        {second}
      </div>
    </div>
  );
}
