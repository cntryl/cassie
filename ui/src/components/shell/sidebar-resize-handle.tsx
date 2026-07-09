import { state } from "@askrjs/askr";

import { clamp } from "@/components/query/resizable-split";

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
  const [px, setPx] = state(clamp(initialPx, SIDEBAR_WIDTH_MIN_PX, SIDEBAR_WIDTH_MAX_PX));
  const [dragging, setDragging] = state(false);
  let startClientX = 0;
  let startPx = 0;
  let handleEl: HTMLElement | null = null;

  function setHandleEl(node: HTMLElement | null) {
    handleEl = node;
  }

  // During drag, only mutate the CSS var (via onDragMove, already imperative
  // in _layout.tsx) and aria-valuenow directly — a raw pointermove stream can
  // fire dozens of times a second, and committing state() that often forces a
  // full component re-render per event, which is what made dragging feel
  // janky. state() is only committed once, at drag end (or per discrete
  // keyboard step).
  function applyPxImperative(nextPx: number) {
    const clamped = clamp(nextPx, SIDEBAR_WIDTH_MIN_PX, SIDEBAR_WIDTH_MAX_PX);
    onDragMove(clamped);
    handleEl?.setAttribute("aria-valuenow", String(Math.round(clamped)));
    return clamped;
  }

  function onPointerDown(event: PointerEvent) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    setDragging(true);
    startClientX = event.clientX;
    startPx = px();
    event.preventDefault();
    target.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragging()) {
      return;
    }

    applyPxImperative(startPx + (event.clientX - startClientX));
    event.preventDefault();
  }

  function onPointerUp(event: PointerEvent) {
    if (!dragging()) {
      return;
    }

    const target = event.currentTarget;
    if (target instanceof HTMLElement) {
      target.releasePointerCapture(event.pointerId);
    }

    const finalPx = applyPxImperative(startPx + (event.clientX - startClientX));
    setPx(finalPx);
    setDragging(false);
    onDragEnd(finalPx);
  }

  function onKeyDown(event: KeyboardEvent) {
    const smallStep = 16;
    const largeStep = 48;
    const step = event.shiftKey ? largeStep : smallStep;
    const current = px();

    const nextPx = (() => {
      if (event.key === "Home") {
        return SIDEBAR_WIDTH_MIN_PX;
      }
      if (event.key === "End") {
        return SIDEBAR_WIDTH_MAX_PX;
      }
      if (event.key === "ArrowLeft") {
        return current - step;
      }
      if (event.key === "ArrowRight") {
        return current + step;
      }

      return null;
    })();

    if (nextPx === null) {
      return;
    }

    event.preventDefault();
    const clamped = applyPxImperative(nextPx);
    setPx(clamped);
    onDragEnd(clamped);
  }

  return (
    <div
      class="cassie-admin-sidebar-resize-handle"
      ref={setHandleEl}
      data-testid="admin-sidebar-resize-handle"
      data-dragging={dragging() ? "true" : undefined}
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize navigation sidebar"
      aria-valuemin={SIDEBAR_WIDTH_MIN_PX}
      aria-valuemax={SIDEBAR_WIDTH_MAX_PX}
      aria-valuenow={Math.round(px())}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onKeyDown={onKeyDown}
    />
  );
}
