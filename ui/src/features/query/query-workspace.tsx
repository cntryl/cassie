import { state } from "@askrjs/askr";
import { task } from "@askrjs/askr/resources";

import { QueryEditorPanel } from "@/components/query/query-editor-panel";
import { QueryAvailabilityStatus } from "@/components/query/query-availability-status";
import { QueryExecutionBanner } from "@/components/query/query-execution-banner";
import { QueryExecutionSummary } from "@/components/query/query-execution-summary";
import { setMonacoSqlEditorValue } from "@/components/query/monaco-sql-editor";
import { QueryPlaceholder } from "@/components/query/query-placeholder";
import { QueryPlanText } from "@/components/query/query-plan-text";
import { QueryResultJson } from "@/components/query/query-result-json";
import { QueryResultTable } from "@/components/query/query-result-table";
import { QueryResultTab, QueryResultsTabs } from "@/components/query/query-results-tabs";
import type { QueryValidationToastData } from "@/components/query/query-validation-toast";
import { QueryValidationToast } from "@/components/query/query-validation-toast";
import { ResizableSplit } from "@/components/query/resizable-split";
import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import { buildSqlCompletionItems } from "@/features/query/query-completions";
import {
  createCancelQueryMutation,
  createExecuteQueryMutation,
  createExplainQueryMutation,
  createValidateQueryMutation,
} from "@/features/query/query-actions";
import {
  type QueryExecutionResult,
  type QuerySchemaDatabase,
  type QueryStatus,
} from "@/features/query/query-models";
import type { PersistedQueryTab } from "@/features/query/query-tabs";
import { apiErrorMessage, AppApiError } from "@/shared/errors/api";

const schemaChangingCommandPrefixes = ["ALTER ", "CREATE ", "DROP "];

function changesSchema(command: string) {
  return schemaChangingCommandPrefixes.some((prefix) => command.startsWith(prefix));
}

export type DatabaseAvailability = "checking" | "available" | "unavailable" | "discovery-error";

interface ActiveQueryOperation {
  id: string;
  abort: () => void;
}

export interface QueryWorkspaceProps {
  tab: PersistedQueryTab;
  active: () => boolean;
  availability: () => DatabaseAvailability;
  onSqlChange: (sql: string) => void;
  onEditorSplitChange: (percent: number) => void;
  onRegister: (controller: { isBusy: () => boolean; cancel: () => Promise<void> }) => () => void;
  onCreateDatabase: () => void;
  catalogs: () => DatabaseCatalogEntry[];
  onSchemaChanged: (database: string) => Promise<void>;
}

function QueryRunNote({ query }: { query: () => string }) {
  return query().trim().length > 0 ? null : (
    <p class="cassie-query-run-note">Type SQL to enable run, validate, and explain actions.</p>
  );
}

