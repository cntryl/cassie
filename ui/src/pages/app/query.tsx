import { state } from "@askrjs/askr";
import { createQuery, queryScope } from "@askrjs/askr/data";
import { For } from "@askrjs/askr/control";
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
import type { QueryValidationToastData } from "@/components/query/query-validation-toast";
import { QueryValidationToast } from "@/components/query/query-validation-toast";
import { ResizableSplit } from "@/components/query/resizable-split";
import { SidebarPortalContent } from "@/components/shell/sidebar-portal-host";
import { buildSqlCompletionItems } from "@/features/query/query-completions";
import {
  type QuerySchemaDatabase,
  QuerySchemaItem,
  QueryStatus,
  type QueryTabRuntimeState,
} from "@/features/query/query-models";
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
import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import { apiv1 } from "@/adapters";
import { getSession } from "@/shared/auth";
import { apiErrorMessage, AppApiError, unwrapResponse } from "@/shared/errors/api";

const defaultQuery = "SELECT 1 AS ready;";

const schemaChangingCommandPrefixes = ["ALTER ", "CREATE ", "DROP "];
export type DatabaseAvailability = "checking" | "available" | "unavailable" | "discovery-error";

function changesSchema(command: string) {
  return schemaChangingCommandPrefixes.some((prefix) => command.startsWith(prefix));
}

const workspaceQueries = queryScope("query-workspace");
const fetchDatabases = async ({ signal }: { signal?: AbortSignal }) =>
  unwrapResponse(await apiv1.listAdminDatabases({ signal }), "Unable to load databases");
const controllers = new Map<string, { isBusy: () => boolean; cancel: () => Promise<void> }>();
const activeWorkspaceReporters = new Map<string, (active: boolean) => void>();

