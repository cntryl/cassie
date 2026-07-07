import { Button } from "@askrjs/themes/components";
import { CheckCircle2Icon, PlayIcon, RefreshCwIcon, SquareIcon } from "@askrjs/lucide";

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
      <Button
        type="button"
        size="sm"
        variant="secondary"
        onPress={onFormat}
      >
        Fmt
      </Button>
      <Button
        type="button"
        size="sm"
        variant="secondary"
        onPress={onValidate}
      >
        <CheckCircle2Icon size={14} />
        <span>Validate</span>
      </Button>
      <Button
        type="button"
        size="sm"
        variant="secondary"
        onPress={onExplain}
      >
        <RefreshCwIcon size={14} />
        <span>Explain</span>
      </Button>
      <Button
        type="button"
        variant="primary"
        size="sm"
        disabled={!canRun || isRunning}
        data-action="play"
        onPress={onPlay}
      >
        <PlayIcon size={14} />
        <span>Run</span>
      </Button>
      <Button
        type="button"
        variant="destructive"
        size="sm"
        disabled={!isRunning}
        data-action="stop"
        onPress={onStop}
      >
        <SquareIcon size={14} />
        <span>Stop</span>
      </Button>
    </div>
  );
}
