import { state } from "@askrjs/askr";
import { Portal } from "@askrjs/askr/foundations";
import { CommandIcon, TriangleAlertIcon } from "@askrjs/lucide";
import { Alert, Badge, Button, Kbd, PageHeader, Text } from "@askrjs/themes/components";

import { QueryEditorPanel } from "@/components/query/query-editor-panel";
import { QueryExecutionBanner } from "@/components/query/query-execution-banner";
import { QueryExecutionSummary } from "@/components/query/query-execution-summary";
import { QueryPlaceholder } from "@/components/query/query-placeholder";
import { QueryPlanText } from "@/components/query/query-plan-text";
import { QueryResultJson } from "@/components/query/query-result-json";
import { QueryResultTable } from "@/components/query/query-result-table";
import { QueryResultTab, QueryResultsTabs } from "@/components/query/query-results-tabs";
import { QuerySchemaTree } from "@/components/query/query-schema-tree";
import type { QueryValidationToastData } from "@/components/query/query-validation-toast";
import { QueryValidationToast } from "@/components/query/query-validation-toast";
import { ResizableSplit } from "@/components/query/resizable-split";
import { flattenCompletionItems } from "@/features/query/query-mappers";
import { QuerySchemaItem, QueryStatus } from "@/features/query/query-models";
import { createAdminQuerySchemaQuery } from "@/features/query/query-query";
import {
  createExecuteQueryMutation,
  createExplainQueryMutation,
  createValidateQueryMutation,
} from "@/features/query/query-actions";
import { apiErrorMessage } from "@/shared/errors/api";

const defaultQuery = "SELECT 1 AS ready;";

const schemaChangingCommands = new Set([
  "ALTER SCHEMA",
  "ALTER TABLE",
  "CREATE DATABASE",
  "CREATE FUNCTION",
  "CREATE INDEX",
  "CREATE PROCEDURE",
  "CREATE SCHEMA",
  "CREATE TABLE",
  "CREATE VIEW",
  "DROP DATABASE",
  "DROP FUNCTION",
  "DROP INDEX",
  "DROP PROCEDURE",
  "DROP SCHEMA",
  "DROP TABLE",
  "DROP VIEW",
]);

