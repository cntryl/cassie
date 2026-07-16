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
    writes: BTreeMap<String, BTreeMap<String, TransactionRowChange>>,
    conflict_intents: Vec<TransactionConflictIntent>,
    savepoints: Vec<SessionSavepoint>,
}

#[derive(Debug, Clone)]
struct SessionSavepoint {
    name: String,
    writes: BTreeMap<String, BTreeMap<String, TransactionRowChange>>,
    conflict_intents: Vec<TransactionConflictIntent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionTransactionStatus {
    Idle,
    InTransaction,
    Failed,
}

#[derive(Debug, Clone)]
pub(crate) enum TransactionRowChange {
    Upsert(serde_json::Value),
    Delete,
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
                writes: BTreeMap::new(),
                conflict_intents: Vec::new(),
                savepoints: Vec::new(),
            })),
            procedure_calls: Arc::new(Mutex::new(Vec::new())),
        }
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
        transaction.writes.clear();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        Ok(())
    }

    pub(crate) fn commit_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes.clear();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
    }

    pub(crate) fn rollback_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes.clear();
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
        transaction
            .writes
            .entry(collection.to_string())
            .or_default()
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
        transaction
            .writes
            .entry(collection.to_string())
            .or_default()
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
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn transaction_writes(
        &self,
    ) -> BTreeMap<String, BTreeMap<String, TransactionRowChange>> {
        self.transaction.lock().writes.clone()
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
        let Some(writes) = transaction.writes.get_mut(collection) else {
            return;
        };
        writes.remove(id);
        if writes.is_empty() {
            transaction.writes.remove(collection);
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
