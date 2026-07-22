import { Button } from "@askrjs/themes/components";
import { CheckCircle2Icon, EraserIcon, ListTreeIcon, PlayIcon, SquareIcon } from "@askrjs/lucide";

interface QueryEditorToolbarProps {
  onTrim: () => void;
  onValidate: () => void;
  onExplain: () => void;
  onPlay: () => void;
  onStop: () => void;
  isBusy: boolean;
  isStopping: boolean;
  hasQuery: boolean;
}

export function QueryEditorToolbar({
  onTrim,
  onValidate,
  onExplain,
  onPlay,
  onStop,
  isBusy,
  isStopping,
  hasQuery,
}: QueryEditorToolbarProps) {
  const canRun = hasQuery && !isBusy;

  return (
    <div class="cassie-query-editor-toolbar" data-testid="query-editor-toolbar" role="toolbar">
      <Button
        type="button"
        size="xs"
        variant="secondary"
        title="Trim whitespace"
        onPress={onTrim}
        disabled={isBusy}
      >
        <EraserIcon size={14} />
        <span>Trim</span>
      </Button>
      <Button
        type="button"
        size="xs"
        variant="secondary"
        title="Validate SQL"
        onPress={onValidate}
        disabled={!canRun}
      >
        <CheckCircle2Icon size={14} />
        <span>Validate</span>
      </Button>
      <Button
        type="button"
        size="xs"
        variant="secondary"
        title="Explain SQL execution plan"
        onPress={onExplain}
        disabled={!canRun}
      >
        <ListTreeIcon size={14} />
        <span>Explain</span>
      </Button>
      <Button
        type="button"
        variant="primary"
        size="xs"
        title="Execute SQL"
        disabled={!canRun}
        data-action="play"
        onPress={onPlay}
      >
        <PlayIcon size={14} />
        <span>Run</span>
      </Button>
      {isBusy ? (
        <Button
          type="button"
          variant="outline"
          size="xs"
          title="Stop running query operation"
          data-action="stop"
          onPress={onStop}
          disabled={isStopping}
        >
          <SquareIcon size={14} />
          <span>{isStopping ? "Stopping…" : "Stop"}</span>
        </Button>
      ) : null}
    </div>
  );
}
