import { state } from "@askrjs/askr";
import { createQuery, queryScope } from "@askrjs/askr/data";
import { For } from "@askrjs/askr/control";
import { Portal } from "@askrjs/askr/foundations";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
} from "@askrjs/ui";
import { TriangleAlertIcon } from "@askrjs/lucide";
import { Alert, Button } from "@askrjs/themes/components";

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
import { queryService } from "@/features/query/query-service";
import {
  loadQueryWorkspace,
  saveQueryWorkspace,
  type PersistedQueryTab,
} from "@/features/query/query-tabs";
import { apiv1 } from "@/adapters";
import { getSession } from "@/shared/auth";
import { apiErrorMessage, AppApiError, unwrapResponse } from "@/shared/errors/api";

const defaultQuery = "SELECT 1 AS ready;";

const schemaChangingCommandPrefixes = ["ALTER ", "CREATE ", "DROP "];

function changesSchema(command: string) {
  return schemaChangingCommandPrefixes.some((prefix) => command.startsWith(prefix));
}

const workspaceQueries = queryScope("query-workspace");
const fetchDatabases = async ({ signal }: { signal?: AbortSignal }) =>
  unwrapResponse(await apiv1.listAdminDatabases({ signal }), "Unable to load databases");

export default function QueryPage() {
  const user = getSession()?.user ?? "anonymous";
  const restored = loadQueryWorkspace(user);
  const [tabs, setTabs] = state<PersistedQueryTab[]>(restored.tabs);
  const [activeTabId, setActiveTabId] = state<string | null>(restored.activeTabId);
  const [dialogOpen, setDialogOpen] = state(false);
  const [filter, setFilter] = state("");
  const controllers = new Map<string, { isBusy: () => boolean; cancel: () => Promise<void> }>();
  const databaseQuery = createQuery({
    key: workspaceQueries.key("databases"),
    fetch: fetchDatabases,
  });

  function persist(nextTabs: PersistedQueryTab[], nextActiveTabId: string | null) {
    setTabs(nextTabs);
    setActiveTabId(nextActiveTabId);
    saveQueryWorkspace(user, { version: 1, tabs: nextTabs, activeTabId: nextActiveTabId });
  }

  function createTab(database: string) {
    const ordinal = tabs().reduce((largest, tab) => Math.max(largest, tab.ordinal), 0) + 1;
    const tab: PersistedQueryTab = {
      id: crypto.randomUUID(),
      ordinal,
      title: `Query ${ordinal}`,
      database,
      sql: defaultQuery,
    };
    persist([...tabs(), tab], tab.id);
    setDialogOpen(false);
    setFilter("");
  }

  function updateSql(id: string, sql: string) {
    const nextTabs = tabs().map((tab) => {
      if (tab.id === id) tab.sql = sql;
      return tab;
    });
    saveQueryWorkspace(user, { version: 1, tabs: nextTabs, activeTabId: activeTabId() });
  }

  function activateTab(id: string | null) {
    setActiveTabId(id);
    saveQueryWorkspace(user, { version: 1, tabs: tabs(), activeTabId: id });
  }

  async function closeTab(tab: PersistedQueryTab) {
    const controller = controllers.get(tab.id);
    const needsConfirmation = tab.sql !== defaultQuery || controller?.isBusy();
    if (
      needsConfirmation &&
      !window.confirm(
        `Close ${tab.title}? Its saved SQL draft will be removed${controller?.isBusy() ? " after the running operation is cancelled" : ""}.`,
      )
    ) {
      return;
    }
    if (controller?.isBusy()) {
      try {
        await controller.cancel();
      } catch (error) {
        window.alert(`${apiErrorMessage(error)} The tab remains open.`);
        return;
      }
    }
    const nextTabs = tabs().filter((candidate) => candidate.id !== tab.id);
    const nextActive =
      activeTabId() === tab.id
        ? (nextTabs[Math.max(0, tabs().findIndex((candidate) => candidate.id === tab.id) - 1)]
            ?.id ?? null)
        : activeTabId();
    persist(nextTabs, nextActive);
  }

  function handleTabKeyDown(event: KeyboardEvent, index: number) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const availableTabs = tabs();
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? availableTabs.length - 1
          : (index + (event.key === "ArrowRight" ? 1 : -1) + availableTabs.length) %
            availableTabs.length;
    activateTab(availableTabs[nextIndex]?.id ?? null);
  }

  const availableNames = () => new Set((databaseQuery.data ?? []).map((database) => database.name));
  const filteredDatabases = () =>
    (databaseQuery.data ?? []).filter((database) =>
      database.name.toLowerCase().includes(filter().trim().toLowerCase()),
    );

  return (
    <>
      <nav class="cassie-query-tabs" aria-label="Query tabs">
        <div class="cassie-query-tabs-scroll" role="tablist" aria-label="Open queries">
          <For each={tabs()} by={(tab) => tab.id}>
            {(tab, index) => (
              <div class="cassie-query-tab-item">
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTabId() === tab.id}
                  tabIndex={activeTabId() === tab.id ? 0 : -1}
                  onClick={() => activateTab(tab.id)}
                  onKeyDown={(event: KeyboardEvent) => handleTabKeyDown(event, index())}
                >
                  <span>{tab.title}</span>
                  <small>{tab.database}</small>
                </button>
                <button
                  type="button"
                  aria-label={`Close ${tab.title}`}
                  onClick={() => void closeTab(tab)}
                >
                  ×
                </button>
              </div>
            )}
          </For>
        </div>
        <Button type="button" size="sm" variant="ghost" onPress={() => setDialogOpen(true)}>
          New Query
        </Button>
      </nav>

      {tabs().length === 0 ? (
        <main class="cassie-query-empty" id="main-content" tabindex={-1}>
          <h1>New Query</h1>
          <p>Choose a database to open a query workspace.</p>
          <Button type="button" variant="primary" onPress={() => setDialogOpen(true)}>
            New Query
          </Button>
        </main>
      ) : null}

      <For each={tabs()} by={(tab) => tab.id}>
        {(tab) => (
          <QueryWorkspace
            tab={tab}
            active={() => activeTabId() === tab.id}
            available={() => availableNames().has(tab.database)}
            onSqlChange={(sql) => updateSql(tab.id, sql)}
            onRegister={(controller) => controllers.set(tab.id, controller)}
          />
        )}
      </For>

      {dialogOpen() ? (
        <Dialog open onOpenChange={setDialogOpen}>
          <DialogPortal>
            <DialogOverlay class="cassie-query-dialog-overlay" />
            <DialogContent class="cassie-query-dialog">
              <DialogTitle>New Query</DialogTitle>
              <DialogDescription>Select the database this tab will use.</DialogDescription>
              <input
                aria-label="Filter databases"
                placeholder="Filter databases"
                value={filter()}
                onInput={(event: Event) => setFilter((event.target as HTMLInputElement).value)}
              />
              {databaseQuery.loading ? <p>Loading databases…</p> : null}
              {databaseQuery.error ? (
                <Alert
                  title="Unable to load databases"
                  variant="danger"
                  description={apiErrorMessage(databaseQuery.error)}
                  icon={<TriangleAlertIcon size={16} />}
                />
              ) : null}
              <div class="cassie-query-database-list">
                {filteredDatabases().map((database) => (
                  <div key={database.name}>
                    <button
                      type="button"
                      onClick={() => {
                        createTab(database.name);
                        setDialogOpen(false);
                      }}
                    >
                      <strong>{database.name}</strong>
                      {database.description ? <span>{database.description}</span> : null}
                    </button>
                  </div>
                ))}
              </div>
              <DialogClose asChild>
                <Button type="button" variant="ghost" onPress={() => setDialogOpen(false)}>
                  Cancel
                </Button>
              </DialogClose>
            </DialogContent>
          </DialogPortal>
        </Dialog>
      ) : null}
    </>
  );
}

