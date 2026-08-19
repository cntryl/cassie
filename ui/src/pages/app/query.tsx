import { state } from "@askrjs/askr";
import { createQuery } from "@askrjs/askr/data";
import { For, Show } from "@askrjs/askr/control";
import { defer } from "@askrjs/askr/fx";
import { task } from "@askrjs/askr/resources";
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

import { CreateDatabaseDialog } from "@/components/query/create-database-dialog";
import { QuerySidebar } from "@/components/query/query-sidebar";
import { NewQueryDialog } from "@/components/query/new-query-dialog";
import { SidebarPortalContent } from "@/components/shell/sidebar-portal-host";
import { DatabaseCatalogController } from "@/features/query/database-catalog-controller";
import { databaseListQuery } from "@/features/query/admin-query";
import { queryService } from "@/features/query/query-service";
import { type DatabaseAvailability, QueryWorkspace } from "@/features/query/query-workspace";
import { loadQueryWorkspace, type PersistedQueryTab } from "@/features/query/query-tabs";
import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import { QueryWorkspaceRegistry } from "@/features/query/query-workspace-registry";
import {
  createWorkspaceTab,
  removeWorkspaceTab,
  renameWorkspaceTab,
  updateWorkspaceTab,
} from "@/features/query/query-workspace-tabs";
import { getSession } from "@/shared/auth";
import { apiErrorMessage } from "@/shared/errors/api";

export default function QueryPage() {
  const [workspaceRegistryState] = state(new QueryWorkspaceRegistry());
  const workspaceRegistry = workspaceRegistryState();
  const user = getSession()?.user ?? "anonymous";
  const restored = loadQueryWorkspace(user);
  const [tabs, setTabs] = state<PersistedQueryTab[]>(restored.tabs);
  const [activeTabId, setActiveTabId] = state<string | null>(restored.activeTabId);
  const [dialogOpen, setDialogOpen] = state(false);
  const [createDatabaseOpen, setCreateDatabaseOpen] = state(false);
  const [persistenceFailed, setPersistenceFailed] = state(false);
  const [persistenceOperationId, setPersistenceOperationId] = state("");
  const [closeCandidate, setCloseCandidate] = state<PersistedQueryTab | null>(null);
  const [closeError, setCloseError] = state<string | null>(null);
  const [catalogRevision, setCatalogRevision] = state(0);
  const [persistenceState] = state(
    createQueryPersistenceCoordinator(user, (failed) => setPersistenceFailed(failed)),
  );
  const persistence = persistenceState();
  const [catalogController] = state(
    new DatabaseCatalogController(
      (database, options) => queryService.getSchema(database, options),
      () => setCatalogRevision((revision) => revision + 1),
    ),
  );
  const databaseCatalogs = catalogController();
  const databaseQuery = createQuery(databaseListQuery, {});
  catalogRevision();
  task(() => {
    const initialActiveDatabase = restored.tabs.find(
      (tab) => tab.id === restored.activeTabId,
    )?.database;
    if (initialActiveDatabase) void databaseCatalogs.activate(initialActiveDatabase);
    return () => {
      persistence.flush(persistenceOperationId() || undefined);
      databaseCatalogs.dispose();
    };
  });

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
    defer(() => focusQuery(tab.id));
    setDialogOpen(false);
  }

  function openNewQuery() {
    setDialogOpen(true);
  }

  function renameTab(id: string, title: string) {
    const currentTabs = tabs();
    const nextTabs = renameWorkspaceTab(currentTabs, id, title);
    if (nextTabs === currentTabs) return;
    persist(nextTabs, activeTabId());
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
    persist(tabs(), id);
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
    defer(() => {
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

      {tabs().length === 0 ? (
        <section class="cassie-query-empty" aria-labelledby="empty-workspace-title">
          <h1 id="empty-workspace-title">New Query</h1>
          <p>Choose a database to open a query workspace.</p>
          <Button type="button" variant="primary" onPress={openNewQuery}>
            New Query
          </Button>
        </section>
      ) : null}

      <For each={() => tabs()} by={(tab) => tab.id}>
        {(tab) => (
          <QueryWorkspace
            tab={tab}
            active={() => activeTabId() === tab.id}
            availability={() => databaseAvailability(tab.database)}
            onSqlChange={(sql) => updateSql(tab.id, sql)}
            onEditorSplitChange={(percent) => updateEditorSplit(tab.id, percent)}
            onRegister={(controller) => workspaceRegistry.registerController(tab.id, controller)}
            onCreateDatabase={openCreateDatabase}
            catalogs={catalogEntries}
            onSchemaChanged={(database) => databaseCatalogs.refresh(database)}
          />
        )}
      </For>

      <Show when={dialogOpen()}>
        <NewQueryDialog
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
