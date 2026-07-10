import { Toolbar } from "@askrjs/themes/components";

import { QueryEditorToolbar } from "./query-editor-toolbar";
import { MonacoCompletionItem, MonacoSqlEditor } from "./monaco-sql-editor";

interface QueryEditorPanelProps {
  query: string;
  onQueryChange: (query: string) => void;
  isRunning: boolean;
  onTrim: () => void;
  onValidate: () => void;
  onExplain: () => void;
  onPlay: () => void;
  onStop: () => void;
  completionItems: () => MonacoCompletionItem[];
}

export function QueryEditorPanel({
  query,
  onQueryChange,
  isRunning,
  onTrim,
  onValidate,
  onExplain,
  onPlay,
  onStop,
  completionItems,
}: QueryEditorPanelProps) {
  // Attached here (not inside MonacoSqlEditor) so the run shortcut works
  // without threading a new prop through the Monaco file — that file is
  // already fragile (see its MONACO_ENABLED comment) and best left untouched.
  // Listens on the *capture* phase (fires top-down, before any descendant's
  // own bubble-phase handling) rather than bubble, specifically so that once
  // Monaco is re-enabled, its internal key-binding service can't intercept or
  // stop-propagate this combo before it reaches us — preventDefault() here
  // still suppresses the textarea/editor's default Enter behavior either way,
  // since default actions run after the full capture+bubble dispatch.
  function handleKeyDown(event: KeyboardEvent) {
    const isRunShortcut = event.key === "Enter" && (event.metaKey || event.ctrlKey);
    if (!isRunShortcut || isRunning || query.trim().length === 0) {
      return;
    }

    event.preventDefault();
    onPlay();
  }

  return (
    <section
      class="cassie-query-editor-panel"
      data-testid="query-editor-panel"
      aria-label="Query editor panel"
      onKeyDownCapture={handleKeyDown}
    >
      <Toolbar
        class="cassie-query-editor-panel-header"
        title="SQL Editor"
        actions={
          <QueryEditorToolbar
            onTrim={onTrim}
            onValidate={onValidate}
            onExplain={onExplain}
            onPlay={onPlay}
            onStop={onStop}
            isBusy={isRunning}
            hasQuery={query.trim().length > 0}
          />
        }
      />
      <div class="cassie-query-editor-wrapper">
        <MonacoSqlEditor
          value={query}
          onChange={onQueryChange}
          disabled={isRunning}
          completionProvider={completionItems}
        />
      </div>
    </section>
  );
}