export function QueryWorkspace({
  tab,
  active,
  availability,
  onSqlChange,
  onEditorSplitChange,
  onRegister,
  onCreateDatabase,
  catalogs,
  onSchemaChanged,
}: QueryWorkspaceProps) {
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();
  const cancelMutation = createCancelQueryMutation();

  const [queryDraft, setQueryDraft] = state(tab.sql);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [status, setStatus] = state<QueryStatus>("idle");
  const [editorHeight, setEditorHeight] = state(tab.editorSplitPercent ?? 52);
  const [validationToast, setValidationToast] = state<QueryValidationToastData | null>(null);
  const [stopError, setStopError] = state<string | null>(null);
  const [stopPending, setStopPending] = state(false);
  const [executionResult, setExecutionResult] = state<QueryExecutionResult | null>(null);
  const [planResult, setPlanResult] = state<QueryExecutionResult | null>(null);
  const [activeOperation, setActiveOperation] = state<ActiveQueryOperation | null>(null);

  function showValidationToast(toast: QueryValidationToastData) {
    setValidationToast(toast);
  }

  function dismissValidationToast() {
    setValidationToast(null);
  }

  const getSchemaDatabases = () => {
    return catalogs().flatMap((catalog): QuerySchemaDatabase[] =>
      catalog.status === "loaded" && catalog.database ? [catalog.database] : [],
    );
  };
  const getCompletionItems = (context: Parameters<typeof buildSqlCompletionItems>[2]) =>
    buildSqlCompletionItems(getSchemaDatabases(), tab.database, context);
  const currentQuery = () => queryDraft();

  const isExecutionBusy = executeMutation.pending || explainMutation.pending;
  const isValidating = validateMutation.pending || status() === "validating";
  const isQueryBusy = status() !== "idle" || isExecutionBusy || isValidating;
  const canRun = () =>
    currentQuery().trim().length > 0 && !isQueryBusy && availability() === "available";
  validationToast();
  const currentStopError = stopError();
  const actionError = activeTab() === "plan" ? explainMutation.error : executeMutation.error;
  const actionErrorMessage =
    status() === "stopping" || actionError === null
      ? currentStopError
      : apiErrorMessage(actionError);

  function beginOperation(nextStatus: QueryStatus, abort: () => void) {
    const operationId = crypto.randomUUID();
    setActiveOperation({ id: operationId, abort });
    setStopError(null);
    setStatus(nextStatus);
    return operationId;
  }

  function finishOperation(operationId: string) {
    if (activeOperation()?.id !== operationId || stopError() !== null) {
      return;
    }
    setActiveOperation(null);
    setStatus("idle");
  }

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

    setExecutionResult(null);
    setPlanResult(null);
    executeMutation.reset();
    validateMutation.reset();
    explainMutation.reset();
    setStatus("idle");
    dismissValidationToast();
  }

  function handleQueryChange(nextQuery: string) {
    if (nextQuery === currentQuery()) {
      return;
    }

    setQueryDraft(nextQuery);
    onSqlChange(nextQuery);
  }

  function handleTrimQuery() {
    const trimmed = currentQuery().trim();
    if (trimmed === currentQuery()) return;
    setQueryDraft(trimmed);
    setMonacoSqlEditorValue(tab.id, trimmed);
    resetQueryFeedback();
    onSqlChange(trimmed);
  }

  async function runValidate() {
    if (!canRun()) {
      return;
    }

    const operationId = beginOperation("validating", () => validateMutation.abort());
    validateMutation.reset();
    try {
      await validateMutation.execute({ database: tab.database, sql: currentQuery(), operationId });
      if (validateMutation.result) {
        const result = validateMutation.result;
        showValidationToast({
          variant: result.valid ? "success" : "warning",
          title: result.valid ? "Validation passed" : "Validation failed",
          description: result.command,
        });
      }
    } catch {
      // MutationCell.execute() always rethrows on failure (including an
      // intentional abort via the Stop button), so a failed/aborted request
      // never reaches the success branch above — it lands here instead.
      // validateMutation.error is only non-null for a genuine failure (an
      // abort leaves it null), which is what distinguishes "show an error
      // toast" from "the user cancelled, stay silent" below.
      if (validateMutation.error !== null && status() !== "stopping") {
        showValidationToast({
          variant: "danger",
          title: "Validation failed",
          description: apiErrorMessage(validateMutation.error),
        });
      }
    } finally {
      finishOperation(operationId);
    }
  }

  async function runExplain() {
    if (!canRun()) {
      return;
    }

    const operationId = beginOperation("explaining", () => explainMutation.abort());
    explainMutation.reset();
    setPlanResult(null);
    setActiveTab("plan");
    try {
      await explainMutation.execute({ database: tab.database, sql: currentQuery(), operationId });
      setPlanResult(explainMutation.result);
    } catch {
      // Mutation state owns the visible error. Swallow the rethrow so an
      // expected HTTP failure or user abort does not become an unhandled
      // promise rejection.
    } finally {
      finishOperation(operationId);
    }
  }

  async function runExecute() {
    if (!canRun()) {
      return;
    }

    const operationId = beginOperation("running", () => executeMutation.abort());
    executeMutation.reset();
    setExecutionResult(null);
    setActiveTab("results");
    try {
      const result = await executeMutation.execute({
        database: tab.database,
        sql: currentQuery(),
        operationId,
      });
      setExecutionResult(result);
      if (result && changesSchema(result.command)) {
        await onSchemaChanged(tab.database);
      }
    } catch {
      // Mutation state owns the visible error. Swallow the rethrow so an
      // expected HTTP failure or user abort does not become an unhandled
      // promise rejection.
    } finally {
      finishOperation(operationId);
    }
  }

  async function stopAction() {
    const operation = activeOperation();
    if (!operation || stopPending()) {
      return;
    }
    setStatus("stopping");
    setStopError(null);
    setStopPending(true);
    const acknowledgeCancellation = () => {
      operation.abort();
      setActiveOperation(null);
      setStatus("idle");
    };
    try {
      await cancelMutation.execute(operation.id);
      acknowledgeCancellation();
    } catch (error) {
      if (error instanceof AppApiError && (error.status === 404 || error.status === 409)) {
        acknowledgeCancellation();
        return;
      }
      setStopError(`${apiErrorMessage(error)} Try stopping again.`);
    } finally {
      setStopPending(false);
    }
  }

  async function cancelForClose() {
    const operation = activeOperation();
    if (!operation) return;
    try {
      await cancelMutation.execute(operation.id);
    } catch (error) {
      if (!(error instanceof AppApiError && (error.status === 404 || error.status === 409)))
        throw error;
    }
    operation.abort();
    setActiveOperation(null);
  }

  task(() =>
    onRegister({
      isBusy: () =>
        status() !== "idle" ||
        executeMutation.pending ||
        validateMutation.pending ||
        explainMutation.pending,
      cancel: cancelForClose,
    }),
  );

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
      <div hidden={!active()} style={{ display: active() ? undefined : "none" }}>
        <QueryValidationToast toast={validationToast()} onDismiss={dismissValidationToast} />
      </div>

      <section
        class="cassie-query-page cassie-query-shell"
        data-slot="main"
        data-query-page="true"
        data-operation-active={activeOperation() ? "true" : "false"}
        id={`query-workspace-${tab.id}`}
        role="region"
        aria-labelledby={`query-workspace-title-${tab.id}`}
        hidden={!active()}
        style={{ display: active() ? undefined : "none" }}
      >
        <h1 class="sr-only" id={`query-workspace-title-${tab.id}`}>
          {tab.title} query workspace
        </h1>

        <section class="cassie-query-workspace" aria-label="Query workspace">
          <div class="cassie-query-availability-slot">
            <QueryAvailabilityStatus
              database={tab.database}
              state={availability()}
              onCreateDatabase={onCreateDatabase}
            />
          </div>
          <ResizableSplit
            orientation="vertical"
            initialSize={editorHeight()}
            min={30}
            max={80}
            onResize={(size) => {
              setEditorHeight(size);
              onEditorSplitChange(size);
            }}
            first={
              <QueryEditorPanel
                tabId={tab.id}
                database={tab.database}
                active={active}
                initialQuery={tab.sql}
                query={currentQuery}
                onQueryChange={handleQueryChange}
                isRunning={isQueryBusy}
                actionsEnabled={availability() === "available"}
                isStopping={stopPending()}
                onTrim={handleTrimQuery}
                onValidate={handleValidate}
                onExplain={handleExplain}
                onPlay={handlePlay}
                onStop={() => void stopAction()}
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

                <QueryExecutionSummary
                  result={activeTab() === "plan" ? planResult() : executionResult()}
                />

                <QueryResultsTabs
                  workspaceId={tab.id}
                  activeTab={activeTab}
                  onTabChange={handleTabChange}
                  resultsContent={
                    executionResult() ? (
                      <QueryResultTable result={executionResult() as QueryExecutionResult} />
                    ) : (
                      <QueryPlaceholder title="No rows" description="No query has run yet." />
                    )
                  }
                  listContent={
                    executionResult() ? (
                      <>
                        <QueryResultJson result={executionResult() as QueryExecutionResult} />
                      </>
                    ) : (
                      <QueryPlaceholder title="No rows" description="No query has run yet." />
                    )
                  }
                  planContent={
                    planResult() ? (
                      <QueryPlanText result={planResult() as QueryExecutionResult} />
                    ) : (
                      <QueryPlaceholder
                        title="No plan"
                        description="Run explain to inspect plan rows."
                      />
                    )
                  }
                />

                <QueryRunNote query={currentQuery} />
              </>
            }
          />
        </section>
      </section>
    </>
  );
}
