import { state } from "@askrjs/askr";
import { createQuery, queryScope } from "@askrjs/askr/data";
import { For, Show } from "@askrjs/askr/control";
import { TriangleAlertIcon } from "@askrjs/lucide";
import {
  Alert,
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogOverlay,
  AlertDialogPortal,
  AlertDialogTitle,
  Button,
} from "@askrjs/themes/components";

import { QueryEditorPanel } from "@/components/query/query-editor-panel";
import { QueryAvailabilityStatus } from "@/components/query/query-availability-status";
import { CreateDatabaseDialog } from "@/components/query/create-database-dialog";
import { QueryExecutionBanner } from "@/components/query/query-execution-banner";
import { QueryExecutionSummary } from "@/components/query/query-execution-summary";
import { QueryPlaceholder } from "@/components/query/query-placeholder";
import { QueryPlanText } from "@/components/query/query-plan-text";
import { QueryResultJson } from "@/components/query/query-result-json";
import { QueryResultTable } from "@/components/query/query-result-table";
import { QueryResultTab, QueryResultsTabs } from "@/components/query/query-results-tabs";
import { QuerySidebar } from "@/components/query/query-sidebar";
import { NewQueryDialog } from "@/components/query/new-query-dialog";
import type { QueryValidationToastData } from "@/components/query/query-validation-toast";
import { QueryValidationToast } from "@/components/query/query-validation-toast";
import { ResizableSplit } from "@/components/query/resizable-split";
import { SidebarPortalContent } from "@/components/shell/sidebar-portal-host";
import {
  DatabaseCatalogController,
  type DatabaseCatalogEntry,
} from "@/features/query/database-catalog-controller";
import { buildSqlCompletionItems } from "@/features/query/query-completions";
import {
  type QuerySchemaDatabase,
  QuerySchemaItem,
  QueryStatus,
  type QueryExecutionResult,
  type QueryTabRuntimeState,
} from "@/features/query/query-models";
import {
  createExecuteQueryMutation,
  createExplainQueryMutation,
  createValidateQueryMutation,
} from "@/features/query/query-actions";
import { queryService } from "@/features/query/query-service";
import { loadQueryWorkspace, type PersistedQueryTab } from "@/features/query/query-tabs";
import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import { QueryWorkspaceRegistry } from "@/features/query/query-workspace-registry";
import {
  createWorkspaceTab,
  removeWorkspaceTab,
  renameWorkspaceTab,
  updateWorkspaceTab,
} from "@/features/query/query-workspace-tabs";
import { apiv1 } from "@/adapters";
import { getSession } from "@/shared/auth";
import { apiErrorMessage, AppApiError, unwrapResponse } from "@/shared/errors/api";

const schemaChangingCommandPrefixes = ["ALTER ", "CREATE ", "DROP "];
export type DatabaseAvailability = "checking" | "available" | "unavailable" | "discovery-error";

function changesSchema(command: string) {
  return schemaChangingCommandPrefixes.some((prefix) => command.startsWith(prefix));
}

const workspaceQueries = queryScope("query-workspace");
const fetchDatabases = async ({ signal }: { signal?: AbortSignal }) =>
  unwrapResponse(await apiv1.listAdminDatabases({ signal }), "Unable to load databases");
const workspaceRegistry = new QueryWorkspaceRegistry();

