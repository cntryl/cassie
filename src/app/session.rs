use super::{
    normalize_role_name, Arc, BTreeMap, CassieError, Mutex, Serialize, TransactionIsolation,
};

#[derive(Debug, Clone, Serialize)]
pub struct CassieSession {
    pub user: String,
    pub database: Option<String>,
    #[serde(skip)]
    transaction: Arc<Mutex<SessionTransactionState>>,
    #[serde(skip)]
    procedure_calls: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone)]
struct SessionTransactionState {
    status: SessionTransactionStatus,
    isolation: Option<TransactionIsolation>,
    writes: BTreeMap<String, BTreeMap<String, TransactionRowChange>>,
    savepoints: Vec<SessionSavepoint>,
}

#[derive(Debug, Clone)]
struct SessionSavepoint {
    name: String,
    writes: BTreeMap<String, BTreeMap<String, TransactionRowChange>>,
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

impl CassieSession {
    #[must_use]
    pub fn new(user: String, database: Option<String>) -> Self {
        Self {
            user: normalize_role_name(user),
            database,
            transaction: Arc::new(Mutex::new(SessionTransactionState {
                status: SessionTransactionStatus::Idle,
                isolation: None,
                writes: BTreeMap::new(),
                savepoints: Vec::new(),
            })),
            procedure_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn transaction_status(&self) -> &'static str {
        match self.transaction.lock().status {
            SessionTransactionStatus::Idle => "idle",
            SessionTransactionStatus::InTransaction => "in_transaction",
            SessionTransactionStatus::Failed => "failed",
        }
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

        transaction.status = SessionTransactionStatus::InTransaction;
        transaction.isolation = isolation;
        transaction.writes.clear();
        transaction.savepoints.clear();
        Ok(())
    }

    pub(crate) fn commit_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes.clear();
        transaction.savepoints.clear();
    }

    pub(crate) fn rollback_transaction(&self) {
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.isolation = None;
        transaction.writes.clear();
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
        transaction.savepoints.push(SessionSavepoint {
            name: name.to_ascii_lowercase(),
            writes,
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
    ) {
        let mut transaction = self.transaction.lock();
        transaction
            .writes
            .entry(collection.to_string())
            .or_default()
            .insert(id, TransactionRowChange::Upsert(payload));
    }

    pub(crate) fn stage_document_delete(&self, collection: &str, id: String) {
        let mut transaction = self.transaction.lock();
        transaction
            .writes
            .entry(collection.to_string())
            .or_default()
            .insert(id, TransactionRowChange::Delete);
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
}
