import { state } from "@askrjs/askr";

import { MonacoEditor, type MonacoEditorInstance, type MonacoNamespace } from "@/vendor/askr-monaco";

export interface MonacoCompletionItem {
  label: string;
  detail?: string;
  insertText: string;
}

export interface MonacoSqlEditorProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  completionProvider?: () => MonacoCompletionItem[];
}

function emptyCompletionItems(): MonacoCompletionItem[] {
  return [];
}

export function MonacoSqlEditor({
  value,
  onChange,
  disabled = false,
  completionProvider,
}: MonacoSqlEditorProps) {
  const isTestMode = import.meta.env.MODE === "test";
  const [editorUnavailable, setEditorUnavailable] = state(false);
  const latestCompletionProvider = completionProvider ?? emptyCompletionItems;

  function handleFallbackKeyDown(event: KeyboardEvent) {
    if (event.key !== "Tab" || event.shiftKey) {
      return;
    }

    const input =
      event.currentTarget instanceof HTMLTextAreaElement
        ? event.currentTarget
        : event.target instanceof HTMLTextAreaElement
          ? event.target
          : null;
    if (!input) {
      return;
    }

    // A plain <textarea> treats Tab as a focus-move, not an indent — that's
    // fine for a form field but not for a code editor. Insert a soft tab at
    // the cursor instead, and restore the cursor position after the
    // controlled `value` update (assigning .value resets it to the end).
    event.preventDefault();

    const indent = "  ";
    const { selectionStart, selectionEnd, value: current } = input;
    const next = `${current.slice(0, selectionStart)}${indent}${current.slice(selectionEnd)}`;
    const cursor = selectionStart + indent.length;

    onChange(next);
    requestAnimationFrame(() => {
      input.selectionStart = cursor;
      input.selectionEnd = cursor;
    });
  }

  // jsdom can't run Monaco (no real Worker/Canvas/ResizeObserver support), so
  // tests always exercise this same plain-textarea contract instead — real
  // browsers only fall back here if Monaco itself fails to load (onError).
  if (isTestMode || typeof window === "undefined" || editorUnavailable()) {
    return (
      <div
        class="cassie-query-editor-host"
        data-query-editor="fallback"
        data-testid="query-editor"
        aria-label="SQL editor"
      >
        <textarea
          aria-label="SQL query"
          class="cassie-query-editor-fallback"
          value={value}
          onInput={(event: InputEvent) => {
            const input =
              event.currentTarget instanceof HTMLTextAreaElement
                ? event.currentTarget
                : event.target instanceof HTMLTextAreaElement
                  ? event.target
                  : null;
            if (!input) {
              return;
            }

            onChange(input.value);
          }}
          onKeyDown={handleFallbackKeyDown}
          disabled={disabled}
          rows={10}
          spellcheck={false}
        />
      </div>
    );
  }

  function handleBeforeMount(monaco: MonacoNamespace) {
    monaco.languages.registerCompletionItemProvider("sql", {
      provideCompletionItems: (model, position) => {
        const word = model.getWordUntilPosition(position);
        const range = {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endColumn: word.endColumn,
        };

        return {
          suggestions: latestCompletionProvider().map((item) => ({
            label: item.label,
            insertText: item.insertText,
            detail: item.detail,
            documentation: item.detail ?? "",
            kind: monaco.languages.CompletionItemKind.Snippet,
            range,
          })),
        };
      },
    });
  }

  function handleMount(editor: MonacoEditorInstance) {
    editor.onDidChangeModelContent(() => {
      onChange(editor.getValue());
    });
  }

  return (
    <div
      class="cassie-query-editor-host"
      data-testid="query-editor"
      data-query-editor="monaco"
      aria-label="SQL editor"
    >
      <MonacoEditor
        value={value}
        language="sql"
        theme="vs-dark"
        options={{
          readOnly: disabled,
          automaticLayout: true,
          minimap: { enabled: false },
          fontSize: 13,
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          renderWhitespace: "none",
          wordWrap: "on",
        }}
        beforeMount={handleBeforeMount}
        onMount={handleMount}
        onError={() => setEditorUnavailable(true)}
        aria-label="SQL editor"
      />
    </div>
  );
}