export default function QueryPage() {
  const user = getSession()?.user ?? "anonymous";
  const restored = loadQueryWorkspace(user);
  const [tabs, setTabs] = state<PersistedQueryTab[]>(restored.tabs);
  const [activeTabId, setActiveTabId] = state<string | null>(restored.activeTabId);
  const [dialogOpen, setDialogOpen] = state(false);
  const [createDatabaseOpen, setCreateDatabaseOpen] = state(false);
  const [newQueryDraft] = state({ name: "", database: "" });
  const [persistenceFailed, setPersistenceFailed] = state(false);
  const [persistenceOperationId, setPersistenceOperationId] = state("");
  const [closeCandidate, setCloseCandidate] = state<PersistedQueryTab | null>(null);
  const [closeError, setCloseError] = state<string | null>(null);
  const [catalogRevision, setCatalogRevision] = state(0);
  const persistence = createQueryPersistenceCoordinator(user, (failed) =>
    setPersistenceFailed(failed),
  );
  const [catalogController] = state(
    new DatabaseCatalogController(
      (database, options) => queryService.getSchema(database, options),
      () => setCatalogRevision((revision) => revision + 1),
      (database) => queryService.invalidateSchema(database),
    ),
  );
  const databaseCatalogs = catalogController();
  const databaseQuery = createQuery({
    key: workspaceQueries.key("databases"),
    fetch: fetchDatabases,
  });
  catalogRevision();
  const initialActiveDatabase = restored.tabs.find(
    (tab) => tab.id === restored.activeTabId,
  )?.database;
  if (initialActiveDatabase) {
    queueMicrotask(() => void databaseCatalogs.activate(initialActiveDatabase));
  }

  const catalogEntries = () => {
    catalogRevision();
    const entries = databaseCatalogs.entries();
    const known = new Set(entries.map((entry) => entry.canonicalName));
    for (const database of databaseQuery.data ?? []) {
      const canonicalName = database.name.trim().toLocaleLowerCase();
      if (!known.has(canonicalName)) {
        entries.push({
          canonicalName,
          name: database.name,
          status: "idle",
          expanded: false,
        });
      }
    }
    return entries.sort((left, right) =>
      left.name.localeCompare(right.name, undefined, { sensitivity: "base" }),
    );
  };
  const currentActiveDatabase = () => tabs().find((tab) => tab.id === activeTabId())?.database;

  function persist(
    nextTabs: PersistedQueryTab[],
    nextActiveTabId: string | null,
    operationId = crypto.randomUUID(),
  ) {
    setTabs(nextTabs);
    setActiveTabId(nextActiveTabId);
    setPersistenceOperationId(operationId);
    persistence.schedule({ version: 1, tabs: nextTabs, activeTabId: nextActiveTabId }, operationId);
  }

  function createTab(database: string, requestedTitle = "") {
    const tab = createWorkspaceTab(tabs(), database, requestedTitle, crypto.randomUUID());
    databaseCatalogs.insert(database);
    void databaseCatalogs.activate(database);
    persist([...tabs(), tab], tab.id);
    requestAnimationFrame(() => {
      syncActiveDom(tab.id);
      focusQuery(tab.id);
    });
    setDialogOpen(false);
    newQueryDraft().name = "";
    newQueryDraft().database = "";
  }

  function openNewQuery() {
    newQueryDraft().name = "";
    newQueryDraft().database = "";
    setDialogOpen(true);
  }

  function renameTab(id: string, title: string) {
    persist(renameWorkspaceTab(tabs(), id, title), activeTabId());
  }

  function openCreateDatabase() {
    setCreateDatabaseOpen(true);
  }

  function loadCatalogsForSearch() {
    databaseCatalogs.reconcile((databaseQuery.data ?? []).map((database) => database.name));
    void databaseCatalogs.loadRemaining(3);
  }

  function updateSql(id: string, sql: string) {
    persist(updateWorkspaceTab(tabs(), id, { sql }), activeTabId());
  }

  function updateEditorSplit(id: string, editorSplitPercent: number) {
    persist(updateWorkspaceTab(tabs(), id, { editorSplitPercent }), activeTabId());
  }

  function activateTab(id: string | null) {
    const database = tabs().find((tab) => tab.id === id)?.database;
    if (database) void databaseCatalogs.activate(database);
    setActiveTabId(id);
    workspaceRegistry.activate(id);
    persist(tabs(), id);
    syncActiveDom(id);
  }

  function syncActiveDom(id: string | null) {
    document.querySelectorAll<HTMLElement>("[data-query-page]").forEach((workspace) => {
      const selected = workspace.id === `query-workspace-${id ?? ""}`;
      workspace.hidden = !selected;
      workspace.style.display = selected ? "" : "none";
    });
  }

  function focusQuery(id: string) {
    const trigger = document.getElementById(`saved-query-${id}`);
    trigger?.focus();
    trigger?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }

  function requestCloseTab(tab: PersistedQueryTab) {
    setCloseError(null);
    setCloseCandidate(tab);
  }

  async function closeTab(tab: PersistedQueryTab) {
    persistence.flush(persistenceOperationId() || undefined);
    const controller = workspaceRegistry.controller(tab.id);
    if (controller?.isBusy()) {
      try {
        await controller.cancel();
      } catch (error) {
        setCloseError(`${apiErrorMessage(error)} The query remains available.`);
        return;
      }
    }
    const next = removeWorkspaceTab(tabs(), activeTabId(), tab.id);
    persist(next.tabs, next.activeTabId);
    workspaceRegistry.remove(tab.id);
    setCloseCandidate(null);
    requestAnimationFrame(() => {
      if (next.activeTabId) focusQuery(next.activeTabId);
      else document.getElementById("new-query-button")?.focus();
    });
  }

  const availableNames = () => new Set((databaseQuery.data ?? []).map((database) => database.name));
  const databaseAvailability = (database: string): DatabaseAvailability => {
    if (databaseQuery.loading && !databaseQuery.data) return "checking";
    if (databaseQuery.error && !databaseQuery.data) return "discovery-error";
    return availableNames().has(database) ? "available" : "unavailable";
  };
  const currentCloseError = closeError();
  const currentPersistenceOperationId = persistenceOperationId();
  return (
    <main
      id="main-content"
      class="cassie-query-route"
      data-has-saved-queries={tabs().length > 0 ? "true" : "false"}
      data-catalog-revision={catalogRevision()}
      data-persistence-operation-id={currentPersistenceOperationId}
      tabindex={-1}
      ref={(node: HTMLElement | null) => {
        if (node === null) {
          persistence.flush(persistenceOperationId() || undefined);
        }
      }}
    >
      {persistenceFailed() ? (
        <Alert
          title="SQL draft not saved"
          variant="warning"
          description="Browser storage rejected the latest change. Your draft remains available in this query."
          icon={<TriangleAlertIcon size={16} />}
        />
      ) : null}
      {tabs().length === 0 ? (
        <SidebarPortalContent>
          <QuerySidebar
            queries={tabs}
            activeQueryId={activeTabId}
            catalogs={catalogEntries}
            activeDatabase={currentActiveDatabase}
            onActivateQuery={(id) => activateTab(id)}
            onRenameQuery={renameTab}
            onRemoveQuery={requestCloseTab}
            onNewQuery={openNewQuery}
            onCreateDatabase={openCreateDatabase}
            onSelectSchemaItem={() => undefined}
            onSetDatabaseExpanded={(database, expanded) => {
              void databaseCatalogs.setExpanded(database, expanded);
            }}
            onSearchCatalogs={loadCatalogsForSearch}
            onRetryDatabase={(database) => void databaseCatalogs.retry(database)}
          />
        </SidebarPortalContent>
      ) : null}

      {tabs().length === 0 ? (
        <section class="cassie-query-empty" aria-labelledby="empty-workspace-title">
          <h1 id="empty-workspace-title">New Query</h1>
          <p>Choose a database to open a query workspace.</p>
          <Button type="button" variant="primary" onPress={openNewQuery}>
            New Query
          </Button>
        </section>
      ) : null}

      <For each={() => tabs().filter((tab) => tab.id === activeTabId())} by={(tab) => tab.id}>
        {(tab) => (
          <QueryWorkspace
            tab={tab}
            active={activeTabId() === tab.id}
            availability={() => databaseAvailability(tab.database)}
            onSqlChange={(sql) => updateSql(tab.id, sql)}
            onEditorSplitChange={(percent) => updateEditorSplit(tab.id, percent)}
            onRuntimeChange={() => undefined}
            onActiveRegister={(reporter) =>
              workspaceRegistry.registerActiveReporter(tab.id, reporter)
            }
            onRegister={(controller) => workspaceRegistry.registerController(tab.id, controller)}
            onCreateDatabase={openCreateDatabase}
            catalogs={catalogEntries}
            queries={tabs}
            activeQueryId={activeTabId}
            onActivateQuery={(id) => activateTab(id)}
            onRenameQuery={renameTab}
            onRemoveQuery={requestCloseTab}
            onNewQuery={openNewQuery}
            onSetDatabaseExpanded={(database, expanded) => {
              void databaseCatalogs.setExpanded(database, expanded);
            }}
            onSearchCatalogs={loadCatalogsForSearch}
            onRetryDatabase={(database) => void databaseCatalogs.retry(database)}
            onSchemaChanged={(database) => databaseCatalogs.refresh(database)}
          />
        )}
      </For>

      <Show when={dialogOpen()}>
        <NewQueryDialog
          draft={newQueryDraft()}
          databases={() => (databaseQuery.data ?? []).map((database) => database.name)}
          loading={databaseQuery.loading}
          error={databaseQuery.error ? apiErrorMessage(databaseQuery.error) : null}
          suggestedName={`Query ${
            tabs().reduce((largest, tab) => Math.max(largest, tab.ordinal), 0) + 1
          }`}
          onClose={() => setDialogOpen(false)}
          onCreate={createTab}
        />
      </Show>
      {createDatabaseOpen() ? (
        <CreateDatabaseDialog
          onClose={() => setCreateDatabaseOpen(false)}
          onCreated={async (name) => {
            await databaseQuery.refresh();
            createTab(name);
            await databaseCatalogs.activate(name);
          }}
        />
      ) : null}
      {closeCandidate() ? (
        <AlertDialog open onOpenChange={(open) => !open && setCloseCandidate(null)}>
          <AlertDialogPortal>
            <AlertDialogOverlay class="cassie-query-dialog-overlay" />
            <AlertDialogContent class="cassie-query-dialog cassie-delete-query-dialog">
              <div class="cassie-delete-query-dialog-header">
                <span class="cassie-delete-query-dialog-icon" aria-hidden="true">
                  <TriangleAlertIcon size={18} />
                </span>
                <div>
                  <AlertDialogTitle>Delete query?</AlertDialogTitle>
                  <AlertDialogDescription>
                    “{closeCandidate()?.title}” will be permanently deleted from this browser. This
                    action cannot be undone
                    {workspaceRegistry.controller(closeCandidate()?.id ?? "")?.isBusy()
                      ? ", and its running operation will be cancelled first."
                      : "."}
                  </AlertDialogDescription>
                </div>
              </div>
              {currentCloseError ? (
                <Alert
                  title="Unable to delete query"
                  variant="danger"
                  description={currentCloseError}
                />
              ) : null}
              <div class="cassie-delete-query-dialog-actions">
                <AlertDialogCancel asChild>
                  <Button type="button" variant="outline">
                    Cancel
                  </Button>
                </AlertDialogCancel>
                <AlertDialogAction asChild>
                  <Button
                    type="button"
                    variant="destructive"
                    onPress={() => {
                      const candidate = closeCandidate();
                      if (candidate) void closeTab(candidate);
                    }}
                  >
                    Delete query
                  </Button>
                </AlertDialogAction>
              </div>
            </AlertDialogContent>
          </AlertDialogPortal>
        </AlertDialog>
      ) : null}
    </main>
  );
}

