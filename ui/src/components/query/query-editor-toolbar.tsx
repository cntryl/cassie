interface QueryEditorToolbarProps {
  onFormat: () => void;
  onValidate: () => void;
  onExplain: () => void;
  onPlay: () => void;
  onStop: () => void;
  isRunning: boolean;
  canRun: boolean;
}

export function QueryEditorToolbar({
  onFormat,
  onValidate,
  onExplain,
  onPlay,
  onStop,
  isRunning,
  canRun,
}: QueryEditorToolbarProps) {
  return (
    <div class="cassie-query-editor-toolbar" data-testid="query-editor-toolbar" role="toolbar">
      <button type="button" class="cassie-query-toolbar-btn" onClick={onFormat}>
        Fmt
      </button>
      <button type="button" class="cassie-query-toolbar-btn" onClick={onValidate}>
        Validate
      </button>
      <button type="button" class="cassie-query-toolbar-btn" onClick={onExplain}>
        Explain
      </button>
      <button
        type="button"
        class="cassie-query-toolbar-btn cassie-query-toolbar-btn-primary"
        disabled={!canRun || isRunning}
        data-action="play"
        onClick={onPlay}
      >
        Play
      </button>
      <button
        type="button"
        class="cassie-query-toolbar-btn cassie-query-toolbar-btn-danger"
        disabled={!isRunning}
        data-action="stop"
        onClick={onStop}
      >
        Stop
      </button>
    </div>
  );
}