export default function QueryPage() {
  const schemaQuery = createAdminQuerySchemaQuery();
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();

  const [query, setQuery] = state(defaultQuery);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [status, setStatus] = state<QueryStatus>("idle");
  const [editorHeight, setEditorHeight] = state(62);
  const [validationToast, setValidationToast] = state<QueryValidationToastData | null>(null);

  // Plain closure state, not state() — this timer id is never rendered, only
  // read inside the handlers below, so making it reactive would just force a
  // needless extra re-render on every show/dismiss.
  let validationToastTimer: ReturnType<typeof setTimeout> | null = null;
  let isValidationToastSuspended = false;

  function clearValidationToastTimer() {
    if (validationToastTimer !== null) {
      clearTimeout(validationToastTimer);
      validationToastTimer = null;
    }
  }

  function scheduleValidationToastDismiss() {
    clearValidationToastTimer();
    if (isValidationToastSuspended) {
      return;
    }
    validationToastTimer = setTimeout(() => {
      validationToastTimer = null;
      setValidationToast(null);
    }, 4000);
  }

  function showValidationToast(toast: QueryValidationToastData) {
    setValidationToast(toast);
    scheduleValidationToastDismiss();
  }

  function dismissValidationToast() {
    clearValidationToastTimer();
    setValidationToast(null);
  }

  // Pause the auto-dismiss while the toast is hovered or has focus (e.g. on
  // its dismiss button) so it can't disappear out from under a user who's
  // still reading or interacting with it, then resume the countdown once
  // they leave — otherwise a screen-reader user tabbing to the dismiss
  // button could have the toast vanish mid-interaction.
  function suspendValidationToastTimer() {
    isValidationToastSuspended = true;
    clearValidationToastTimer();
  }

  function resumeValidationToastTimer() {
    isValidationToastSuspended = false;
    if (validationToast() !== null) {
      scheduleValidationToastDismiss();
    }
  }

  // askr has no onUnmount/onCleanup hook; the established pattern in this
  // codebase (see monaco-sql-editor.tsx's mountEditor) is a ref callback that
  // fires with null on unmount. Used here only to stop a pending toast timer
  // from firing against a torn-down page.
  function handleMainRef(node: HTMLElement | null) {
    if (node === null) {
      clearValidationToastTimer();
    }
  }

  const getSchemaDatabases = () => schemaQuery.data?.databases ?? [];
  const getCompletionItems = () => flattenCompletionItems(getSchemaDatabases());
  const hasQuery = query().trim().length > 0;

  const isExecutionBusy = executeMutation.pending || explainMutation.pending;
  const isValidating = validateMutation.pending || status() === "validating";
  const isQueryBusy = status() !== "idle" || isExecutionBusy || isValidating;
  const activeExecution = activeTab() === "plan" ? explainMutation.result : executeMutation.result;
  const canRun = hasQuery && !isQueryBusy;

  const actionErrorMessage = (() => {
    if (executeMutation.error !== null) {
      return apiErrorMessage(executeMutation.error);
    }
    if (explainMutation.error !== null) {
      return apiErrorMessage(explainMutation.error);
    }

    return null;
  })();

  function resetQueryFeedback() {
    if (executeMutation.pending) {
      executeMutation.abort();
    }
    if (validateMutation.pending) {
      validateMutation.abort();
    }
    if (explainMutation.pending) {
      explainMutation.abort();
    }

    executeMutation.reset();
    validateMutation.reset();
    explainMutation.reset();
    setStatus("idle");
    dismissValidationToast();
  }

  function handleQueryChange(nextQuery: string) {
    if (nextQuery === query()) {
      return;
    }

    resetQueryFeedback();
    setQuery(nextQuery);
  }

  function handleSchemaSelection(item: QuerySchemaItem) {
    setSelectedItemId(item.id);
  }

  function handleTrimQuery() {
    handleQueryChange(query().trim());
  }

  async function runValidate() {
    if (!canRun) {
      return;
    }

    setStatus("validating");
    validateMutation.reset();
    try {
      await validateMutation.execute({ sql: query() });
      if (validateMutation.result) {
        const result = validateMutation.result;
        showValidationToast({
          variant: result.valid ? "success" : "warning",
          title: result.valid ? "Validation passed" : "Validation failed",
          description: `Command ${result.command}`,
        });
      }
    } catch {
      // MutationCell.execute() always rethrows on failure (including an
      // intentional abort via the Stop button), so a failed/aborted request
      // never reaches the success branch above — it lands here instead.
      // validateMutation.error is only non-null for a genuine failure (an
      // abort leaves it null), which is what distinguishes "show an error
      // toast" from "the user cancelled, stay silent" below.
      if (validateMutation.error !== null) {
        showValidationToast({
          variant: "danger",
          title: "Validation failed",
          description: apiErrorMessage(validateMutation.error),
        });
      }
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
      if (
        executeMutation.result !== null &&
        schemaChangingCommands.has(executeMutation.result.command)
      ) {
        await schemaQuery.refresh();
      }
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

  return (
    <>
      <Portal>
        <QuerySchemaTree
          schema={getSchemaDatabases}
          selectedItemId={() => selectedItemId() ?? undefined}
          onSelectItem={handleSchemaSelection}
        />
      </Portal>

      <QueryValidationToast
        toast={validationToast()}
        onDismiss={dismissValidationToast}
        onPause={suspendValidationToastTimer}
        onResume={resumeValidationToastTimer}
      />

      <main
        class="cassie-query-page cassie-query-shell"
        data-slot="main"
        data-query-page="true"
        id="main-content"
        tabindex={-1}
        aria-labelledby="query-workspace-title"
        ref={handleMainRef}
      >
        <PageHeader
          title={<span id="query-workspace-title">Query workspace</span>}
          description="Explore the catalog, shape a query, then inspect rows and execution plans."
          actions={
            <Badge variant="outline" data-testid="query-run-shortcut" title="Run query">
              <CommandIcon size={14} aria-hidden="true" />
              <Text as="span" size="sm" tone="muted">
                Run
              </Text>
              <Kbd>⌘ / Ctrl</Kbd>
              <span>+</span>
              <Kbd>Enter</Kbd>
            </Badge>
          }
        />

        {schemaQuery.loading && !schemaQuery.data ? (
          <p class="cassie-query-loading">Loading query schema…</p>
        ) : null}

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
            orientation="vertical"
            initialSize={editorHeight()}
            min={30}
            max={80}
            onResize={(size) => setEditorHeight(size)}
            first={
              <QueryEditorPanel
                query={query()}
                onQueryChange={handleQueryChange}
                isRunning={isQueryBusy}
                onTrim={handleTrimQuery}
                onValidate={handleValidate}
                onExplain={handleExplain}
                onPlay={handlePlay}
                onStop={stopAction}
                completionItems={getCompletionItems}
              />
            }
            second={
              <>
                <QueryExecutionBanner
                  status={status()}
                  isBusy={isQueryBusy}
                  errorMessage={actionErrorMessage}
                />

                <QueryExecutionSummary result={activeExecution} />

                <QueryResultsTabs
                  activeTab={activeTab}
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
                        <QueryResultJson result={activeExecution} />
                      </>
                    ) : (
                      <QueryPlaceholder title="No rows" description="No query has run yet." />
                    )
                  }
                  planContent={
                    activeExecution ? (
                      <QueryPlanText result={activeExecution} />
                    ) : (
                      <QueryPlaceholder
                        title="No plan"
                        description="Run explain to inspect plan rows."
                      />
                    )
                  }
                />

                {schemaQuery.error ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onPress={() => {
                      void schemaQuery.refresh();
                    }}
                  >
                    Retry schema
                  </Button>
                ) : null}

                {hasQuery ? null : (
                  <p class="cassie-query-run-note">
                    Type SQL to enable run, validate, and explain actions.
                  </p>
                )}
              </>
            }
          />
        </section>
      </main>
    </>
  );
}
