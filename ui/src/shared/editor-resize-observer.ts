export interface EditorLayoutObserver {
  disconnect(): void;
}

export function observeEditorLayout(
  element: HTMLElement,
  layout: () => void,
): EditorLayoutObserver {
  let width = -1;
  let height = -1;
  let frame: number | null = null;

  const observer = new ResizeObserver((entries) => {
    const entry = entries[entries.length - 1];
    if (!entry || (entry.contentRect.width === width && entry.contentRect.height === height)) {
      return;
    }
    width = entry.contentRect.width;
    height = entry.contentRect.height;
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      layout();
    });
  });
  observer.observe(element);

  return {
    disconnect() {
      observer.disconnect();
      if (frame !== null) {
        cancelAnimationFrame(frame);
        frame = null;
      }
    },
  };
}