export default function QueryPage() {
  const user = getSession()?.user ?? "anonymous";
  const restored = loadQueryWorkspace(user);
  const [tabs, setTabs] = state<PersistedQueryTab[]>(restored.tabs);
  const [activeTabId, setActiveTabId] = state<string | null>(restored.activeTabId);
  const [dialogOpen, setDialogOpen] = state(false);
  const [createDatabaseOpen, setCreateDatabaseOpen] = state(false);
  const [filter, setFilter] = state("");
  const [persistenceFailed, setPersistenceFailed] = state(false);
  const [closeCandidate, setCloseCandidate] = state<PersistedQueryTab | null>(null);
  const [closeError, setCloseError] = state<string | null>(null);
  const persistence = createQueryPersistenceCoordinator(user, () => setPersistenceFailed(true));
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
    requestAnimationFrame(() => {
      syncActiveDom(tab.id);
      focusQuery(tab.id);
    });
    setDialogOpen(false);
    setFilter("");
  }

  function openCreateDatabase() {
    setCreateDatabaseOpen(true);
  }

  function updateSql(id: string, sql: string) {
    const nextTabs = tabs().map((tab) => {
      if (tab.id === id) tab.sql = sql;
      return tab;
    });
    persistence.schedule({ version: 1, tabs: nextTabs, activeTabId: activeTabId() });
  }

  function activateTab(id: string | null) {
    setActiveTabId(id);
    for (const [tabId, reportActive] of activeWorkspaceReporters) reportActive(tabId === id);
    saveQueryWorkspace(user, { version: 1, tabs: tabs(), activeTabId: id });
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
    persistence.flush();
    const controller = controllers.get(tab.id);
    if (controller?.isBusy()) {
      try {
        await controller.cancel();
      } catch (error) {
        setCloseError(`${apiErrorMessage(error)} The query remains available.`);
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
    controllers.delete(tab.id);
    setCloseCandidate(null);
    requestAnimationFrame(() => {
      if (nextActive) focusQuery(nextActive);
      else document.getElementById("new-query-button")?.focus();
    });
  }

  const availableNames = () => new Set((databaseQuery.data ?? []).map((database) => database.name));
  const databaseAvailability = (database: string): DatabaseAvailability => {
    if (databaseQuery.loading && !databaseQuery.data) return "checking";
    if (databaseQuery.error && !databaseQuery.data) return "discovery-error";
    return availableNames().has(database) ? "available" : "unavailable";
  };
  const filteredDatabases = () =>
    (databaseQuery.data ?? []).filter((database) =>
      database.name.toLowerCase().includes(filter().trim().toLowerCase()),
    );
  const currentFilter = filter();
  const currentCloseError = closeError();
  const databaseTree = (): QuerySchemaDatabase[] =>
    (databaseQuery.data ?? []).map((database) => ({
      id: database.name,
      label: database.name,
      namespaces: [],
    }));

  return (
    <main
      id="main-content"
      class="cassie-query-route"
      data-has-saved-queries={tabs().length > 0 ? "true" : "false"}
      tabindex={-1}
      ref={(node: HTMLElement | null) => {
        if (node === null) {
          persistence.dispose();
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
            schema={databaseTree}
            onActivateQuery={(id) => activateTab(id)}
            onRemoveQuery={requestCloseTab}
            onNewQuery={() => setDialogOpen(true)}
            onCreateDatabase={openCreateDatabase}
            onSelectSchemaItem={() => undefined}
          />
        </SidebarPortalContent>
      ) : null}

      {tabs().length === 0 ? (
        <section class="cassie-query-empty" aria-labelledby="empty-workspace-title">
          <h1 id="empty-workspace-title">New Query</h1>
          <p>Choose a database to open a query workspace.</p>
          <Button type="button" variant="primary" onPress={() => setDialogOpen(true)}>
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
            onRuntimeChange={() => undefined}
            onActiveRegister={(reporter) => activeWorkspaceReporters.set(tab.id, reporter)}
            onRegister={(controller) => controllers.set(tab.id, controller)}
            onCreateDatabase={openCreateDatabase}
            databases={() => (databaseQuery.data ?? []).map((database) => database.name)}
            queries={tabs}
            activeQueryId={activeTabId}
            onActivateQuery={(id) => activateTab(id)}
            onRemoveQuery={requestCloseTab}
            onNewQuery={() => setDialogOpen(true)}
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
                value={currentFilter}
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
      {createDatabaseOpen() ? (
        <CreateDatabaseDialog
          databaseNames={() => (databaseQuery.data ?? []).map((database) => database.name)}
          onClose={() => setCreateDatabaseOpen(false)}
          onCreated={async (name) => {
            await databaseQuery.refresh();
            createTab(name);
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
                    {controllers.get(closeCandidate()?.id ?? "")?.isBusy()
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
  onRuntimeChange: (runtime: QueryTabRuntimeState) => void;
  onActiveRegister: (setActive: (active: boolean) => void) => void;
  onRegister: (controller: { isBusy: () => boolean; cancel: () => Promise<void> }) => void;
  onCreateDatabase: () => void;
  databases: () => string[];
  queries: () => PersistedQueryTab[];
  activeQueryId: () => string | null;
  onActivateQuery: (id: string) => void;
  onRemoveQuery: (query: PersistedQueryTab) => void;
  onNewQuery: () => void;
}

function QueryWorkspace({
  tab,
  active,
  availability,
  onSqlChange,
  onRuntimeChange,
  onActiveRegister,
  onRegister,
  onCreateDatabase,
  databases,
  queries,
  activeQueryId,
  onActivateQuery,
  onRemoveQuery,
  onNewQuery,
}: QueryWorkspaceProps) {
  const schemaQuery = createAdminQuerySchemaQuery(tab.database);
  const executeMutation = createExecuteQueryMutation();
  const validateMutation = createValidateQueryMutation();
  const explainMutation = createExplainQueryMutation();

  const [queryDraft] = state({ value: tab.sql });
  const [isActive, setIsActive] = state(active);
  onActiveRegister(setIsActive);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [status, setStatus] = state<QueryStatus>("idle");
  const [editorHeight, setEditorHeight] = state(52);
  const [validationToast, setValidationToast] = state<QueryValidationToastData | null>(null);
  const [stopError, setStopError] = state<string | null>(null);
  const [stopPending, setStopPending] = state(false);
  let activeOperationId: string | null = null;
  let dirty = false;

  function reportRuntime(
    phase: QueryTabRuntimeState["phase"],
    outcome: QueryTabRuntimeState["outcome"],
    message?: string,
  ) {
    onRuntimeChange({ dirty, phase, outcome, unread: !isActive() && outcome !== "none", message });
  }

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

  const getSchemaDatabases = () => {
    const schemaDatabases = schemaQuery.data?.databases ?? [];
    const loaded = new Map(schemaDatabases.map((database) => [database.id, database]));
    return databases().map(
      (database): QuerySchemaDatabase =>
        loaded.get(database) ?? { id: database, label: database, namespaces: [] },
    );
  };
  const getCompletionItems = (context: Parameters<typeof buildSqlCompletionItems>[2]) =>
    buildSqlCompletionItems(getSchemaDatabases(), tab.database, context);
  const currentQuery = () => queryDraft().value;
  const hasQuery = currentQuery().trim().length > 0;

  const isExecutionBusy = executeMutation.pending || explainMutation.pending;
  const isValidating = validateMutation.pending || status() === "validating";
  const isQueryBusy = status() !== "idle" || isExecutionBusy || isValidating;
  const activeExecution = activeTab() === "plan" ? explainMutation.result : executeMutation.result;
  const canRun = hasQuery && !isQueryBusy && availability() === "available";
  selectedItemId();
  validationToast();
  const currentStopError = stopError();

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
      await explainMutation.execute({ database: tab.database, sql: currentQuery(), operationId });
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
      await executeMutation.execute({ database: tab.database, sql: currentQuery(), operationId });
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
            schema={getSchemaDatabases}
            selectedItemId={() => selectedItemId() ?? undefined}
            onActivateQuery={onActivateQuery}
            onRemoveQuery={onRemoveQuery}
            onNewQuery={onNewQuery}
            onCreateDatabase={onCreateDatabase}
            onSelectSchemaItem={handleSchemaSelection}
          />
        </SidebarPortalContent>
      ) : null}

      <QueryValidationToast
        toast={isActive() ? validationToast() : null}
        onDismiss={dismissValidationToast}
        onPause={suspendValidationToastTimer}
        onResume={resumeValidationToastTimer}
      />

      <section
        class="cassie-query-page cassie-query-shell"
        data-slot="main"
        data-query-page="true"
        id={`query-workspace-${tab.id}`}
        role="region"
        aria-labelledby={`query-workspace-title-${tab.id}`}
        ref={handleMainRef}
        hidden={!isActive()}
        style={{ display: isActive() ? undefined : "none" }}
      >
        <h1 class="sr-only" id={`query-workspace-title-${tab.id}`}>
          {tab.title} query workspace
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
            onResize={(size) => setEditorHeight(size)}
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

                <QueryExecutionSummary result={activeExecution} />

                <QueryResultsTabs
                  workspaceId={tab.id}
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
      </section>
    </>
  );
}
