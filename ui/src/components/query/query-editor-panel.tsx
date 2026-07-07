import { Text } from "@askrjs/themes/components";

import { QueryEditorToolbar } from "./query-editor-toolbar";
import { MonacoCompletionItem, MonacoSqlEditor } from "./monaco-sql-editor";

interface QueryEditorPanelProps {
  query: string;
  onQueryChange: (query: string) => void;
  isRunning: boolean;
  onFormat: () => void;
  onValidate: () => void;
  onExplain: () => void;
  onPlay: () => void;
  onStop: () => void;
  completionItems: MonacoCompletionItem[];
}

export function QueryEditorPanel({
  query,
  onQueryChange,
  isRunning,
  onFormat,
  onValidate,
  onExplain,
  onPlay,
  onStop,
  completionItems,
}: QueryEditorPanelProps) {
  return (
    <section class="cassie-query-editor-panel" data-testid="query-editor-panel" aria-label="Query editor panel">
      <header class="cassie-query-editor-panel-header">
        <Text size="sm" weight="semibold">
          SQL Editor
        </Text>
      </header>
      <QueryEditorToolbar
        onFormat={onFormat}
        onValidate={onValidate}
        onExplain={onExplain}
        onPlay={onPlay}
        onStop={onStop}
        isBusy={isRunning}
        hasQuery={query.trim().length > 0}
      />
      <div class="cassie-query-editor-wrapper">
        <MonacoSqlEditor
          value={query}
          onChange={onQueryChange}
          disabled={isRunning}
          completionProvider={() => completionItems}
        />
      </div>
    </section>
  );
}
