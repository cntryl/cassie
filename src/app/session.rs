mod bound_read;
pub(crate) use bound_read::{BoundReadScope, BoundReadView};
mod catalog_overlay;
mod copy_owner;
pub(crate) use copy_owner::{CopyBindingContext, CopyCommandOwner};
mod command_catalog;
pub(crate) use command_catalog::CommandCatalogScope;
mod savepoints;
mod transaction_snapshot;
use transaction_snapshot::TransactionReadSnapshot;

mod statement_batch;
mod statement_read;
mod statement_text;
pub(crate) use statement_read::{SessionReadScope, StatementOverlay};

mod owned_setting;
#[path = "session/random_state.rs"]
mod random_state;
pub(crate) use random_state::SessionRandom;
mod setting_control;
mod sql_objects;
pub(crate) use sql_objects::{
    CursorOptions, SessionCursor, SessionObjects, SessionPrepared, MAX_CURSORS_PER_SESSION,
    MAX_PREPARED_PER_SESSION,
};
mod temporal_context;
pub(crate) use temporal_context::setting_zone;
mod transaction_control;
#[path = "session/transaction_origin.rs"]
mod transaction_origin;

use super::session_settings::SessionSettings;
use super::{
    normalize_role_name, Arc, BTreeMap, CassieError, Mutex, Serialize, TransactionIsolation,
};
use crate::catalog::{Catalog, DEFAULT_SCHEMA};
use crate::midge::adapter::StorageOverlay;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicI32, Ordering};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct CassieSession {
    #[serde(skip)]
    statement_text: Option<Arc<statement_text::StatementText>>,
    pub user: String,
    pub database: Option<String>,
    #[serde(skip)]
    access: SessionAccess,
    #[serde(skip)]
    pub(super) schema_context: Arc<Mutex<Option<super::session_schema::SessionSchemaContext>>>,
    #[serde(skip)]
    search_path: Arc<Mutex<Vec<String>>>,
    #[serde(skip)]
    settings: Arc<Mutex<SessionSettings>>,
    #[serde(skip)]
    backend_pid: Arc<AtomicI32>,
    #[serde(skip)]
    transaction: Arc<Mutex<SessionTransactionState>>,
    #[serde(skip)]
    transaction_unique_waits: Arc<Mutex<Option<Arc<super::TransactionUniqueWaits>>>>,
    #[serde(skip)]
    owns_transaction_lifecycle: bool,
    #[serde(skip)]
    procedure_calls: Arc<Mutex<Vec<String>>>,
    #[serde(skip)]
    pub(super) sequence_values: Arc<Mutex<super::session_sequences::SessionSequenceValues>>,
    #[serde(skip)]
    random: Arc<Mutex<SessionRandom>>,
    /// SQL and protocol prepared statements and open cursors.
    #[serde(skip)]
    session_objects: Arc<Mutex<SessionObjects>>,
    /// The transaction generation and its `transaction_timestamp()`.
    #[serde(skip)]
    transaction_clock: Arc<Mutex<Option<(u64, i64)>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionAccess {
    TrustedEmbedded,
    AuthenticatedAdmin,
    AuthenticatedReadOnly,
}

#[derive(Debug, Clone)]
struct SessionTransactionState {
    status: SessionTransactionStatus,
    implicit: bool,
    isolation: Option<TransactionIsolation>,
    modes: crate::sql::ast::TransactionModes,
    has_query: bool,
    generation: u64,
    repeatable_read: Option<Arc<TransactionReadSnapshot>>,
    unique_owner_id: Uuid,
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
    savepoints: Vec<SessionSavepoint>,
    catalog_overlay: Option<Catalog>,
    catalog_base_version: Option<u64>,
    catalog_views: catalog_overlay::TransactionCatalogViews,
    storage_overlay: Arc<Mutex<StorageOverlay>>,
    session_state: Option<SessionStateSnapshot>,
    /// Settings changed by `SET LOCAL`, restored when the transaction ends.
    local_settings: Vec<String>,
}

#[derive(Debug, Clone)]
struct SessionStateSnapshot {
    search_path: Vec<String>,
    settings: SessionSettings,
}

#[derive(Debug, Clone)]
struct SessionSavepoint {
    name: String,
    /// Cursors declared after this mark close on rollback to the savepoint.
    object_mark: u64,
    modes: crate::sql::ast::TransactionModes,
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
    catalog_overlay: Option<Catalog>,
    catalog_base_version: Option<u64>,
    catalog_views: catalog_overlay::TransactionCatalogViews,
    storage_overlay: StorageOverlay,
    session_state: SessionStateSnapshot,
    unique_keys: BTreeSet<super::UniqueTransactionKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionTransactionStatus {
    Idle,
    InTransaction,
    Failed,
}

const MAX_SAVEPOINTS_PER_TRANSACTION: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TransactionRowChange {
    Upsert(serde_json::Value),
    Delete,
}

type CollectionChanges = BTreeMap<String, TransactionRowChange>;
type SharedCollectionChanges = Arc<CollectionChanges>;
type TransactionWrites = BTreeMap<String, SharedCollectionChanges>;
type SharedTransactionWrites = Arc<TransactionWrites>;

#[derive(Debug, Clone, Default)]
pub(crate) struct StagedWriteSnapshot {
    changes: SharedCollectionChanges,
}

#[derive(Debug)]
pub(crate) struct StatementMutationBatch {
    session: CassieSession,
    base_transaction_active: bool,
    base_generation: u64,
    base_writes: SharedTransactionWrites,
    base_conflict_intent_count: usize,
    base_catalog_overlay: Option<Catalog>,
    base_catalog_base_version: Option<u64>,
    base_storage_overlay: StorageOverlay,
}

#[derive(Debug)]
struct StatementRowMutation {
    collection: String,
    id: String,
    before: Option<TransactionRowChange>,
    after: Option<TransactionRowChange>,
}

#[derive(Debug)]
struct StatementMutationDelta {
    rows: Vec<StatementRowMutation>,
    conflict_intents: Vec<TransactionConflictIntent>,
}

#[derive(Debug, Clone)]
pub(crate) struct TransactionConflictIntent {
    pub(crate) provisional_id: String,
    pub(crate) statement: crate::sql::ast::InsertStatement,
    pub(crate) payload: serde_json::Value,
    pub(crate) params: Vec<crate::types::Value>,
    pub(crate) user_functions: std::collections::HashMap<String, crate::catalog::FunctionMeta>,
    pub(crate) schema: crate::catalog::CollectionSchema,
}

impl StagedWriteSnapshot {
    #[must_use]
    fn matching(writes: &TransactionWrites, collection: &str) -> Self {
        writes
            .iter()
            .find(|(name, _)| crate::catalog::name_matches(name, collection))
            .map_or_else(Self::default, |(_, changes)| Self {
                changes: Arc::clone(changes),
            })
    }

    #[must_use]
    pub(crate) fn ordered_changes(&self) -> &CollectionChanges {
        &self.changes
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    #[must_use]
    pub(crate) fn shared_changes(&self) -> Arc<CollectionChanges> {
        Arc::clone(&self.changes)
    }

    pub(crate) fn estimated_retained_bytes(&self) -> Result<usize, CassieError> {
        statement_read::collection_snapshot_bytes(&self.changes)
    }
}

fn staged_snapshot_accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("staged snapshot retained memory accounting overflow".to_owned())
}

impl CassieSession {
    #[must_use]
    pub fn new(user: String, database: Option<String>) -> Self {
        Self::with_access(user, database, SessionAccess::TrustedEmbedded)
    }

    pub(crate) fn authenticated(user: String, database: Option<String>, is_admin: bool) -> Self {
        let access = if is_admin {
            SessionAccess::AuthenticatedAdmin
        } else {
            SessionAccess::AuthenticatedReadOnly
        };
        Self::with_access(user, database, access)
    }

    fn with_access(user: String, database: Option<String>, access: SessionAccess) -> Self {
        Self {
            statement_text: None,
            user: normalize_role_name(user),
            database,
            access,
            schema_context: Arc::new(Mutex::new(None)),
            search_path: Arc::new(Mutex::new(vec![DEFAULT_SCHEMA.to_string()])),
            settings: Arc::new(Mutex::new(SessionSettings::default())),
            backend_pid: Arc::new(AtomicI32::new(0)),
            sequence_values: Arc::default(),
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::Idle,
                implicit: false,
                isolation: None,
                modes: crate::sql::ast::TransactionModes::default(),
                has_query: false,
                generation: 0,
                repeatable_read: None,
                unique_owner_id: Uuid::new_v4(),
                writes: SharedTransactionWrites::default(),
                conflict_intents: Vec::new(),
                savepoints: Vec::new(),
                catalog_overlay: None,
                catalog_base_version: None,
                catalog_views: catalog_overlay::TransactionCatalogViews::default(),
                storage_overlay: Arc::new(Mutex::new(StorageOverlay::default())),
                session_state: None,
                local_settings: Vec::new(),
            })),
            transaction_unique_waits: Arc::default(),
            owns_transaction_lifecycle: true,
            procedure_calls: Arc::new(Mutex::new(Vec::new())),
            random: Arc::new(Mutex::new(SessionRandom::default())),
            session_objects: Arc::default(),
            transaction_clock: Arc::default(),
        }
    }

    pub(crate) fn attach_transaction_unique_waits(
        &self,
        waits: Arc<super::TransactionUniqueWaits>,
    ) {
        *self.transaction_unique_waits.lock() = Some(waits);
    }

    pub(crate) fn transaction_unique_waits(&self) -> Option<Arc<super::TransactionUniqueWaits>> {
        self.transaction_unique_waits.lock().clone()
    }

    pub(crate) fn transaction_unique_owner_id(&self) -> Uuid {
        self.transaction.lock().unique_owner_id
    }

    /// Runs `operation` against this session's `random()` sequence.
    pub(crate) fn with_random<T>(&self, operation: impl FnOnce(&mut SessionRandom) -> T) -> T {
        operation(&mut self.random.lock())
    }

    pub(crate) fn is_authenticated_read_only(&self) -> bool {
        self.access == SessionAccess::AuthenticatedReadOnly
    }

    pub(crate) fn is_network_authenticated(&self) -> bool {
        self.access != SessionAccess::TrustedEmbedded
    }

    #[must_use]
    pub fn transaction_status(&self) -> &'static str {
        match self.transaction.lock().status {
            SessionTransactionStatus::Idle => "idle",
            SessionTransactionStatus::InTransaction => "in_transaction",
            SessionTransactionStatus::Failed => "failed",
        }
    }

    #[must_use]
    pub fn current_database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    #[must_use]
    pub fn current_schema(&self) -> String {
        self.current_existing_schema()
            .unwrap_or_else(|| DEFAULT_SCHEMA.to_string())
    }

    #[must_use]
    pub fn search_path(&self) -> Vec<String> {
        let mut path = self.search_path.lock().clone();
        if path.is_empty() {
            path.push(DEFAULT_SCHEMA.to_string());
        }
        path
    }

    pub fn set_search_path(&self, path: Vec<String>) {
        let normalized = if path.is_empty() {
            vec![DEFAULT_SCHEMA.to_string()]
        } else {
            path
        };
        *self.search_path.lock() = normalized;
    }

    fn session_state_snapshot(&self) -> SessionStateSnapshot {
        SessionStateSnapshot {
            search_path: self.search_path(),
            settings: self.settings.lock().clone(),
        }
    }

    fn restore_session_state(&self, state: SessionStateSnapshot) {
        *self.search_path.lock() = state.search_path;
        *self.settings.lock() = state.settings;
    }

    /// Returns the normalized value of a supported session setting.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter error when `name` is not in Cassie's
    /// `PostgreSQL` compatibility registry.
    pub fn setting(&self, name: &str) -> Result<String, CassieError> {
        if name.trim().eq_ignore_ascii_case("search_path") {
            return Ok(self.search_path().join(", "));
        }
        if let Some(value) = self.transaction_setting(name) {
            return Ok(value.to_string());
        }
        self.settings.lock().get(name)
    }

    pub(crate) fn settings_fingerprint(&self) -> u64 {
        crate::runtime::stable_fingerprint(&*self.settings.lock())
    }

    /// Validates and applies a supported session setting.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameter error for unknown settings or values that
    /// are incompatible with Cassie's fixed `PostgreSQL` compatibility values.
    pub fn set_setting(&self, name: &str, value: &str) -> Result<String, CassieError> {
        if name.trim().eq_ignore_ascii_case("transaction_read_only") {
            return self.set_transaction_read_only(value);
        }
        self.settings.lock().set(name, value)
    }

    pub(crate) fn set_backend_pid(&self, process_id: i32) {
        self.backend_pid.store(process_id, Ordering::Relaxed);
    }

    #[must_use]
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid.load(Ordering::Relaxed)
    }

    pub(crate) fn is_transaction_active(&self) -> bool {
        self.transaction.lock().status == SessionTransactionStatus::InTransaction
    }

    pub(crate) fn rename_transaction_collections(
        &self,
        renames: &[(String, String)],
    ) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Ok(());
        }

        for (current, next) in renames {
            let writes = Arc::make_mut(&mut transaction.writes);
            if let Some(changes) = writes.remove(current) {
                let mut merged = changes.as_ref().clone();
                if let Some(existing) = writes.remove(next) {
                    for (id, change) in existing.iter() {
                        merged.entry(id.clone()).or_insert_with(|| change.clone());
                    }
                }
                writes.insert(next.clone(), Arc::new(merged));
            }

            for intent in &mut transaction.conflict_intents {
                if crate::catalog::name_matches(&intent.schema.collection, current) {
                    intent.schema.collection.clone_from(next);
                    intent.statement.table =
                        crate::sql::ast::IdentifierPath::parse(next).map_err(CassieError::Parse)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn rename_transaction_column(&self, collection: &str, current: &str, next: &str) {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return;
        }

        let current_key = crate::sql::ColumnIdentifierPath::stored_field_key(current);
        let writes = Arc::make_mut(&mut transaction.writes);
        for (name, changes) in writes.iter_mut() {
            if !crate::catalog::name_matches(name, collection) {
                continue;
            }
            for change in Arc::make_mut(changes).values_mut() {
                if let TransactionRowChange::Upsert(payload) = change {
                    rename_payload_field(payload, &current_key, next);
                }
            }
        }

        for intent in &mut transaction.conflict_intents {
            if !crate::catalog::name_matches(&intent.schema.collection, collection) {
                continue;
            }

            rename_payload_field(&mut intent.payload, &current_key, next);
            for field in &mut intent.schema.fields {
                if crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == current_key {
                    next.clone_into(&mut field.name);
                }
            }
            for column in &mut intent.statement.columns {
                rename_column_reference(column, &current_key, next);
            }
            if let Some(conflict) = &mut intent.statement.on_conflict {
                for field in &mut conflict.target_fields {
                    rename_column_reference(field, &current_key, next);
                }
                for expression in &mut conflict.target_expressions {
                    rename_expression_column(expression, &current_key, next);
                }
                if let Some(predicate) = &mut conflict.target_predicate {
                    rename_expression_column(predicate, &current_key, next);
                }
                if let crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } = &mut conflict.action
                {
                    for (field, expression) in assignments {
                        rename_column_reference(field, &current_key, next);
                        rename_expression_column(expression, &current_key, next);
                    }
                    if let Some(filter) = filter {
                        rename_expression_column(filter, &current_key, next);
                    }
                }
            }
        }
    }

    pub(crate) fn is_transaction_failed(&self) -> bool {
        self.transaction.lock().status == SessionTransactionStatus::Failed
    }

    pub(crate) fn mark_transaction_failed(&self) {
        let mut transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::InTransaction {
            transaction.status = SessionTransactionStatus::Failed;
        }
    }

    pub(crate) fn preflight_transaction_collections(
        &self,
        collections: &[String],
    ) -> Result<(), CassieError> {
        if collections
            .iter()
            .any(|collection| self.collection_is_cross_database(collection))
        {
            self.mark_transaction_failed();
            return Err(CassieError::Unsupported(
                "cross-database transactions are not supported".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn enter_procedure_call(&self, name: &str) -> Result<(), CassieError> {
        let mut procedure_calls = self.procedure_calls.lock();
        let normalized = name.to_ascii_lowercase();
        if procedure_calls.iter().any(|entry| entry == &normalized) {
            return Err(CassieError::Execution(format!(
                "procedure '{name}' is recursively invoked"
            )));
        }

        procedure_calls.push(normalized);
        Ok(())
    }

    pub(crate) fn leave_procedure_call(&self) {
        let mut procedure_calls = self.procedure_calls.lock();
        procedure_calls.pop();
    }

    pub(crate) fn stage_document_write(
        &self,
        collection: &str,
        id: String,
        payload: serde_json::Value,
    ) -> Result<(), CassieError> {
        self.ensure_transaction_writable()?;
        self.preflight_transaction_collections(&[collection.to_string()])?;
        let mut transaction = self.transaction.lock();
        let writes = Arc::make_mut(&mut transaction.writes);
        Arc::make_mut(writes.entry(collection.to_string()).or_default())
            .insert(id, TransactionRowChange::Upsert(payload));
        Ok(())
    }

    pub(crate) fn stage_document_delete(
        &self,
        collection: &str,
        id: String,
    ) -> Result<(), CassieError> {
        self.ensure_transaction_writable()?;
        self.preflight_transaction_collections(&[collection.to_string()])?;
        let mut transaction = self.transaction.lock();
        let writes = Arc::make_mut(&mut transaction.writes);
        Arc::make_mut(writes.entry(collection.to_string()).or_default())
            .insert(id, TransactionRowChange::Delete);
        Ok(())
    }

    pub(crate) fn document_change(
        &self,
        collection: &str,
        id: &str,
    ) -> Option<TransactionRowChange> {
        self.transaction
            .lock()
            .writes
            .get(collection)
            .and_then(|collection_writes| collection_writes.get(id).cloned())
    }

    pub(crate) fn collection_changes(
        &self,
        collection: &str,
    ) -> BTreeMap<String, TransactionRowChange> {
        self.transaction
            .lock()
            .writes
            .get(collection)
            .map_or_default(|changes| changes.as_ref().clone())
    }

    pub(crate) fn collection_changes_matching(
        &self,
        collection: &str,
    ) -> BTreeMap<String, TransactionRowChange> {
        self.staged_write_snapshot(collection)
            .ordered_changes()
            .clone()
    }

    #[must_use]
    pub(crate) fn staged_write_snapshot(&self, collection: &str) -> StagedWriteSnapshot {
        if let Some(snapshot) = statement_read::captured_snapshot(self, collection) {
            return snapshot;
        }
        StagedWriteSnapshot::matching(&self.transaction.lock().writes, collection)
    }

    #[must_use]
    pub(crate) fn has_collection_changes(&self, collection: &str) -> bool {
        if let Some(changed) = self.captured_has_collection_changes(collection) {
            return changed;
        }
        self.transaction
            .lock()
            .writes
            .iter()
            .any(|(name, changes)| {
                !changes.is_empty() && crate::catalog::name_matches(name, collection)
            })
    }

    pub(crate) fn transaction_write_collections(&self) -> Vec<String> {
        self.transaction.lock().writes.keys().cloned().collect()
    }

    pub(crate) fn transaction_writes(
        &self,
    ) -> BTreeMap<String, BTreeMap<String, TransactionRowChange>> {
        self.transaction
            .lock()
            .writes
            .iter()
            .map(|(collection, changes)| (collection.clone(), changes.as_ref().clone()))
            .collect()
    }

    pub(crate) fn apply_literal_default_to_staged_rows(
        &self,
        collection: &str,
        field: &str,
        default_value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        if !self.is_transaction_active() || default_value.is_null() {
            return Ok(());
        }

        for (staged_collection, changes) in self.transaction_writes() {
            if !crate::catalog::name_matches(&staged_collection, collection) {
                continue;
            }
            for (id, change) in changes {
                let TransactionRowChange::Upsert(mut payload) = change else {
                    continue;
                };
                let Some(object) = payload.as_object_mut() else {
                    return Err(CassieError::Execution(format!(
                        "staged row '{id}' in '{staged_collection}' is not a JSON object"
                    )));
                };
                if object.contains_key(field) {
                    continue;
                }
                object.insert(field.to_owned(), default_value.clone());
                self.stage_document_write(&staged_collection, id, payload)?;
            }
        }
        Ok(())
    }

    pub(crate) fn stage_conflict_intent(&self, intent: TransactionConflictIntent) {
        self.transaction.lock().conflict_intents.push(intent);
    }

    pub(crate) fn transaction_conflict_intents(&self) -> Vec<TransactionConflictIntent> {
        self.transaction.lock().conflict_intents.clone()
    }

    pub(crate) fn clear_conflict_intents(&self) {
        self.transaction.lock().conflict_intents.clear();
    }

    pub(crate) fn remove_document_change(&self, collection: &str, id: &str) {
        let mut transaction = self.transaction.lock();
        let transaction_writes = Arc::make_mut(&mut transaction.writes);
        let Some(writes) = transaction_writes.get_mut(collection) else {
            return;
        };
        let writes = Arc::make_mut(writes);
        writes.remove(id);
        if writes.is_empty() {
            transaction_writes.remove(collection);
        }
    }

    fn collection_is_cross_database(&self, collection: &str) -> bool {
        let Some(current_database) = self.current_database() else {
            return false;
        };
        crate::catalog::relation_database_name(collection)
            .is_some_and(|database| !database.eq_ignore_ascii_case(current_database))
    }
}

impl Drop for CassieSession {
    fn drop(&mut self) {
        if !self.owns_transaction_lifecycle || Arc::strong_count(&self.transaction) != 1 {
            return;
        }
        let owner = self.transaction.lock().unique_owner_id;
        if let Some(waits) = self.transaction_unique_waits() {
            waits.release_owner(owner);
        }
    }
}

fn rename_payload_field(payload: &mut serde_json::Value, current_key: &str, next: &str) {
    let Some(fields) = payload.as_object_mut() else {
        return;
    };
    let Some(current) = fields
        .keys()
        .find(|field| crate::sql::ColumnIdentifierPath::stored_field_key(field) == current_key)
        .cloned()
    else {
        return;
    };
    if let Some(value) = fields.remove(&current) {
        fields.insert(next.to_owned(), value);
    }
}

fn rename_column_reference(column: &mut String, current_key: &str, next: &str) {
    if crate::sql::ColumnIdentifierPath::reference_field_key(column) == current_key {
        next.clone_into(column);
    }
}

fn rename_expression_column(expression: &mut crate::sql::ast::Expr, current_key: &str, next: &str) {
    if let crate::sql::ast::Expr::Column(column) = expression {
        rename_column_reference(column, current_key, next);
        return;
    }
    let _ = expression.try_visit_children_mut(|child| {
        rename_expression_column(child, current_key, next);
        Ok::<(), std::convert::Infallible>(())
    });
}

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
