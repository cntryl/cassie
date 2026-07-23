import { Toolbar } from "@askrjs/themes/components";

import { QueryEditorToolbar } from "./query-editor-toolbar";
import {
  MonacoCompletionContext,
  MonacoCompletionItem,
  MonacoSqlEditor,
} from "./monaco-sql-editor";

interface QueryEditorPanelProps {
  tabId: string;
  database: string;
  active: () => boolean;
  query: string;
  onQueryChange: (query: string) => void;
  isRunning: boolean;
  actionsEnabled: boolean;
  isStopping: boolean;
  onTrim: () => void;
  onValidate: () => void;
  onExplain: () => void;
  onPlay: () => void;
  onStop: () => void;
  completionItems: (context: MonacoCompletionContext) => MonacoCompletionItem[];
}

export function QueryEditorPanel({
  tabId,
  database,
  active,
  query,
  onQueryChange,
  isRunning,
  actionsEnabled,
  isStopping,
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
        title={
          <span class="cassie-query-editor-title">
            SQL Editor <span class="cassie-query-database-badge">{database}</span>
          </span>
        }
        paddingX="sm"
        paddingY="xs"
        gap="sm"
        borderBottom
        background="surface"
        actions={
          <QueryEditorToolbar
            onTrim={onTrim}
            onValidate={onValidate}
            onExplain={onExplain}
            onPlay={onPlay}
            onStop={onStop}
            isBusy={isRunning}
            isStopping={isStopping}
            hasQuery={query.trim().length > 0 && actionsEnabled}
          />
        }
      />
      <div class="cassie-query-editor-wrapper">
        <MonacoSqlEditor
          tabId={tabId}
          active={active}
          value={query}
          onChange={onQueryChange}
          disabled={isRunning}
          completionProvider={completionItems}
        />
      </div>
    </section>
  );
}
