import { createDragResize } from "@/shared/drag-resize";

export const SIDEBAR_WIDTH_MIN_PX = 224;
export const SIDEBAR_WIDTH_MAX_PX = 512;

interface SidebarResizeHandleProps {
  initialPx: number;
  onDragMove: (px: number) => void;
  onDragEnd: (px: number) => void;
}

export function SidebarResizeHandle({
  initialPx,
  onDragMove,
  onDragEnd,
}: SidebarResizeHandleProps) {
  const resize = createDragResize({
    min: SIDEBAR_WIDTH_MIN_PX,
    max: SIDEBAR_WIDTH_MAX_PX,
    initialValue: initialPx,
    smallStep: 16,
    largeStep: 48,
    decreaseKeys: ["ArrowLeft"],
    increaseKeys: ["ArrowRight"],
    computeNextValue: (event, start) => start.value + (event.clientX - start.clientX),
    applyValue: onDragMove,
    onCommit: onDragEnd,
  });

  return (
    <div
      class="cassie-admin-sidebar-resize-handle"
      ref={resize.setHandleEl}
      data-testid="admin-sidebar-resize-handle"
      data-dragging={resize.dragging() ? "true" : undefined}
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize navigation sidebar"
      aria-valuemin={SIDEBAR_WIDTH_MIN_PX}
      aria-valuemax={SIDEBAR_WIDTH_MAX_PX}
      aria-valuenow={Math.round(resize.value())}
      tabIndex={0}
      onPointerDown={resize.onPointerDown}
      onPointerMove={resize.onPointerMove}
      onPointerUp={resize.onPointerUp}
      onKeyDown={resize.onKeyDown}
    />
  );
}
