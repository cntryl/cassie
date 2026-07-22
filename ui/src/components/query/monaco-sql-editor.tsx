import { state } from "@askrjs/askr";

import { MonacoEditor, type MonacoEditorInstance, type MonacoNamespace } from "@askrjs/monaco";
import { theme } from "@askrjs/themes/theme";

export interface MonacoCompletionItem {
  label: string;
  detail?: string;
  insertText: string;
}

export interface MonacoSqlEditorProps {
  tabId: string;
  active: () => boolean;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  completionProvider?: () => MonacoCompletionItem[];
}

interface MonacoProviderRegistry {
  owners: Map<string, () => MonacoCompletionItem[]>;
  activeUri: string | null;
  disposable: { dispose(): void };
}

const providerRegistries = new WeakMap<object, MonacoProviderRegistry>();

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
  let providerRegistry: MonacoProviderRegistry | null = null;
  let monacoKey: object | null = null;
  let changeDisposable: { dispose(): void } | null = null;
  let systemThemeQuery: MediaQueryList | null = null;
  let systemThemeListener: ((event: MediaQueryListEvent) => void) | null = null;
  const latestCompletionProvider = completionProvider ?? emptyCompletionItems;
  const isEditorUnavailable = editorUnavailable();
  const followsSystemDark = systemDark();

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
    monacoKey = monaco as object;
    let registry = providerRegistries.get(monaco as object);
    if (!registry) {
      const owners = new Map<string, () => MonacoCompletionItem[]>();
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

            return {
              suggestions: (current.owners.get(uri)?.() ?? []).map((item) => ({
                label: item.label,
                insertText: item.insertText,
                detail: item.detail,
                documentation: item.detail ?? "",
                kind: monaco.languages.CompletionItemKind.Snippet,
                range,
              })),
            };
          },
        }),
      };
      providerRegistries.set(monaco as object, registry);
    }
    registry.owners.set(modelUri, latestCompletionProvider);
    if (active()) registry.activeUri = modelUri;
    providerRegistry = registry;
  }

  function handleMount(editor: MonacoEditorInstance) {
    changeDisposable?.dispose();
    changeDisposable = editor.onDidChangeModelContent(() => {
      onChange(editor.getValue());
    });
    if (active() && providerRegistry) providerRegistry.activeUri = modelUri;
    if (typeof window.matchMedia === "function") {
      systemThemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
      systemThemeListener = (event) => setSystemDark(event.matches);
      systemThemeQuery.addEventListener("change", systemThemeListener);
    }
  }

  function handleUnmount() {
    providerRegistry?.owners.delete(modelUri);
    if (providerRegistry?.activeUri === modelUri) providerRegistry.activeUri = null;
    if (providerRegistry?.owners.size === 0) {
      providerRegistry.disposable.dispose();
      if (monacoKey) providerRegistries.delete(monacoKey);
    }
    providerRegistry = null;
    monacoKey = null;
    changeDisposable?.dispose();
    changeDisposable = null;
    if (systemThemeQuery && systemThemeListener) {
      systemThemeQuery.removeEventListener("change", systemThemeListener);
    }
    systemThemeQuery = null;
    systemThemeListener = null;
  }

  const selectedTheme = themeScope.theme();
  const monacoTheme =
    selectedTheme === "dark" || (selectedTheme === "system" && followsSystemDark)
      ? "vs-dark"
      : "vs";

  return (
    <div
      class="cassie-query-editor-host"
      data-testid="query-editor"
      data-query-editor="monaco"
      aria-label="SQL editor"
    >
      <MonacoEditor
        path={modelUri}
        value={value}
        language="sql"
        theme={monacoTheme}
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
        onUnmount={handleUnmount}
        onError={() => setEditorUnavailable(true)}
        aria-label="SQL editor"
      />
    </div>
  );
}
