import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import { SendIcon, TriangleAlertIcon } from "@askrjs/lucide";
import { Alert, Button, Text } from "@askrjs/themes/components";

import { QueryEditorPanel } from "@/components/query/query-editor-panel";
import { QueryResultTab, QueryResultsTabs } from "@/components/query/query-results-tabs";
import { QuerySchemaItem, QuerySchemaSection, QueryExecutionResult, QueryValidationResult } from "@/features/query/query-models";
import { createAdminQuerySchemaQuery } from "@/features/query/query-query";
import { createExecuteQueryMutation, createExplainQueryMutation, createValidateQueryMutation } from "@/features/query/query-actions";
import { QuerySchemaTree } from "@/components/query/query-schema-tree";
import { ResizableSplit } from "@/components/query/resizable-split";
import { apiErrorMessage } from "@/shared/errors/api";

type QueryStatus = "idle" | "running" | "explaining" | "validating";

const defaultQuery = "SELECT id, name\nFROM documents\nLIMIT 10;";

function flattenCompletionItems(schema: QuerySchemaSection[]) {
  return schema.flatMap((section) =>
    section.items.map((item) => ({
      label: item.label,
      insertText: item.label,
      detail: `${item.kind}${item.metadata ? ` · ${item.metadata}` : ""}`,
    })),
  );
}

function QueryPlaceholder({ title, description }: { title: string; description: string }) {
  return (
    <section class="cassie-query-results-placeholder" aria-label={title}>
      <h3>{title}</h3>
      <p>{description}</p>
    </section>
  );
}

