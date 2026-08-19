import { state } from "@askrjs/askr";
import { raf } from "@askrjs/askr/fx";
import { on, task } from "@askrjs/askr/resources";

import { MonacoEditor, type MonacoEditorInstance, type MonacoNamespace } from "@askrjs/monaco";
import { observeEditorLayout, type EditorLayoutObserver } from "@/shared/editor-resize-observer";

export interface MonacoCompletionItem {
  label: string;
  detail?: string;
  insertText: string;
  filterText?: string;
  sortText?: string;
  kind?: "keyword" | "class" | "field" | "function" | "method" | "reference";
}

export interface MonacoCompletionContext {
  sql: string;
  offset: number;
  word: string;
}

export interface MonacoSqlEditorProps {
  tabId: string;
  active: () => boolean;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  completionProvider?: (context: MonacoCompletionContext) => MonacoCompletionItem[];
}

interface MonacoEditorResource {
  monaco: MonacoNamespace | null;
  editor: MonacoEditorInstance | null;
  changeDisposable: { dispose(): void } | null;
  completionDisposable: { dispose(): void } | null;
  completionProvider: (context: MonacoCompletionContext) => MonacoCompletionItem[];
  layoutObserver: EditorLayoutObserver | null;
  fallback: HTMLTextAreaElement | null;
  host: HTMLElement | null;
}

const editorResources = new Map<string, MonacoEditorResource>();

function getEditorResource(modelUri: string) {
  let resource = editorResources.get(modelUri);
  if (!resource) {
    resource = {
      monaco: null,
      editor: null,
      changeDisposable: null,
      completionDisposable: null,
      completionProvider: emptyCompletionItems,
      layoutObserver: null,
      fallback: null,
      host: null,
    };
    editorResources.set(modelUri, resource);
  }

  return resource;
}

export function setMonacoSqlEditorValue(tabId: string, value: string) {
  const modelUri = `inmemory://cassie/query/${encodeURIComponent(tabId)}.sql`;
  const resource = editorResources.get(modelUri);
  if (resource?.editor && resource.editor.getValue() !== value) {
    resource.editor.setValue(value);
  }
  if (resource?.fallback && resource.fallback.value !== value) {
    resource.fallback.value = value;
  }
}

function emptyCompletionItems(): MonacoCompletionItem[] {
  return [];
}