interface QueryWorkspaceProps {
  tab: PersistedQueryTab;
  active: boolean;
  availability: () => DatabaseAvailability;
  onSqlChange: (sql: string) => void;
  onEditorSplitChange: (percent: number) => void;
  onRuntimeChange: (runtime: QueryTabRuntimeState) => void;
  onActiveRegister: (setActive: (active: boolean) => void) => void;
  onRegister: (controller: { isBusy: () => boolean; cancel: () => Promise<void> }) => void;
  onCreateDatabase: () => void;
  catalogs: () => DatabaseCatalogEntry[];
  queries: () => PersistedQueryTab[];
  activeQueryId: () => string | null;
  onActivateQuery: (id: string) => void;
  onRenameQuery: (id: string, title: string) => void;
  onRemoveQuery: (query: PersistedQueryTab) => void;
  onNewQuery: () => void;
  onSetDatabaseExpanded: (database: string, expanded: boolean) => void;
  onSearchCatalogs: () => void;
  onRetryDatabase: (database: string) => void;
  onSchemaChanged: (database: string) => Promise<void>;
}

function QueryWorkspace({
  tab,
  active,
  availability,
  onSqlChange,
  onEditorSplitChange,
  onRuntimeChange,
  onActiveRegister,
  onRegister,
  onCreateDatabase,
  catalogs,
  queries,
  activeQueryId,
  onActivateQuery,
  onRenameQuery,
  onRemoveQuery,
  onNewQuery,
  onSetDatabaseExpanded,
  onSearchCatalogs,
  onRetryDatabase,
  onSchemaChanged,
}: QueryWorkspaceProps) {
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();

  const [queryDraft] = state({ value: tab.sql });
  const [isActive, setIsActive] = state(active);
  onActiveRegister(setIsActive);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [status, setStatus] = state<QueryStatus>("idle");
  const [editorHeight, setEditorHeight] = state(tab.editorSplitPercent ?? 52);
  const [validationToast, setValidationToast] = state<QueryValidationToastData | null>(null);
  const [stopError, setStopError] = state<string | null>(null);
  const [stopPending, setStopPending] = state(false);
  const [executionResult, setExecutionResult] = state<QueryExecutionResult | null>(null);
  const [planResult, setPlanResult] = state<QueryExecutionResult | null>(null);
  let activeOperationId: string | null = null;
  let dirty = false;

  function reportRuntime(
    phase: QueryTabRuntimeState["phase"],
    outcome: QueryTabRuntimeState["outcome"],
    message?: string,
  ) {
    onRuntimeChange({ dirty, phase, outcome, unread: !isActive() && outcome !== "none", message });
  }

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
  const currentQuery = () => queryDraft().value;
  const hasQuery = currentQuery().trim().length > 0;

  const isExecutionBusy = executeMutation.pending || explainMutation.pending;
  const isValidating = validateMutation.pending || status() === "validating";
  const isQueryBusy = status() !== "idle" || isExecutionBusy || isValidating;
  const canRun = hasQuery && !isQueryBusy && availability() === "available";
  selectedItemId();
  validationToast();
  const currentStopError = stopError();
  const currentExecutionResultValue = executionResult();
  const currentPlanResult = planResult();
  const currentExecutionResult =
    activeTab() === "plan" ? currentPlanResult : currentExecutionResultValue;

  const actionError = activeTab() === "plan" ? explainMutation.error : executeMutation.error;
  const actionErrorMessage =
    status() === "stopping" || actionError === null
      ? currentStopError
      : apiErrorMessage(actionError);

  function beginOperation(nextStatus: QueryStatus) {
    const operationId = crypto.randomUUID();
    activeOperationId = operationId;
    setStopError(null);
    setStatus(nextStatus);
    reportRuntime(
      nextStatus === "running"
        ? "executing"
        : nextStatus === "validating"
          ? "validating"
          : "explaining",
      "none",
    );
    return operationId;
  }

  function finishOperation(operationId: string) {
    if (activeOperationId !== operationId || stopError() !== null) {
      return;
    }
    activeOperationId = null;
    setStatus("idle");
    reportRuntime("idle", "succeeded");
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

    queryDraft().value = nextQuery;
    dirty = true;
    onSqlChange(nextQuery);
    reportRuntime("idle", "none");
  }

  function handleSchemaSelection(item: QuerySchemaItem) {
    setSelectedItemId(item.id);
  }

  function handleTrimQuery() {
    const trimmed = currentQuery().trim();
    if (trimmed === currentQuery()) return;
    queryDraft().value = trimmed;
    resetQueryFeedback();
    dirty = true;
    onSqlChange(trimmed);
    reportRuntime("idle", "none");
  }

  async function runValidate() {
    if (!canRun) {
      return;
    }

    const operationId = beginOperation("validating");
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
    if (!canRun) {
      return;
    }

    const operationId = beginOperation("explaining");
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
    if (!canRun) {
      return;
    }

    const operationId = beginOperation("running");
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
    const operationId = activeOperationId;
    if (!operationId || stopPending()) {
      return;
    }
    setStatus("stopping");
    reportRuntime("cancelling", "none");
    setStopError(null);
    setStopPending(true);
    const acknowledgeCancellation = () => {
      executeMutation.abort();
      validateMutation.abort();
      explainMutation.abort();
      activeOperationId = null;
      setStatus("idle");
      reportRuntime("idle", "cancelled");
    };
    try {
      await queryService.cancel(operationId);
      acknowledgeCancellation();
    } catch (error) {
      if (error instanceof AppApiError && (error.status === 404 || error.status === 409)) {
        acknowledgeCancellation();
        return;
      }
      setStopError(`${apiErrorMessage(error)} Try stopping again.`);
      reportRuntime("cancelling", "failed", apiErrorMessage(error));
    } finally {
      setStopPending(false);
    }
  }

  async function cancelForClose() {
    const operationId = activeOperationId;
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
      {isActive() ? (
        <SidebarPortalContent>
          <QuerySidebar
            queries={queries}
            activeQueryId={activeQueryId}
            catalogs={catalogs}
            activeDatabase={tab.database}
            selectedItemId={() => selectedItemId() ?? undefined}
            onActivateQuery={onActivateQuery}
            onRenameQuery={onRenameQuery}
            onRemoveQuery={onRemoveQuery}
            onNewQuery={onNewQuery}
            onCreateDatabase={onCreateDatabase}
            onSelectSchemaItem={handleSchemaSelection}
            onSetDatabaseExpanded={onSetDatabaseExpanded}
            onSearchCatalogs={onSearchCatalogs}
            onRetryDatabase={onRetryDatabase}
          />
        </SidebarPortalContent>
      ) : null}

      <QueryValidationToast
        toast={isActive() ? validationToast() : null}
        onDismiss={dismissValidationToast}
      />

      <section
        class="cassie-query-page cassie-query-shell"
        data-slot="main"
        data-query-page="true"
        id={`query-workspace-${tab.id}`}
        role="region"
        aria-labelledby={`query-workspace-title-${tab.id}`}
        hidden={!isActive()}
        style={{ display: isActive() ? undefined : "none" }}
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
                active={isActive}
                query={currentQuery()}
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

                <QueryExecutionSummary result={currentExecutionResult} />

                <QueryResultsTabs
                  workspaceId={tab.id}
                  activeTab={activeTab}
                  onTabChange={handleTabChange}
                  resultsContent={
                    currentExecutionResult ? (
                      <QueryResultTable result={currentExecutionResult} />
                    ) : (
                      <QueryPlaceholder title="No rows" description="No query has run yet." />
                    )
                  }
                  listContent={
                    currentExecutionResult ? (
                      <>
                        <QueryResultJson result={currentExecutionResult} />
                      </>
                    ) : (
                      <QueryPlaceholder title="No rows" description="No query has run yet." />
                    )
                  }
                  planContent={
                    currentExecutionResult ? (
                      <QueryPlanText result={currentExecutionResult} />
                    ) : (
                      <QueryPlaceholder
                        title="No plan"
                        description="Run explain to inspect plan rows."
                      />
                    )
                  }
                />

                {hasQuery ? null : (
                  <p class="cassie-query-run-note">
                    Type SQL to enable run, validate, and explain actions.
                  </p>
                )}
              </>
            }
          />
        </section>
      </section>
    </>
  );
}
