import { state } from "@askrjs/askr";

import { MonacoEditor, type MonacoEditorInstance, type MonacoNamespace } from "@askrjs/monaco";
import { theme } from "@askrjs/themes/theme";
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

interface MonacoProviderRegistry {
  owners: Map<string, (context: MonacoCompletionContext) => MonacoCompletionItem[]>;
  activeUri: string | null;
  disposable: { dispose(): void };
}

const providerRegistries = new WeakMap<object, MonacoProviderRegistry>();
interface MonacoEditorResource {
  monaco: object | null;
  editor: MonacoEditorInstance | null;
  changeDisposable: { dispose(): void } | null;
  systemThemeQuery: MediaQueryList | null;
  systemThemeListener: ((event: MediaQueryListEvent) => void) | null;
  layoutObserver: EditorLayoutObserver | null;
}

const editorResources = new Map<string, MonacoEditorResource>();

function getEditorResource(modelUri: string) {
  let resource = editorResources.get(modelUri);
  if (!resource) {
    resource = {
      monaco: null,
      editor: null,
      changeDisposable: null,
      systemThemeQuery: null,
      systemThemeListener: null,
      layoutObserver: null,
    };
    editorResources.set(modelUri, resource);
  }

  return resource;
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
  const themeScope = theme();
  const modelUri = `inmemory://cassie/query/${encodeURIComponent(tabId)}.sql`;
  const resource = getEditorResource(modelUri);
  const latestCompletionProvider = completionProvider ?? emptyCompletionItems;
  const isEditorUnavailable = editorUnavailable();
  const followsSystemDark = systemDark();
  let editorHost: HTMLElement | null = null;

  function setEditorHost(node: unknown) {
    editorHost = node instanceof HTMLElement ? node : null;
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
    requestAnimationFrame(() => {
      input.selectionStart = cursor;
      input.selectionEnd = cursor;
    });
  }

  // jsdom can't run Monaco (no real Worker/Canvas/ResizeObserver support), so
  // tests always exercise this same plain-textarea contract instead — real
  // browsers only fall back here if Monaco itself fails to load (onError).
  if (resource.monaco) {
    const registry = providerRegistries.get(resource.monaco);
    if (registry) {
      registry.owners.set(modelUri, latestCompletionProvider);
      if (active()) registry.activeUri = modelUri;
    }
  }

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
    resource.monaco = monaco as object;
    let registry = providerRegistries.get(resource.monaco);
    if (!registry) {
      const owners = new Map<
        string,
        (context: MonacoCompletionContext) => MonacoCompletionItem[]
      >();
      registry = {
        owners,
        activeUri: null,
        disposable: monaco.languages.registerCompletionItemProvider("sql", {
          provideCompletionItems: (model, position) => {
            const current = providerRegistries.get(monaco as object);
            const uri = model.uri.toString();
            if (!current || current.activeUri !== uri) return { suggestions: [] };
            const word = model.getWordUntilPosition(position);
            const range = {
              startLineNumber: position.lineNumber,
              endLineNumber: position.lineNumber,
              startColumn: word.startColumn,
              endColumn: word.endColumn,
            };

            const offset = model.getOffsetAt(position);
            return {
              suggestions: (
                current.owners.get(uri)?.({ sql: model.getValue(), offset, word: word.word }) ?? []
              ).map((item) => ({
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
        }),
      };
      providerRegistries.set(monaco as object, registry);
    }
    registry.owners.set(modelUri, latestCompletionProvider);
    if (active()) {
      registry.activeUri = modelUri;
    }
  }

  function handleMount(editor: MonacoEditorInstance) {
    resource.editor = editor;
    resource.changeDisposable?.dispose();
    resource.changeDisposable = editor.onDidChangeModelContent(() => {
      onChange(editor.getValue());
    });
    const registry = resource.monaco ? providerRegistries.get(resource.monaco) : null;
    if (active() && registry) {
      registry.activeUri = modelUri;
    }
    if (typeof window.matchMedia === "function") {
      resource.systemThemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
      resource.systemThemeListener = (event) => setSystemDark(event.matches);
      resource.systemThemeQuery.addEventListener("change", resource.systemThemeListener);
    }
    if (editorHost && typeof ResizeObserver !== "undefined") {
      resource.layoutObserver?.disconnect();
      resource.layoutObserver = observeEditorLayout(editorHost, () => resource.editor?.layout());
    }
  }

  function handleUnmount() {
    const registry = resource.monaco ? providerRegistries.get(resource.monaco) : null;
    resource.editor?.dispose();
    resource.editor = null;
    if (resource.systemThemeQuery && resource.systemThemeListener) {
      resource.systemThemeQuery.removeEventListener("change", resource.systemThemeListener);
    }
    resource.systemThemeQuery = null;
    resource.systemThemeListener = null;
    resource.changeDisposable?.dispose();
    resource.changeDisposable = null;
    resource.layoutObserver?.disconnect();
    resource.layoutObserver = null;
    registry?.owners.delete(modelUri);
    if (registry?.activeUri === modelUri) {
      registry.activeUri = null;
    }
    if (registry?.owners.size === 0) {
      registry.disposable.dispose();
      if (resource.monaco) {
        providerRegistries.delete(resource.monaco);
      }
    }
    resource.monaco = null;
    editorResources.delete(modelUri);
  }

  const selectedTheme = themeScope.theme();
  const monacoTheme =
    selectedTheme === "dark" || (selectedTheme === "system" && followsSystemDark)
      ? "vs-dark"
      : "vs";

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
