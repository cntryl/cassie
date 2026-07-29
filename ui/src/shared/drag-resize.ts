import { state } from "@askrjs/askr";

export function clamp(value: number, min: number, max: number) {
  if (Number.isNaN(value)) {
    return min;
  }

  return Math.min(Math.max(value, min), max);
}

export interface DragResizeStart {
  clientX: number;
  clientY: number;
  value: number;
}

export interface DragResizeOptions {
  min: number;
  max: number;
  initialValue: number;
  smallStep: number;
  largeStep: number;
  decreaseKeys: readonly string[];
  increaseKeys: readonly string[];
  /** Compute the candidate value for a pointer event; return null to ignore it (e.g. container not yet connected). */
  computeNextValue: (event: PointerEvent, start: DragResizeStart) => number | null;
  /** Imperative side effect applied on every pointermove and on commit — mutate DOM/CSS vars directly rather than through state(), since a raw pointermove stream can fire dozens of times a second and a state()-driven re-render per event is what makes dragging feel janky. */
  applyValue: (value: number) => void;
  /** Called once the value is finalized: on pointerup or a discrete keyboard step. */
  onCommit?: (value: number) => void;
}

// Shared pointer-drag + keyboard-step resize interaction: used by both the
// query editor/results split pane and the admin nav sidebar width handle,
// which independently reimplemented this exact pattern (pointer capture,
// imperative apply during drag, Home/End/Arrow+shift-step keyboard support,
// aria-valuenow bookkeeping) before this extraction. The two callers differ
// in what a "next value" means (percent-from-rect for the split vs.
// delta-from-pointer-origin for the sidebar) and in what applying it means
// (mutating a pane's own inline style vs. setting a CSS var on an ancestor),
// so both are left as caller-supplied callbacks rather than folded in here.
export function createDragResize(options: DragResizeOptions) {
  const [getValue, setValue] = state(clamp(options.initialValue, options.min, options.max));
  const [getDragging, setDraggingState] = state(false);
  const [getActivePointerId, setActivePointerIdState] = state<number | null>(null);

  function setActivePointerId(pointerId: number | null) {
    setActivePointerIdState(pointerId);
  }
  function setDraggingState(isDragging: boolean) {
    setDraggingState(isDragging);
  }
  function value() {
    return getDragging() || getActivePointerId() !== null ? pendingValue : getValue();
  }
  let start: DragResizeStart = { clientX: 0, clientY: 0, value: 0 };
  let handleEl: HTMLElement | null = null;
  let pendingValue = clamp(options.initialValue, options.min, options.max);

  function setHandleEl(node: HTMLElement | null) {
    handleEl = node;
  }

  function applyClamped(nextValue: number) {
    const clamped = clamp(nextValue, options.min, options.max);
    pendingValue = clamped;
    options.applyValue(clamped);
    handleEl?.setAttribute("aria-valuenow", String(Math.round(clamped)));
    return clamped;
  }

  function commit(nextValue: number) {
    const clamped = applyClamped(nextValue);
    setValue(clamped);
    options.onCommit?.(clamped);
    return clamped;
  }

  function onPointerDown(event: PointerEvent) {
    const target = event.currentTarget ?? event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }
    if (activePointerId !== null) {
      return;
    }

    start = { clientX: event.clientX, clientY: event.clientY, value: value() };
    const next = options.computeNextValue(event, start);
    // Unlike a mid-drag pointermove (where a null value just means "skip this
    // update, stay dragging"), a null result at the very start of the gesture
    // means the caller can't resolve a value at all (e.g. its container ref
    // isn't connected yet) — there's nothing to drag, so abort before
    // entering a dragging state or capturing the pointer.
    if (next === null) {
      return;
    }

    setDraggingState(true);
    setActivePointerIdState(event.pointerId);
    event.preventDefault();
    target.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    const dragging = getDragging();
    const activePointerId = getActivePointerId();
    if (!dragging || activePointerId === null) {
      return;
    }

    const next = options.computeNextValue(event, start);
    if (next !== null) {
      applyClamped(next);
    }
    event.preventDefault();
  }

  function onPointerUp(event: PointerEvent) {
    const activePointerId = getActivePointerId();
    if (event.pointerId !== activePointerId) {
      return;
    }
    if (!getDragging()) {
      setActivePointerId(null);
      return;
    }

    const target = event.currentTarget ?? event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    const next = options.computeNextValue(event, start) ?? value();
    commit(next);
    setDraggingState(false);
    setActivePointerId(null);
    target.releasePointerCapture(event.pointerId);
  }

  function onPointerCancel(event: PointerEvent) {
    const activePointerId = getActivePointerId();
    if (event.pointerId !== activePointerId) {
      return;
    }
    if (!getDragging()) {
      setActivePointerId(null);
      return;
    }

    commit(pendingValue);
    setDraggingState(false);
    setActivePointerId(null);
    const target = event.currentTarget ?? event.target;
    if (target instanceof HTMLElement) {
      try {
        target.releasePointerCapture(event.pointerId);
      } catch {
        // The browser may already have released capture as part of cancelling
        // the gesture. The committed size is still the correct final state.
      }
    }
  }

  function onLostPointerCapture() {
    if (!getDragging()) {
      setActivePointerId(null);
      return;
    }
    if (getActivePointerId() === null) {
      return;
    }

    commit(pendingValue);
    setDraggingState(false);
    setActivePointerId(null);
  }

  function onKeyDown(event: KeyboardEvent) {
    const step = event.shiftKey ? options.largeStep : options.smallStep;
    const current = value();

    const nextValue = (() => {
      if (event.key === "Home") {
        return options.min;
      }
      if (event.key === "End") {
        return options.max;
      }
      if (options.decreaseKeys.includes(event.key)) {
        return current - step;
      }
      if (options.increaseKeys.includes(event.key)) {
        return current + step;
      }

      return null;
    })();

    if (nextValue === null) {
      return;
    }

    commit(nextValue);
    event.preventDefault();
  }

  return {
    value,
    dragging: () => getDragging(),
    setHandleEl,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    onLostPointerCapture,
    onKeyDown,
  };
}
