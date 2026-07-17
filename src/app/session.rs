use super::{
    normalize_role_name, Arc, BTreeMap, CassieError, Mutex, Serialize, TransactionIsolation,
};
use crate::catalog::DEFAULT_SCHEMA;

#[derive(Debug, Clone, Serialize)]
pub struct CassieSession {
    pub user: String,
    pub database: Option<String>,
    #[serde(skip)]
    access: SessionAccess,
    #[serde(skip)]
    search_path: Arc<Mutex<Vec<String>>>,
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
    isolation: Option<TransactionIsolation>,
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
    savepoints: Vec<SessionSavepoint>,
}

#[derive(Debug, Clone)]
struct SessionSavepoint {
    name: String,
    writes: SharedTransactionWrites,
    conflict_intents: Vec<TransactionConflictIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionTransactionStatus {
    Idle,
    InTransaction,
    Failed,
}

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

    #[must_use]
    pub(crate) fn estimated_retained_bytes(&self) -> usize {
        self.changes.iter().fold(0, |bytes, (id, change)| {
            bytes
                .saturating_add(std::mem::size_of::<(String, TransactionRowChange)>())
                .saturating_add(id.len())
                .saturating_add(match change {
                    TransactionRowChange::Upsert(payload) => json_retained_bytes(payload),
                    TransactionRowChange::Delete => 0,
                })
        })
    }
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let value_bytes = std::mem::size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            value_bytes
        }
        serde_json::Value::String(value) => value_bytes.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(value_bytes, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => {
            values.iter().fold(value_bytes, |bytes, (key, value)| {
                bytes
                    .saturating_add(std::mem::size_of::<String>())
                    .saturating_add(key.len())
                    .saturating_add(json_retained_bytes(value))
            })
        }
    }
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
            search_path: Arc::new(Mutex::new(vec![DEFAULT_SCHEMA.to_string()])),
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::Idle,
                isolation: None,
                writes: SharedTransactionWrites::default(),
                conflict_intents: Vec::new(),
                savepoints: Vec::new(),
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
            search_path: Arc::new(Mutex::new(self.search_path())),
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::InTransaction,
                isolation,
                writes: base_writes.clone(),
                conflict_intents,
                savepoints: Vec::new(),
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
        self.search_path()
            .into_iter()
            .next()
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

    pub(crate) fn begin_transaction(
        &self,
        isolation: Option<TransactionIsolation>,
    ) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::Idle {
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

        transaction.status = SessionTransactionStatus::InTransaction;
        transaction.isolation = isolation;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        Ok(())
    }

    pub(crate) fn commit_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
    }

    pub(crate) fn rollback_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
    }

    pub(crate) fn create_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "SAVEPOINT requires an active transaction".to_string(),
            ));
        }

        let writes = transaction.writes.clone();
        let conflict_intents = transaction.conflict_intents.clone();
        transaction.savepoints.push(SessionSavepoint {
            name: name.to_ascii_lowercase(),
            writes,
            conflict_intents,
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
        transaction.conflict_intents = conflict_intents;
        transaction.savepoints.truncate(index + 1);
        transaction.status = SessionTransactionStatus::InTransaction;
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

    pub(super) const fn has_explicit_transaction(&self) -> bool {
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
    use std::sync::Arc;

    use serde_json::json;

    use super::{CassieSession, TransactionRowChange};

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
        let retained_bytes = snapshot.estimated_retained_bytes();

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
