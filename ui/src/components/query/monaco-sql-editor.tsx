import { state } from "@askrjs/askr";

// Monaco's async load has been causing render-cycle instability elsewhere in
// the page (see project memory: askr framework gotchas #8). Disabled for now
// in favor of the plain-textarea fallback below; re-enable by flipping this
// once that's tracked down, rather than fighting it here.
const MONACO_ENABLED = false;

export interface MonacoCompletionItem {
  label: string;
  detail?: string;
  insertText: string;
}

interface MonacoEditorContext {
  editor: unknown;
  model: unknown;
  changeSubscription: { dispose: () => void } | null;
  completionProvider: () => MonacoCompletionItem[];
  completionSubscription: { dispose: () => void } | null;
  suppressNextChange: boolean;
}

export interface MonacoSqlEditorProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  completionProvider?: () => MonacoCompletionItem[];
}

function toSuggestions(items: MonacoCompletionItem[]) {
  return items.map((item) => ({
    label: item.label,
    insertText: item.insertText,
    detail: item.detail,
    kind: 17,
    documentation: item.detail ?? "",
  }));
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
  const isEditorUnavailable = editorUnavailable();
  const shouldUseFallback =
    !MONACO_ENABLED || isTestMode || typeof window === "undefined" || isEditorUnavailable;
  const latestCompletionProvider = completionProvider ?? emptyCompletionItems;
  const [editorContext, setEditorContext] = state<MonacoEditorContext | null>(null);

  const context = editorContext();
  if (context && !shouldUseFallback) {
    context.completionProvider = latestCompletionProvider;
    const editorObj = context.editor as {
      getValue: () => string;
      getModel: () => { setValue: (value: string) => void };
      setValue?: (value: string) => void;
      updateOptions?: (options: { readOnly: boolean }) => void;
    };

    const currentValue = editorObj.getValue();
    if (currentValue !== value && editorObj.getModel) {
      context.suppressNextChange = true;
      editorObj.getModel().setValue(value);
    }

    if (editorObj.updateOptions) {
      editorObj.updateOptions({ readOnly: disabled });
    }
  }

  function disposeEditor() {
    const current = editorContext();
    if (!current) {
      return;
    }

    current.changeSubscription?.dispose();
    current.completionSubscription?.dispose();
    const editorObj = current.editor as { dispose: () => void };
    const modelObj = current.model as { dispose: () => void } | null;
    editorObj.dispose();
    modelObj?.dispose();
    setEditorContext(null);
  }

  const mountEditor = (node: HTMLElement | null) => {
    if (node === null) {
      disposeEditor();
      return;
    }

    if (editorContext()) {
      return;
    }

    void (async () => {
      try {
        if (shouldUseFallback) {
          return;
        }

        const monaco = (await import("monaco-editor/esm/vs/editor/editor.api")) as unknown as {
          editor: {
            create: (element: HTMLElement, options: any) => unknown;
            createModel: (value: string, language: string) => unknown;
            ITextModel?: unknown;
          };
          languages: {
            registerCompletionItemProvider: (
              language: string,
              provider: {
                provideCompletionItems: (
                  model: unknown,
                  position: unknown,
                ) => { suggestions: Array<Record<string, unknown>> };
              },
            ) => { dispose: () => void };
            CompletionItemKind: {
              Snippet: number;
            };
          };
        };
        const model = monaco.editor.createModel(value, "sql");
        const editor = monaco.editor.create(node, {
          model,
          readOnly: disabled,
          automaticLayout: true,
          minimap: { enabled: false },
          fontSize: 13,
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          renderWhitespace: "none",
          wordWrap: "on",
          theme: "vs-dark",
        });
        const ctx: MonacoEditorContext = {
          editor,
          model,
          changeSubscription: null,
          completionProvider: latestCompletionProvider,
          completionSubscription: null,
          suppressNextChange: false,
        };

        ctx.changeSubscription = (
          editor as {
            onDidChangeModelContent: (listener: () => void) => { dispose: () => void };
          }
        ).onDidChangeModelContent(() => {
          if (ctx.suppressNextChange) {
            ctx.suppressNextChange = false;
            return;
          }

          const nextValue = (editor as { getValue: () => string }).getValue();
          onChange(nextValue);
        });

        ctx.completionSubscription = monaco.languages.registerCompletionItemProvider("sql", {
          provideCompletionItems: () => ({
            suggestions: toSuggestions(ctx.completionProvider()).map((item) => ({
              ...item,
              kind: monaco.languages.CompletionItemKind.Snippet,
            })),
          }),
        });

        setEditorContext(ctx);
      } catch {
        setEditorContext(null);
        setEditorUnavailable(true);
      }
    })();
  };

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

  if (shouldUseFallback) {
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

  return (
    <div
      ref={mountEditor}
      class="cassie-query-editor-host"
      data-testid="query-editor"
      data-query-editor="monaco"
      aria-label="SQL editor"
    />
  );
}