interface QueryWorkspaceProps {
  tab: PersistedQueryTab;
  active: () => boolean;
  available: () => boolean;
  onSqlChange: (sql: string) => void;
  onRegister: (controller: { isBusy: () => boolean; cancel: () => Promise<void> }) => void;
}

function QueryWorkspace({ tab, active, available, onSqlChange, onRegister }: QueryWorkspaceProps) {
  const schemaQuery = createAdminQuerySchemaQuery(tab.database);
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();

  const [query, setQuery] = state(tab.sql);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [status, setStatus] = state<QueryStatus>("idle");
  const [editorHeight, setEditorHeight] = state(52);
  const [validationToast, setValidationToast] = state<QueryValidationToastData | null>(null);
  const [activeOperationId, setActiveOperationId] = state<string | null>(null);
  const [stopError, setStopError] = state<string | null>(null);
  const [stopPending, setStopPending] = state(false);

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

  const actionError = activeTab() === "plan" ? explainMutation.error : executeMutation.error;
  const actionErrorMessage =
    status() === "stopping" || actionError === null ? stopError() : apiErrorMessage(actionError);

  function beginOperation(nextStatus: QueryStatus) {
    const operationId = crypto.randomUUID();
    setActiveOperationId(operationId);
    setStopError(null);
    setStatus(nextStatus);
    return operationId;
  }

  function finishOperation(operationId: string) {
    if (activeOperationId() !== operationId || stopError() !== null) {
      return;
    }
    setActiveOperationId(null);
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
    onSqlChange(nextQuery);
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

    const operationId = beginOperation("validating");
    validateMutation.reset();
    try {
      await validateMutation.execute({ database: tab.database, sql: query(), operationId });
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
    if (!canRun) {
      return;
    }

    const operationId = beginOperation("explaining");
    explainMutation.reset();
    setActiveTab("plan");
    try {
      await explainMutation.execute({ database: tab.database, sql: query(), operationId });
    } catch {
      // Mutation state owns the visible error. Swallow the rethrow so an
      // expected HTTP failure or user abort does not become an unhandled
      // promise rejection.
    } finally {
      finishOperation(operationId);
    }
  }

  async function runExecute() {
    if (!canRun) {
      return;
    }

    const operationId = beginOperation("running");
    executeMutation.reset();
    setActiveTab("results");
    try {
      await executeMutation.execute({ database: tab.database, sql: query(), operationId });
      if (executeMutation.result !== null && changesSchema(executeMutation.result.command)) {
        queryService.invalidateSchema(tab.database);
        await schemaQuery.refresh();
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
    const operationId = activeOperationId();
    if (!operationId || stopPending()) {
      return;
    }
    setStatus("stopping");
    setStopError(null);
    setStopPending(true);
    try {
      await queryService.cancel(operationId);
    } catch (error) {
      if (error instanceof AppApiError && (error.status === 404 || error.status === 409)) {
        return;
      }
      setStopError(`${apiErrorMessage(error)} Try stopping again.`);
    } finally {
      setStopPending(false);
    }
  }

  async function cancelForClose() {
    const operationId = activeOperationId();
    if (!operationId) return;
    try {
      await queryService.cancel(operationId);
    } catch (error) {
      if (!(error instanceof AppApiError && (error.status === 404 || error.status === 409)))
        throw error;
    }
    executeMutation.abort();
    validateMutation.abort();
    explainMutation.abort();
  }

  onRegister({
    isBusy: () =>
      status() !== "idle" ||
      executeMutation.pending ||
      validateMutation.pending ||
      explainMutation.pending,
    cancel: cancelForClose,
  });

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
      {active() ? (
        <Portal>
          <QuerySchemaTree
            schema={getSchemaDatabases}
            selectedItemId={() => selectedItemId() ?? undefined}
            onSelectItem={handleSchemaSelection}
          />
        </Portal>
      ) : null}

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
        hidden={!active()}
      >
        <h1 class="sr-only" id="query-workspace-title">
          Query workspace
        </h1>

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
          {!available() ? (
            <Alert
              title="Database unavailable"
              variant="danger"
              description={`${tab.database} is no longer available. Your SQL draft is preserved.`}
              icon={<TriangleAlertIcon size={16} />}
            />
          ) : null}
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
                isRunning={isQueryBusy || !available()}
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
                      queryService.invalidateSchema(tab.database);
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