function QueryResultTable({ result }: { result: QueryExecutionResult }) {
  if (result.columns.length === 0) {
    return <QueryPlaceholder title="No columns" description="This query returned no columns." />;
  }

  return (
    <div class="cassie-query-result-table-wrap">
      <table class="cassie-query-result-table">
        <thead>
          <tr>
            <For each={result.columns} by={(_, index) => index}>
              {(column) => <th>{column}</th>}
            </For>
          </tr>
        </thead>
        <tbody>
          {result.rows.length === 0 ? (
            <tr>
              <td colSpan={result.columns.length} class="cassie-query-empty-result-cell">
                <QueryPlaceholder title="No rows" description="The query returned zero rows." />
              </td>
            </tr>
          ) : null}
          <For each={result.rows} by={(_, index) => index}>
            {(row) => (
              <tr>
                <For each={row} by={(_, index) => index}>
                  {(value) => <td>{value}</td>}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}

function QueryResultJson({ result }: { result: QueryExecutionResult }) {
  return (
    <pre class="cassie-query-json">
      <code>{JSON.stringify({ command: result.command, columns: result.columns, rows: result.rows }, null, 2)}</code>
    </pre>
  );
}

function QueryValidationBanner({ validation }: { validation: QueryValidationResult | null }) {
  if (!validation) {
    return null;
  }

  return (
    <Alert
      variant={validation.valid ? "success" : "warning"}
      title={validation.valid ? "Validation passed" : "Validation failed"}
      description={`Command ${validation.command}`}
    />
  );
}

export default function QueryPage() {
  const schemaQuery = createAdminQuerySchemaQuery();
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();

  const [query, setQuery] = state(defaultQuery);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [status, setStatus] = state<QueryStatus>("idle");
  const [schemaWidth, setSchemaWidth] = state(30);
  const [editorHeight, setEditorHeight] = state(62);

  const schemaSections = schemaQuery.data?.sections ?? [];
  const completionItems = flattenCompletionItems(schemaSections);
  const canRun = query().trim().length > 0 && status() === "idle";

  const isExecutionBusy = executeMutation.pending || explainMutation.pending;
  const activeExecution =
    activeTab() === "plan" ? explainMutation.result : executeMutation.result;
  const validationResult = validateMutation.result;

  async function handleSchemaSelection(item: QuerySchemaItem) {
    setSelectedItemId(item.id);
    setQuery(`SELECT * FROM ${item.label};`);
  }

  function handleFormatQuery() {
    setQuery(query().trim());
  }

  async function runValidate() {
    if (!canRun) {
      return;
    }

    setStatus("validating");
    validateMutation.reset();
    try {
      await validateMutation.execute({ sql: query() });
    } finally {
      setStatus("idle");
    }
  }

  async function runExplain() {
    if (!canRun) {
      return;
    }

    setStatus("explaining");
    explainMutation.reset();
    setActiveTab("plan");
    try {
      await explainMutation.execute({ sql: query() });
    } finally {
      setStatus("idle");
    }
  }

  async function runExecute() {
    if (!canRun) {
      return;
    }

    setStatus("running");
    executeMutation.reset();
    setActiveTab("results");
    try {
      await executeMutation.execute({ sql: query() });
    } finally {
      setStatus("idle");
    }
  }

  function stopAction() {
    executeMutation.abort();
    validateMutation.abort();
    explainMutation.abort();
    setStatus("idle");
  }

  function handlePlay() {
    void runExecute();
  }

  function handleValidate() {
    void runValidate();
  }

  function handleExplain() {
    void runExplain();
  }

  function handleTabChange(tab: QueryResultTab) {
    setActiveTab(tab);
  }

  const hasActionError =
    validateMutation.error !== null || executeMutation.error !== null || explainMutation.error !== null;

  const statusBanner = isExecutionBusy || status() !== "idle" ?
    status() === "running"
      ? "Running query..."
      : status() === "explaining"
        ? "Generating explain plan..."
        : status() === "validating"
          ? "Validating SQL..."
          : null
    : null;

  return (
    <main
      class="cassie-query-page cassie-query-shell"
      data-slot="main"
      data-query-page="true"
      id="main-content"
      tabindex={-1}
      aria-labelledby="cassie-admin-page-title"
    >
      <section class="cassie-admin-page-header" aria-label="Query page">
        <div class="cassie-admin-page-icon" aria-hidden="true">
          <SendIcon size={20} />
        </div>
        <div class="cassie-admin-page-title-group">
          <Text
            as="p"
            class="cassie-admin-page-kicker"
            size="sm"
            weight="semibold"
            transform="uppercase"
          >
            Cassie
          </Text>
          <h1 id="cassie-admin-page-title">Query</h1>
          <p>SQL and document query workflows.</p>
        </div>
      </section>

      {schemaQuery.loading && !schemaQuery.data ? <p class="cassie-query-loading">Loading query schema…</p> : null}

      {schemaQuery.error && !schemaQuery.data ? (
        <Alert
          title="Unable to load query schema"
          variant="danger"
          description={apiErrorMessage(schemaQuery.error)}
          icon={<TriangleAlertIcon size={16} />}
        />
      ) : null}

      <section class="cassie-query-workspace" aria-label="Query workspace">
        <ResizableSplit
          orientation="horizontal"
          initialSize={schemaWidth()}
          min={18}
          max={40}
          onResize={(size) => setSchemaWidth(size)}
          first={
            <QuerySchemaTree
              schema={schemaSections}
              selectedItemId={selectedItemId() ?? undefined}
              onSelectItem={handleSchemaSelection}
            />
          }
          second={
            <ResizableSplit
              orientation="vertical"
              initialSize={editorHeight()}
              min={30}
              max={80}
              onResize={(size) => setEditorHeight(size)}
              first={
                <QueryEditorPanel
                  query={query()}
                  onQueryChange={setQuery}
                  isRunning={isExecutionBusy || status() !== "idle"}
                  onFormat={handleFormatQuery}
                  onValidate={handleValidate}
                  onExplain={handleExplain}
                  onPlay={handlePlay}
                  onStop={stopAction}
                  completionItems={completionItems}
                />
              }
              second={
                <>
                  {(statusBanner || hasActionError) ? (
                    <Alert
                      title="Query action"
                      variant={hasActionError ? "danger" : "info"}
                      description={
                        hasActionError
                          ? validateMutation.error
                            ? apiErrorMessage(validateMutation.error)
                            : executeMutation.error
                              ? apiErrorMessage(executeMutation.error)
                              : explainMutation.error
                                ? apiErrorMessage(explainMutation.error)
                                : "Unknown query action error."
                          : statusBanner ?? "Working..."
                      }
                    />
                  ) : null}

                  <QueryValidationBanner validation={validationResult ?? null} />

                  <QueryResultsTabs
                    activeTab={activeTab()}
                    onTabChange={handleTabChange}
                    resultsContent={
                      activeExecution ? (
                        <QueryResultTable result={activeExecution} />
                      ) : (
                        <QueryPlaceholder title="No rows" description="No query has run yet." />
                      )
                    }
                    listContent={
                      activeExecution ? (
                        <>
                          <div class="cassie-query-results-header">
                            <strong>Result set</strong>
                            <span>{activeExecution.rows.length} rows</span>
                          </div>
                          <QueryResultJson result={activeExecution} />
                        </>
                      ) : (
                        <QueryPlaceholder title="No rows" description="No query has run yet." />
                      )
                    }
                    planContent={
                      activeExecution ? <QueryResultJson result={activeExecution} /> : <QueryPlaceholder title="No plan" description="Run explain to inspect plan rows." />
                    }
                  />

                  {schemaQuery.error ? (
                    <Button
                      type="button"
                      size="sm"
                      onPress={() => {
                        void schemaQuery.refresh();
                      }}
                    >
                      Retry schema
                    </Button>
                  ) : null}

                  {canRun ? null : (
                    <p class="cassie-query-run-note">Type SQL to enable run, validate, and explain actions.</p>
                  )}
                </>
              }
            />
          }
        />
      </section>
    </main>
  );
}