export function MonacoSqlEditor({
  tabId,
  active,
  value,
  onChange,
  disabled = false,
  completionProvider,
}: MonacoSqlEditorProps) {
  const isTestMode = import.meta.env.MODE === "test";
  const [editorUnavailable, setEditorUnavailable] = state(false);
  const [systemDark, setSystemDark] = state(
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
      : false,
  );
  const [explicitTheme, setExplicitTheme] = state(
    typeof document !== "undefined" ? document.documentElement.getAttribute("data-theme") : null,
  );
  const modelUri = `inmemory://cassie/query/${encodeURIComponent(tabId)}.sql`;
  const resource = getEditorResource(modelUri);
  resource.completionProvider = completionProvider ?? emptyCompletionItems;
  const isEditorUnavailable = editorUnavailable();
  const followsSystemDark = systemDark();
  const systemThemeQuery =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-color-scheme: dark)")
      : null;
  on(
    () => systemThemeQuery,
    "change",
    (event) => {
      if ("matches" in event) setSystemDark(Boolean(event.matches));
    },
  );
  task(() => {
    if (typeof document === "undefined" || typeof MutationObserver === "undefined") return;
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      setExplicitTheme(root.getAttribute("data-theme"));
    });
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  });

  function setEditorHost(node: unknown) {
    resource.host = node instanceof HTMLElement ? node : null;
  }

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

    input.value = next;
    onChange(next);
    raf(() => {
      input.selectionStart = cursor;
      input.selectionEnd = cursor;
    })();
  }

  // jsdom can't run Monaco (no real Worker/Canvas/ResizeObserver support), so
  // tests always exercise this same plain-textarea contract instead — real
  // browsers only fall back here if Monaco itself fails to load (onError).
  if (isTestMode || typeof window === "undefined" || isEditorUnavailable) {
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
          ref={(node: unknown) => {
            resource.fallback = node instanceof HTMLTextAreaElement ? node : null;
          }}
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
        />
      </div>
    );
  }

  function handleBeforeMount(monaco: MonacoNamespace) {
    resource.monaco = monaco;
    resource.completionDisposable?.dispose();
    resource.completionDisposable = monaco.languages.registerCompletionItemProvider("sql", {
      provideCompletionItems: (model, position) => {
        if (!active() || model.uri.toString() !== modelUri) return { suggestions: [] };
        const word = model.getWordUntilPosition(position);
        const range = {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endColumn: word.endColumn,
        };

        const offset = model.getOffsetAt(position);
        return {
          suggestions: resource
            .completionProvider({ sql: model.getValue(), offset, word: word.word })
            .map((item) => ({
              label: item.label,
              insertText: item.insertText,
              filterText: item.filterText,
              sortText: item.sortText,
              detail: item.detail,
              documentation: item.detail ?? "",
              kind: monaco.languages.CompletionItemKind[
                item.kind === "keyword"
                  ? "Keyword"
                  : item.kind === "field"
                    ? "Field"
                    : item.kind === "function"
                      ? "Function"
                      : item.kind === "method"
                        ? "Method"
                        : item.kind === "reference"
                          ? "Reference"
                          : "Class"
              ],
              range,
            })),
        };
      },
    });
  }

  function handleMount(editor: MonacoEditorInstance) {
    resource.editor = editor;
    resource.changeDisposable?.dispose();
    resource.changeDisposable = editor.onDidChangeModelContent(() => {
      onChange(editor.getValue());
    });
    const host = resource.host;
    if (host && typeof ResizeObserver !== "undefined") {
      resource.layoutObserver?.disconnect();
      resource.layoutObserver = observeEditorLayout(host, () => resource.editor?.layout());
    }
  }

  function handleUnmount() {
    resource.editor = null;
    resource.changeDisposable?.dispose();
    resource.changeDisposable = null;
    resource.completionDisposable?.dispose();
    resource.completionDisposable = null;
    resource.layoutObserver?.disconnect();
    resource.layoutObserver = null;
    resource.fallback = null;
    resource.host = null;
    resource.monaco = null;
    editorResources.delete(modelUri);
  }

  const selectedTheme = explicitTheme();
  const monacoTheme =
    selectedTheme === "dark" || (!selectedTheme && followsSystemDark) ? "vs-dark" : "vs";

  function handleEditorKeyDown(event: KeyboardEvent) {
    if (!event.metaKey && !event.ctrlKey) return;
    const editor = resource.editor;
    if (!editor) return;
    const key = event.key.toLowerCase();
    if (key === "a") {
      const model = editor.getModel();
      if (!model) return;
      event.preventDefault();
      editor.setSelection(model.getFullModelRange());
      editor.focus();
      return;
    }
    if (key === "z") {
      event.preventDefault();
      editor.trigger("cassie-keyboard", event.shiftKey ? "redo" : "undo", null);
      editor.focus();
    }
  }

  return (
    <div
      class="cassie-query-editor-host"
      data-testid="query-editor"
      data-query-editor="monaco"
      aria-label="SQL editor"
      onKeyDownCapture={handleEditorKeyDown}
      ref={setEditorHost}
    >
      <MonacoEditor
        path={modelUri}
        value={value}
        language="sql"
        theme={monacoTheme}
        options={{
          readOnly: disabled,
          automaticLayout: false,
          minimap: { enabled: false },
          fontSize: 13,
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          renderWhitespace: "none",
          wordWrap: "on",
        }}
        beforeMount={handleBeforeMount}
        onMount={handleMount}
        onUnmount={handleUnmount}
        onError={() => setEditorUnavailable(true)}
        aria-label="SQL editor"
      />
    </div>
  );
}
