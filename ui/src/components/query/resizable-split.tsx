import { state } from "@askrjs/askr";
import { task } from "@askrjs/askr/resources";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@askrjs/themes/components";
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
  const [elements] = state<{
    container: HTMLElement | null;
    primaryPane: HTMLElement | null;
  }>({ container: null, primaryPane: null });

  function setContainer(node: HTMLElement | null) {
    elements().container = node;
  }

  function setPrimaryPane(node: HTMLElement | null) {
    elements().primaryPane = node;
  }

  function percentFromPointer(clientX: number, clientY: number): number | null {
    const root = elements().container;
    if (!root || !root.isConnected) {
      return null;
    }

    const rect = root.getBoundingClientRect();
    return orientation === "horizontal"
      ? ((clientX - rect.left) / rect.width) * 100
      : ((clientY - rect.top) / rect.height) * 100;
  }

  function applyPercent(nextPercent: number) {
    const { container, primaryPane } = elements();
    if (!primaryPane || !container) {
      return;
    }

    if (orientation === "horizontal") {
      primaryPane.style.inlineSize = `${nextPercent}%`;
    } else {
      container.style.setProperty("--cassie-split-size", `${nextPercent}%`);
    }
  }

  const [resizeState] = state(
    createDragResize({
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
    }),
  );
  const resize = resizeState();
  task(() => () => resize.dispose());

  const split = resize.value();
  const isDragging = resize.dragging();
  const primaryStyle = {
    flex: "0 0 auto",
    ...(orientation === "horizontal"
      ? { inlineSize: `${split}%`, minInlineSize: `${min}%`, maxInlineSize: `${max}%` }
      : {}),
  };
  const containerStyle =
    orientation === "vertical" ? { "--cassie-split-size": `${split}%` } : undefined;
  const secondaryStyle = {
    flex: "1 1 auto",
  };
  const separatorAttributes = {
    "aria-label": `Resize ${orientation} split`,
    "aria-orientation": orientation === "vertical" ? "horizontal" : "vertical",
    "aria-valuemax": max,
    "aria-valuemin": min,
    "aria-valuenow": Math.round(split),
    role: "separator",
  };

  return (
    <ResizablePanelGroup
      class={`cassie-resizable-split cassie-resizable-split-${orientation}`}
      ref={setContainer}
      style={containerStyle}
      data-dragging={isDragging ? "true" : undefined}
      data-testid={`query-resizable-split-${orientation}`}
    >
      <ResizablePanel class="cassie-resizable-split-pane" ref={setPrimaryPane} style={primaryStyle}>
        {first}
      </ResizablePanel>
      <ResizableHandle
        class="cassie-resizable-split-handle"
        ref={resize.setHandleEl}
        onPointerDown={resize.onPointerDown}
        onPointerMove={resize.onPointerMove}
        onPointerUp={resize.onPointerUp}
        onPointerCancel={resize.onPointerCancel}
        onLostPointerCapture={resize.onLostPointerCapture}
        onKeyDown={resize.onKeyDown}
        tabIndex={0}
        {...separatorAttributes}
      />
      <ResizablePanel class="cassie-resizable-split-pane" style={secondaryStyle}>
        {second}
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}
