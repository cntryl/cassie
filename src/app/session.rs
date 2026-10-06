#[path = "session/transaction_origin.rs"]
mod transaction_origin;

use super::session_settings::SessionSettings;
use super::{
    normalize_role_name, Arc, BTreeMap, CassieError, Mutex, Serialize, TransactionIsolation,
};
use crate::catalog::DEFAULT_SCHEMA;
use crate::runtime::accounted::json;
use std::sync::atomic::{AtomicI32, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct CassieSession {
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
    procedure_calls: Arc<Mutex<Vec<String>>>,
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
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
    savepoints: Vec<SessionSavepoint>,
    session_state: Option<SessionStateSnapshot>,
}

#[derive(Debug, Clone)]
struct SessionStateSnapshot {
    search_path: Vec<String>,
    settings: SessionSettings,
}

#[derive(Debug, Clone)]
struct SessionSavepoint {
    name: String,
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
    session_state: SessionStateSnapshot,
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
    base_writes: SharedTransactionWrites,
    base_conflict_intent_count: usize,
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
        // Charge a full internal BTree node per entry, including unused key/value slots.
        // An empty owned map can retain its root, so its estimate includes one node too.
        let node_bytes = 11
            * (std::mem::size_of::<String>() + std::mem::size_of::<TransactionRowChange>())
            + 16 * std::mem::size_of::<usize>();
        let container_bytes =
            std::mem::size_of::<CollectionChanges>() + 2 * std::mem::size_of::<usize>();
        let map_bytes = self
            .changes
            .len()
            .max(1)
            .checked_mul(node_bytes)
            .and_then(|bytes| bytes.checked_add(container_bytes))
            .ok_or_else(staged_snapshot_accounting_overflow)?;
        self.changes
            .iter()
            .try_fold(map_bytes, |bytes, (id, change)| {
                let payload_bytes = match change {
                    TransactionRowChange::Upsert(payload) => json::retained_bytes(payload)?,
                    TransactionRowChange::Delete => 0,
                };
                bytes
                    .checked_add(id.capacity())
                    .and_then(|bytes| bytes.checked_add(payload_bytes))
                    .ok_or_else(staged_snapshot_accounting_overflow)
            })
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
            user: normalize_role_name(user),
            database,
            access,
            schema_context: Arc::new(Mutex::new(None)),
            search_path: Arc::new(Mutex::new(vec![DEFAULT_SCHEMA.to_string()])),
            settings: Arc::new(Mutex::new(SessionSettings::default())),
            backend_pid: Arc::new(AtomicI32::new(0)),
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::Idle,
                implicit: false,
                isolation: None,
                writes: SharedTransactionWrites::default(),
                conflict_intents: Vec::new(),
                savepoints: Vec::new(),
                session_state: None,
            })),
            procedure_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn fork_statement_batch(&self) -> Result<StatementMutationBatch, CassieError> {
        let transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Failed {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }

        let base_transaction_active = transaction.status == SessionTransactionStatus::InTransaction;
        let (isolation, base_writes, conflict_intents) = if base_transaction_active {
            (
                transaction.isolation,
                transaction.writes.clone(),
                transaction.conflict_intents.clone(),
            )
        } else {
            (None, SharedTransactionWrites::default(), Vec::new())
        };
        drop(transaction);

        let base_conflict_intent_count = conflict_intents.len();
        let session = Self {
            user: self.user.clone(),
            database: self.database.clone(),
            access: self.access,
            schema_context: Arc::clone(&self.schema_context),
            search_path: Arc::new(Mutex::new(self.search_path())),
            settings: Arc::new(Mutex::new(self.settings.lock().clone())),
            backend_pid: Arc::clone(&self.backend_pid),
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::InTransaction,
                implicit: false,
                isolation,
                writes: base_writes.clone(),
                conflict_intents,
                savepoints: Vec::new(),
                session_state: None,
            })),
            procedure_calls: Arc::new(Mutex::new(self.procedure_calls.lock().clone())),
        };

        Ok(StatementMutationBatch {
            session,
            base_transaction_active,
            base_writes,
            base_conflict_intent_count,
        })
    }

    pub(crate) fn publish_statement_batch(
        &self,
        batch: &StatementMutationBatch,
    ) -> Result<(), CassieError> {
        let delta = batch.delta()?;
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "statement mutation requires an active transaction".to_string(),
            ));
        }

        for mutation in &delta.rows {
            let current = transaction
                .writes
                .get(&mutation.collection)
                .and_then(|writes| writes.get(&mutation.id));
            if current != mutation.before.as_ref() {
                return Err(CassieError::Execution(
                    "session transaction changed while statement was executing".to_string(),
                ));
            }
        }

        for mutation in delta.rows {
            if let Some(change) = mutation.after {
                let writes = Arc::make_mut(&mut transaction.writes);
                Arc::make_mut(writes.entry(mutation.collection).or_default())
                    .insert(mutation.id, change);
            } else {
                let writes = Arc::make_mut(&mut transaction.writes);
                let remove_collection =
                    writes.get_mut(&mutation.collection).is_some_and(|writes| {
                        let writes = Arc::make_mut(writes);
                        writes.remove(&mutation.id);
                        writes.is_empty()
                    });
                if remove_collection {
                    writes.remove(&mutation.collection);
                }
            }
        }
        transaction.conflict_intents.extend(delta.conflict_intents);
        Ok(())
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
        self.settings.lock().set(name, value)
    }

    pub(crate) fn set_backend_pid(&self, process_id: i32) {
        self.backend_pid.store(process_id, Ordering::Relaxed);
    }

    #[must_use]
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid.load(Ordering::Relaxed)
    }

    pub(crate) fn begin_transaction(
        &self,
        isolation: Option<TransactionIsolation>,
    ) -> Result<(), CassieError> {
        let session_state = self.session_state_snapshot();
        let mut transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Failed
            || (transaction.status != SessionTransactionStatus::Idle && !transaction.implicit)
        {
            return Err(CassieError::Unsupported(
                "transaction already in progress".to_string(),
            ));
        }
        if matches!(
            isolation,
            Some(TransactionIsolation::RepeatableRead | TransactionIsolation::Serializable)
        ) {
            return Err(CassieError::Unsupported(
                "only READ COMMITTED transaction isolation is supported".to_string(),
            ));
        }

        if transaction.implicit {
            transaction.implicit = false;
            transaction.isolation = isolation;
            return Ok(());
        }

        transaction.status = SessionTransactionStatus::InTransaction;
        transaction.implicit = false;
        transaction.isolation = isolation;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.session_state = Some(session_state);
        Ok(())
    }

    pub(crate) fn commit_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.session_state = None;
    }

    pub(crate) fn rollback_transaction(&self) {
        let mut transaction = self.transaction.lock();
        let session_state = transaction.session_state.take();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        if let Some(session_state) = session_state {
            self.restore_session_state(session_state);
        }
    }

    pub(crate) fn create_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "SAVEPOINT requires an active transaction".to_string(),
            ));
        }
        if transaction.savepoints.len() >= MAX_SAVEPOINTS_PER_TRANSACTION {
            return Err(CassieError::ResourceLimit(
                "savepoint limit exceeded".to_string(),
            ));
        }

        let writes = transaction.writes.clone();
        let conflict_intents = transaction.conflict_intents.clone();
        transaction.savepoints.push(SessionSavepoint {
            name: name.to_ascii_lowercase(),
            writes,
            conflict_intents,
            session_state: self.session_state_snapshot(),
        });
        Ok(())
    }

    pub(crate) fn rollback_to_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if !matches!(
            transaction.status,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed
        ) {
            return Err(CassieError::Execution(
                "ROLLBACK TO SAVEPOINT requires an active transaction".to_string(),
            ));
        }

        let normalized = name.to_ascii_lowercase();
        let Some(index) = transaction
            .savepoints
            .iter()
            .rposition(|savepoint| savepoint.name == normalized)
        else {
            return Err(CassieError::Execution(format!(
                "savepoint '{name}' does not exist"
            )));
        };

        transaction.writes = transaction.savepoints[index].writes.clone();
        let conflict_intents = transaction.savepoints[index].conflict_intents.clone();
        let session_state = transaction.savepoints[index].session_state.clone();
        transaction.conflict_intents = conflict_intents;
        transaction.savepoints.truncate(index + 1);
        transaction.status = SessionTransactionStatus::InTransaction;
        self.restore_session_state(session_state);
        Ok(())
    }

    pub(crate) fn release_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "RELEASE SAVEPOINT requires an active transaction".to_string(),
            ));
        }

        let normalized = name.to_ascii_lowercase();
        let Some(index) = transaction
            .savepoints
            .iter()
            .rposition(|savepoint| savepoint.name == normalized)
        else {
            return Err(CassieError::Execution(format!(
                "savepoint '{name}' does not exist"
            )));
        };

        transaction.savepoints.truncate(index);
        Ok(())
    }

    pub(crate) fn is_transaction_active(&self) -> bool {
        self.transaction.lock().status == SessionTransactionStatus::InTransaction
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
            .map(|changes| changes.as_ref().clone())
            .unwrap_or_default()
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
        StagedWriteSnapshot::matching(&self.transaction.lock().writes, collection)
    }

    #[must_use]
    pub(crate) fn has_collection_changes(&self, collection: &str) -> bool {
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

impl StatementMutationBatch {
    pub(crate) fn session(&self) -> &CassieSession {
        &self.session
    }

    pub(super) const fn has_enclosing_transaction(&self) -> bool {
        self.base_transaction_active
    }

    pub(super) fn into_session(self) -> CassieSession {
        self.session
    }

    fn delta(&self) -> Result<StatementMutationDelta, CassieError> {
        let transaction = self.session.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "statement mutation batch is not active".to_string(),
            ));
        }
        if transaction.conflict_intents.len() < self.base_conflict_intent_count {
            return Err(CassieError::Execution(
                "statement mutation changed prior conflict intents".to_string(),
            ));
        }

        let mut rows = Vec::new();
        for (collection, working_rows) in transaction.writes.iter() {
            let base_rows = self.base_writes.get(collection);
            for (id, change) in working_rows.iter() {
                let before = base_rows.and_then(|base| base.get(id));
                if before != Some(change) {
                    rows.push(StatementRowMutation {
                        collection: collection.clone(),
                        id: id.clone(),
                        before: before.cloned(),
                        after: Some(change.clone()),
                    });
                }
            }
        }
        for (collection, base_rows) in self.base_writes.iter() {
            let working_rows = transaction.writes.get(collection);
            for (id, change) in base_rows.iter() {
                if working_rows.is_none_or(|working| !working.contains_key(id)) {
                    rows.push(StatementRowMutation {
                        collection: collection.clone(),
                        id: id.clone(),
                        before: Some(change.clone()),
                        after: None,
                    });
                }
            }
        }

        Ok(StatementMutationDelta {
            rows,
            conflict_intents: transaction.conflict_intents[self.base_conflict_intent_count..]
                .to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::time::Instant;

    use serde_json::json;

    use crate::config::CassieRuntimeLimits;
    use crate::runtime::accounted::Accounted;
    use crate::runtime::QueryExecutionControls;

    use super::{
        BTreeMap, CassieError, CassieSession, CollectionChanges, StagedWriteSnapshot,
        TransactionRowChange,
    };

    #[test]
    fn should_include_staged_snapshot_btree_nodes_before_retaining_shared_changes() {
        // Arrange
        let snapshot = StagedWriteSnapshot {
            changes: Arc::new(BTreeMap::from([(
                "one".to_owned(),
                TransactionRowChange::Delete,
            )])),
        };
        let node_bytes = 11
            * (std::mem::size_of::<String>() + std::mem::size_of::<TransactionRowChange>())
            + 16 * std::mem::size_of::<usize>();
        let owned_container_bytes = std::mem::size_of::<CollectionChanges>()
            + 2 * std::mem::size_of::<usize>()
            + node_bytes
            + "one".len();

        // Act
        let estimated_bytes = snapshot
            .estimated_retained_bytes()
            .expect("snapshot retained estimate");

        // Assert
        assert!(
            estimated_bytes >= owned_container_bytes,
            "a retained snapshot owns BTree slots and its Arc allocation"
        );
    }

    #[test]
    fn should_reserve_sparse_staged_snapshot_json_before_retaining_owned_changes() {
        // Arrange
        let session = CassieSession::new("postgres".to_owned(), None);
        session.begin_transaction(None).expect("begin transaction");
        let mut payload = serde_json::Value::Null;
        for _ in 0..32 {
            payload = json!({"a": payload});
        }
        session
            .stage_document_write("items", "one".to_owned(), payload)
            .expect("stage sparse JSON");
        let snapshot = session.staged_write_snapshot("items");
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 16 * 1_024,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let retained_calls = Cell::new(0);

        // Act
        let result = Accounted::try_new(
            &controls,
            snapshot
                .estimated_retained_bytes()
                .expect("snapshot retained estimate"),
            || {
                retained_calls.set(retained_calls.get() + 1);
                snapshot.shared_changes()
            },
        );

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(
            retained_calls.get(),
            0,
            "reserve the complete staged snapshot"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_staged_write_snapshot_immutable_when_session_changes() {
        // Arrange
        let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
        session.begin_transaction(None).expect("begin transaction");
        session
            .stage_document_write(
                "cassie.public.snapshot_items",
                "item-a".to_string(),
                json!({"value": "before"}),
            )
            .expect("stage initial row");
        let snapshot = session.staged_write_snapshot("snapshot_items");
        let shared_changes = snapshot.shared_changes();
        let retained_bytes = snapshot
            .estimated_retained_bytes()
            .expect("snapshot retained estimate");

        // Act
        session
            .stage_document_write(
                "cassie.public.snapshot_items",
                "item-b".to_string(),
                json!({"value": "after"}),
            )
            .expect("stage later row");
        session
            .stage_document_delete("cassie.public.snapshot_items", "item-a".to_string())
            .expect("stage later delete");
        let updated = session.staged_write_snapshot("snapshot_items");

        // Assert
        assert!(Arc::ptr_eq(&shared_changes, &snapshot.shared_changes()));
        assert!(retained_bytes > "item-a".len());
        assert!(matches!(
            snapshot.ordered_changes().get("item-a"),
            Some(TransactionRowChange::Upsert(payload))
                if payload == &json!({"value": "before"})
        ));
        assert!(!snapshot.ordered_changes().contains_key("item-b"));
        assert!(matches!(
            updated.ordered_changes().get("item-a"),
            Some(TransactionRowChange::Delete)
        ));
        assert!(updated.ordered_changes().contains_key("item-b"));
        assert!(session.has_collection_changes("snapshot_items"));
    }

    #[test]
    fn should_preserve_cow_snapshots_across_savepoint_rollback() {
        // Arrange
        let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
        session.begin_transaction(None).expect("begin transaction");
        session
            .stage_document_write(
                "snapshot_savepoint_items",
                "item-a".to_string(),
                json!({"value": "kept"}),
            )
            .expect("stage row before savepoint");
        session
            .create_savepoint("before_more")
            .expect("create savepoint");
        let before_more = session.staged_write_snapshot("snapshot_savepoint_items");
        session
            .stage_document_write(
                "snapshot_savepoint_items",
                "item-b".to_string(),
                json!({"value": "rolled back"}),
            )
            .expect("stage row after savepoint");
        let after_more = session.staged_write_snapshot("snapshot_savepoint_items");

        // Act
        session
            .rollback_to_savepoint("before_more")
            .expect("rollback to savepoint");
        let restored = session.staged_write_snapshot("snapshot_savepoint_items");

        // Assert
        assert!(before_more.ordered_changes().contains_key("item-a"));
        assert!(!before_more.ordered_changes().contains_key("item-b"));
        assert!(after_more.ordered_changes().contains_key("item-b"));
        assert_eq!(restored.ordered_changes(), before_more.ordered_changes());
    }

    #[test]
    fn should_report_no_changes_for_an_unmatched_collection_snapshot() {
        // Arrange
        let session = CassieSession::new("postgres".to_string(), Some("cassie".to_string()));
        session.begin_transaction(None).expect("begin transaction");
        session
            .stage_document_write(
                "cassie.public.snapshot_items",
                "item-a".to_string(),
                json!({"value": "present"}),
            )
            .expect("stage row");

        // Act
        let snapshot = session.staged_write_snapshot("other_items");

        // Assert
        assert!(snapshot.is_empty());
        assert!(!session.has_collection_changes("other_items"));
    }
}
