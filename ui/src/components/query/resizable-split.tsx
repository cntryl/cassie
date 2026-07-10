import { createDragResize } from "@/shared/drag-resize";

interface ResizableSplitProps {
  orientation: "horizontal" | "vertical";
  initialSize: number;
  min?: number;
  max?: number;
  onResize?: (size: number) => void;
  first: unknown;
  second: unknown;
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
  let container: HTMLElement | null = null;
  let primaryPane: HTMLElement | null = null;

  function setContainer(node: HTMLElement | null) {
    container = node;
  }

  function setPrimaryPane(node: HTMLElement | null) {
    primaryPane = node;
  }

  function percentFromPointer(clientX: number, clientY: number): number | null {
    const root = container;
    if (!root || !root.isConnected) {
      return null;
    }

    const rect = root.getBoundingClientRect();
    return orientation === "horizontal"
      ? ((clientX - rect.left) / rect.width) * 100
      : ((clientY - rect.top) / rect.height) * 100;
  }

  function applyPercent(nextPercent: number) {
    if (!primaryPane) {
      return;
    }

    if (orientation === "horizontal") {
      primaryPane.style.inlineSize = `${nextPercent}%`;
    } else {
      primaryPane.style.blockSize = `${nextPercent}%`;
    }
  }

  const resize = createDragResize({
    min,
    max,
    initialValue: initialSize,
    smallStep: 2,
    largeStep: 10,
    decreaseKeys: orientation === "horizontal" ? ["ArrowLeft"] : ["ArrowUp"],
    increaseKeys: orientation === "horizontal" ? ["ArrowRight"] : ["ArrowDown"],
    computeNextValue: (event) => percentFromPointer(event.clientX, event.clientY),
    applyValue: applyPercent,
    onCommit: onResize,
  });

  const split = resize.value();
  const isDragging = resize.dragging();
  const primaryStyle = {
    flex: "0 0 auto",
    ...(orientation === "horizontal"
      ? { inlineSize: `${split}%`, minInlineSize: `${min}%`, maxInlineSize: `${max}%` }
      : { blockSize: `${split}%`, minBlockSize: `${min}%`, maxBlockSize: `${max}%` }),
  };
  const secondaryStyle = {
    flex: "1 1 auto",
  };
  const separatorAttributes = {
    "aria-label": `Resize ${orientation} split`,
    "aria-orientation": orientation,
    "aria-valuemax": max,
    "aria-valuemin": min,
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
        ref={resize.setHandleEl}
        onPointerDown={resize.onPointerDown}
        onPointerMove={resize.onPointerMove}
        onPointerUp={resize.onPointerUp}
        onKeyDown={resize.onKeyDown}
        tabIndex={0}
        {...separatorAttributes}
      />
      <div class="cassie-resizable-split-pane" style={secondaryStyle}>
        {second}
      </div>
    </div>
  );
}
