use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};

use crate::catalog::{
    normalize_role_name, Catalog, CollectionSchema, ConstraintCheck, ConstraintOperator,
    FieldConstraint, RoleMeta,
};
use crate::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiCompatibleRuntimeConfig,
    OpenAiRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
};
use crate::embeddings::{
    cohere::CohereProvider,
    compatible::{OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig},
    local::LocalProvider,
    ollama::{OllamaProvider, OllamaProviderConfig},
    openai::{OpenAiProvider, OpenAiProviderConfig},
    tei::{TeiProvider, TeiProviderConfig},
    voyage::VoyageProvider,
    DistanceMetric, Embedding, EmbeddingError, EmbeddingProvider, NormalizedVectorRecord,
    VectorIndexRecord, VectorIndexType,
};
use crate::executor::{
    vector_prefilter_fallback_reason, vector_prefilter_supported, ColumnMeta, QueryError,
    QueryResult,
};
use crate::midge::adapter::{DocumentRef, Midge, MidgeScanTimings, RowDecode, RowFilter};
use crate::query_cache;
use crate::runtime::{
    ExecutionMode, PlanCacheKey, QueryExecutionControls, RuntimeFeedbackKey,
    RuntimeFeedbackObservation, RuntimeState,
};
use crate::sql::ast::{
    QueryStatement, TransactionAction, TransactionIsolation, TransactionStatement,
};
use crate::sql::{binder, parser};
use crate::types::{Value, Vector};
use crate::vector::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target,
    normalize as normalize_vector,
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

#[derive(Debug, Clone, Serialize)]
pub struct CassieRuntimeConfigState {
    pub pgwire_listen: String,
    pub rest_listen: String,
}

#[derive(Clone)]
pub struct Cassie {
    pub midge: Arc<Midge>,
    pub catalog: Catalog,
    pub embedding_provider: Arc<dyn EmbeddingProvider>,
    pub(crate) runtime: Arc<RuntimeState>,
    pub(crate) auth_user: String,
    pub(crate) auth_password: String,
    pub(crate) default_database: String,
    pub started: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
pub enum CassieError {
    #[error("collection not found: {0}")]
    CollectionNotFound(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("planner error: {0}")]
    Planner(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("invalid vector: {0}")]
    InvalidVector(String),

    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),

    #[error("embedding unavailable: {0}")]
    EmbeddingUnavailable(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unsupported feature: {0}")]
    Unsupported(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("storage bootstrap error: {0}")]
    StorageBootstrap(String),

    #[error("storage family missing: {0}")]
    StorageMissingFamily(String),

    #[error("temporary storage unavailable: {0}")]
    StorageRetryable(String),
}

pub(crate) fn unsupported_sql_error(sql: &str) -> Option<CassieError> {
    let keyword = sql.split_whitespace().next()?;
    let keyword = keyword.trim_matches(|character: char| !character.is_ascii_alphabetic());
    let keyword = keyword.to_ascii_uppercase();

    match keyword.as_str() {
        "COPY" | "LISTEN" | "NOTIFY" | "UNLISTEN" => Some(CassieError::Unsupported(format!(
            "{keyword} is not supported"
        ))),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum PlanCacheProvenance {
    L1 {
        durable: bool,
        candidate_expires_at_ms: Option<u64>,
    },
    L2,
    Compiled,
}

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

impl Cassie {
    pub fn new() -> Result<Self, CassieError> {
        let data_dir = std::env::var("CASSIE_MIDGE_DATA_DIR")
            .unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env())
    }

    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        Self::new_with_data_dir_and_config(data_dir, CassieRuntimeConfig::from_env())
    }

    pub fn new_with_data_dir_and_config(
        data_dir: impl AsRef<Path>,
        runtime_config: CassieRuntimeConfig,
    ) -> Result<Self, CassieError> {
        let midge = Arc::new(Midge::new_with_data_dir(data_dir.as_ref())?);
        let embedding_provider = build_embedding_provider(&runtime_config)?;
        let runtime = Arc::new(RuntimeState::new(runtime_config.limits.clone()));
        let auth_user = runtime_config.user.clone();
        let auth_password = runtime_config.password.clone();
        let default_database = runtime_config.database.clone();
        Ok(Self {
            midge,
            catalog: Catalog::new(),
            embedding_provider,
            runtime,
            auth_user,
            auth_password,
            default_database,
            started: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn hydrate_catalog(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        self.catalog.clear();
        self.invalidate_plan_cache();

        let namespaces = self.midge.list_namespaces();
        self.runtime.record_storage_access("schema", false, true);
        for namespace in namespaces {
            self.catalog.register_namespace(&namespace, None);
        }

        let mut collections = self.midge.list_collections();
        self.runtime.record_storage_access("schema", false, true);
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema();
            self.runtime.record_storage_access("schema", false, true);
        }

        for name in collections {
            self.runtime.record_storage_access("schema", false, true);
            if let Some(schema) = self.midge.collection_schema(&name) {
                let constraints = self.midge.load_constraints(&name).map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!(
                        "load constraints for collection '{name}': {error}"
                    ))
                })?;
                self.runtime.record_storage_access("schema", false, true);
                self.catalog.register_collection_with_constraints(
                    &name,
                    schema
                        .fields
                        .into_iter()
                        .map(|field| (field.name, field.data_type))
                        .collect(),
                    constraints,
                );
                let projection_metadata = self
                    .midge
                    .projection_metadata(&name)?
                    .unwrap_or_else(|| crate::catalog::ProjectionMeta::new(&name, 1));
                self.catalog
                    .register_projection_metadata(projection_metadata);
            }
        }

        let indexes = self.midge.list_vector_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list vector indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_vector_index(index.clone());
            self.midge.rebuild_normalized_vectors_for_index(&index)?;
        }

        let indexes = self.midge.list_indexes().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_index(index);
        }

        for collection in self.catalog.list_collections() {
            self.hydrate_cardinality_stats(&collection.name)?;
        }

        let functions = self.midge.list_functions().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list functions: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in functions {
            self.catalog.register_function(metadata);
        }

        let procedures = self.midge.list_procedures().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list procedures: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in procedures {
            self.catalog.register_procedure(metadata);
        }

        let views = self.midge.list_views().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list views: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in views {
            self.catalog.register_view(metadata);
        }

        self.hydrate_roles()?;
        self.runtime.record_catalog_hydration(started_at.elapsed());
        Ok(())
    }

    pub fn startup(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        let families_ready = self.midge.ensure_families_ready();
        self.runtime
            .record_storage_access("schema", true, families_ready.is_ok());
        families_ready.map_err(|error| {
            CassieError::StorageBootstrap(format!("bootstrap families: {error}"))
        })?;

        let schema_epoch = self.midge.schema_epoch();
        self.runtime
            .record_storage_access("schema", false, schema_epoch.is_ok());
        self.runtime.set_schema_epoch(
            schema_epoch
                .map_err(|error| CassieError::Storage(format!("load schema epoch: {error}")))?,
        );

        self.hydrate_catalog()
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        if self.started.swap(false, Ordering::SeqCst) {
            self.runtime.record_shutdown();
            self.runtime.mark_shutdown();
        }
    }

    fn hydrate_roles(&self) -> Result<(), CassieError> {
        let mut roles = self.midge.list_roles().map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list roles: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);

        let admin_name = normalize_role_name(&self.auth_user);
        if !roles.iter().any(|role| role.name == admin_name) {
            let password_hash = if self.auth_password.is_empty() {
                None
            } else {
                Some(hash_password(&self.auth_password)?)
            };
            let role = RoleMeta::bootstrap_admin(&self.auth_user, password_hash);
            self.midge.put_role(role.clone()).map_err(|error| {
                self.runtime.record_storage_access("schema", false, false);
                CassieError::Storage(format!("create bootstrap role: {error}"))
            })?;
            self.runtime.record_storage_access("schema", false, true);
            roles.push(role);
        }

        for role in roles {
            self.catalog.register_role(role);
        }

        Ok(())
    }

    fn hydrate_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        self.runtime.record_cardinality_read();
        match self.midge.get_cardinality_stats(collection) {
            Ok(Some(stats)) if stats.hydrated => {
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Ok(_) => {
                self.runtime.record_cardinality_unavailable();
                let stats = self
                    .midge
                    .rebuild_cardinality_stats_for_collection(collection)
                    .map_err(|error| {
                        CassieError::Storage(format!(
                            "rebuild cardinality stats for collection '{collection}': {error}"
                        ))
                    })?;
                self.runtime.record_cardinality_rebuild();
                self.runtime.record_cardinality_write();
                self.catalog.hydrate_cardinality_stats(collection, stats);
                Ok(())
            }
            Err(error) => Err(CassieError::Storage(format!(
                "load cardinality stats for collection '{collection}': {error}"
            ))),
        }
    }

    pub(crate) fn refresh_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        let stats = self
            .midge
            .rebuild_cardinality_stats_for_collection(collection)
            .map_err(|error| {
                CassieError::Storage(format!(
                    "rebuild cardinality stats for collection '{collection}': {error}"
                ))
            })?;
        self.runtime.record_cardinality_rebuild();
        self.runtime.record_cardinality_write();
        self.catalog.hydrate_cardinality_stats(collection, stats);
        Ok(())
    }

    fn is_query_cacheable(statement: &QueryStatement) -> bool {
        matches!(statement, QueryStatement::Select(_))
    }

    fn plan_cache_key_from_fingerprint(
        &self,
        sql_fingerprint: u64,
        parameter_shape: Vec<crate::runtime::ParameterShape>,
        mode: ExecutionMode,
        database: Option<String>,
    ) -> PlanCacheKey {
        PlanCacheKey {
            sql_fingerprint,
            schema_epoch: self.runtime.schema_epoch(),
            parameter_shape,
            mode,
            database,
        }
    }

    fn feedback_keys_for_plan(
        &self,
        sql_fingerprint: u64,
        database: Option<String>,
        physical: &crate::planner::physical::PhysicalPlan,
    ) -> Vec<RuntimeFeedbackKey> {
        let schema_epoch = self.runtime.schema_epoch();
        let collection = physical.collection.clone();
        physical
            .operators
            .iter()
            .map(|operator| RuntimeFeedbackKey {
                sql_fingerprint,
                schema_epoch,
                database: database.clone(),
                collection: collection.clone(),
                operator: format!("{operator:?}"),
            })
            .collect()
    }

    fn observe_feedback_lookup(&self, keys: &[RuntimeFeedbackKey]) {
        for key in keys {
            let _ = self.runtime.feedback_lookup(key);
        }
    }

    fn record_feedback_for_keys(
        &self,
        keys: Vec<RuntimeFeedbackKey>,
        observation: RuntimeFeedbackObservation,
    ) {
        for key in keys {
            self.runtime.record_feedback(key, observation.clone());
        }
    }

    #[doc(hidden)]
    pub fn plan_cache_hit_for_diagnostics(
        &self,
        parsed: &crate::sql::ast::ParsedStatement,
        params: &[crate::types::Value],
        mode: ExecutionMode,
        database: Option<String>,
    ) -> bool {
        let key = self.plan_cache_key_from_fingerprint(
            crate::runtime::sql_fingerprint(parsed),
            crate::runtime::parameter_shape(params),
            mode,
            database,
        );
        self.runtime.plan_cache_lookup(&key).is_some()
    }

    #[doc(hidden)]
    pub fn feedback_record_for_diagnostics(
        &self,
        key: &RuntimeFeedbackKey,
    ) -> Option<crate::runtime::RuntimeFeedbackRecord> {
        self.runtime.feedback_record(key)
    }

    fn plan_cache_provenance(
        hit: crate::runtime::L1PlanHit,
    ) -> (
        Arc<crate::planner::physical::PhysicalPlan>,
        PlanCacheProvenance,
    ) {
        (
            hit.plan,
            PlanCacheProvenance::L1 {
                durable: hit.durable,
                candidate_expires_at_ms: hit.candidate_expires_at_ms,
            },
        )
    }

    fn resolve_physical_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        key: PlanCacheKey,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<
        (
            Arc<crate::planner::physical::PhysicalPlan>,
            PlanCacheProvenance,
        ),
        CassieError,
    > {
        if let Some(hit) = self.runtime.plan_cache_lookup(&key) {
            return Ok(Self::plan_cache_provenance(hit));
        }

        if let Some(plan) = query_cache::lookup_plan(&self.midge, &self.runtime, &key)? {
            self.runtime.plan_cache_store(key, plan.clone(), true);
            return Ok((plan, PlanCacheProvenance::L2));
        }

        self.runtime.record_query_cache_compile_miss();
        let plan = self.compile_physical_plan(parsed, controls)?;
        self.runtime.plan_cache_store(key, plan.clone(), false);
        Ok((plan, PlanCacheProvenance::Compiled))
    }

    fn observe_query_plan_usage(
        &self,
        key: &PlanCacheKey,
        plan: &Arc<crate::planner::physical::PhysicalPlan>,
        provenance: &PlanCacheProvenance,
    ) -> Result<(), CassieError> {
        match provenance {
            PlanCacheProvenance::L2 => Ok(()),
            PlanCacheProvenance::L1 { durable: true, .. } => Ok(()),
            PlanCacheProvenance::L1 {
                durable: false,
                candidate_expires_at_ms,
            } => {
                let candidate_pending = candidate_expires_at_ms
                    .is_some_and(|expires_at_ms| current_time_millis() <= expires_at_ms);
                match query_cache::observe_non_durable_plan_usage(
                    &self.midge,
                    &self.runtime,
                    key,
                    plan,
                    candidate_pending,
                )? {
                    query_cache::NonDurablePlanOutcome::Durable => {
                        self.runtime.mark_plan_cache_entry_durable(key);
                    }
                    query_cache::NonDurablePlanOutcome::CandidatePending { ttl_seconds } => {
                        self.runtime
                            .mark_plan_cache_entry_candidate_pending(key, ttl_seconds);
                    }
                    query_cache::NonDurablePlanOutcome::Transient => {}
                }
                Ok(())
            }
            PlanCacheProvenance::Compiled => {
                match query_cache::observe_non_durable_plan_usage(
                    &self.midge,
                    &self.runtime,
                    key,
                    plan,
                    false,
                )? {
                    query_cache::NonDurablePlanOutcome::Durable => {
                        self.runtime.mark_plan_cache_entry_durable(key);
                    }
                    query_cache::NonDurablePlanOutcome::CandidatePending { ttl_seconds } => {
                        self.runtime
                            .mark_plan_cache_entry_candidate_pending(key, ttl_seconds);
                    }
                    query_cache::NonDurablePlanOutcome::Transient => {}
                }
                Ok(())
            }
        }
    }

    pub fn execute_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_with_mode(session, sql, params, ExecutionMode::SimpleQuery)
    }

    pub fn describe_sql(&self, sql: &str) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.describe_parsed_statement(parsed, sql_fingerprint)
    }

    pub(crate) fn describe_parsed_statement(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if matches!(parsed.statement, QueryStatement::Explain(_)) {
            return Ok(vec![ColumnMeta::text("QUERY PLAN")]);
        }
        if matches!(parsed.statement, QueryStatement::Transaction(_)) {
            return Ok(Vec::new());
        }

        let controls = self.runtime.query_controls(Instant::now());
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let cache_key = Self::is_query_cacheable(&parsed.statement).then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                Vec::new(),
                ExecutionMode::DescribeQuery,
                None,
            )
        });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, Some(&controls))?
        } else {
            (
                self.compile_physical_plan(parsed, Some(&controls))?,
                PlanCacheProvenance::Compiled,
            )
        };

        let user_functions = if crate::executor::plan_needs_user_functions(&physical.logical) {
            self.catalog
                .list_functions()
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<std::collections::HashMap<String, _>>()
        } else {
            std::collections::HashMap::new()
        };
        let collection_schema = self.catalog.get_schema(&physical.logical.collection);

        if physical.logical.command.is_some() {
            return Ok(Vec::new());
        }

        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(crate::executor::columns_from_projection(
            &physical.logical.projection,
            collection_schema.as_ref(),
            &user_functions,
        ))
    }

    pub(crate) fn execute_sql_with_mode(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let controls = self.runtime.query_controls(query_started);
        let result = self.execute_sql_core(session, sql, params, mode, &controls);
        let elapsed = query_started.elapsed();

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.rows.len()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }

        drop(running_guard);
        result
    }

    pub(crate) fn execute_sql_with_controls(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_core(session, sql, params, mode, controls)
    }

    fn compile_physical_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Arc<crate::planner::physical::PhysicalPlan>, CassieError> {
        let bound = binder::bind(parsed, &self.catalog)?;
        if controls.is_some_and(QueryExecutionControls::is_timed_out) {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let logical = crate::planner::logical::plan(&bound)?;
        let optimized = crate::planner::optimizer::optimize(logical);
        let cardinality_stats = self.catalog.cardinality.read().clone();
        Ok(Arc::new(crate::planner::physical::build_with_indexes(
            optimized,
            bound.indexes,
            &cardinality_stats,
        )))
    }

    fn execute_sql_core(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        if session.user.is_empty() {
            return Err(CassieError::Unauthorized);
        }

        if let Some(error) = unsupported_sql_error(sql) {
            return Err(error);
        }

        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        self.runtime.record_sql_parse();
        let parsed = parser::parse_statement(sql)?;
        let sql_fingerprint = crate::runtime::sql_fingerprint(&parsed);
        self.execute_parsed_statement_core(session, parsed, sql_fingerprint, params, mode, controls)
    }

    pub(crate) fn execute_preparsed_statement_with_mode(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let controls = self.runtime.query_controls(query_started);
        let result = self.execute_parsed_statement_core(
            session,
            parsed,
            sql_fingerprint,
            params,
            mode,
            &controls,
        );
        let elapsed = query_started.elapsed();

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.rows.len()),
            Err(error) => {
                self.runtime.record_query_error(elapsed, error);
                if session.is_transaction_active() {
                    session.mark_transaction_failed();
                }
            }
        }

        drop(running_guard);
        result
    }

    fn execute_parsed_statement_core(
        &self,
        session: &CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }
        if session.is_transaction_failed()
            && !matches!(
                &parsed.statement,
                QueryStatement::Transaction(TransactionStatement {
                    action: TransactionAction::Rollback | TransactionAction::RollbackTo { .. },
                    ..
                })
            )
        {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        if let QueryStatement::Explain(statement) = &parsed.statement {
            return self.explain_statement(
                session,
                statement.statement.as_ref().clone(),
                params,
                statement.analyze,
                controls,
            );
        }
        if let QueryStatement::Transaction(statement) = &parsed.statement {
            return self.execute_transaction_statement(session, statement);
        }

        let is_select = Self::is_query_cacheable(&parsed.statement);
        let cache_key = is_select.then(|| {
            self.plan_cache_key_from_fingerprint(
                sql_fingerprint,
                crate::runtime::parameter_shape(&params),
                mode,
                session.database.clone(),
            )
        });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, Some(controls))?
        } else {
            (
                self.compile_physical_plan(parsed, Some(controls))?,
                PlanCacheProvenance::Compiled,
            )
        };
        let feedback_keys = is_select.then(|| {
            let keys =
                self.feedback_keys_for_plan(sql_fingerprint, session.database.clone(), &physical);
            self.observe_feedback_lookup(&keys);
            keys
        });

        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let params_hash = if is_select {
            Some(crate::runtime::hash_params(&params))
        } else {
            None
        };

        if is_select {
            let exec_cache_key = crate::runtime::ExecutionResultCacheKey {
                sql_fingerprint,
                params_hash: params_hash.unwrap(),
                schema_epoch: self.runtime.schema_epoch(),
                data_epoch: self.runtime.data_epoch(),
                database: session.database.clone(),
                mode,
            };
            if let Some(cached) = self.runtime.execution_result_cache_lookup(&exec_cache_key) {
                if let Some(key) = cache_key.as_ref() {
                    self.observe_query_plan_usage(key, &physical, &provenance)?;
                }
                return Ok(cached);
            }
        }

        let feedback_before = feedback_keys.as_ref().map(|_| self.runtime.snapshot());
        let feedback_started_at = Instant::now();
        let execution = crate::executor::run_with_session_controls(
            self,
            Some(session),
            physical.clone(),
            params,
            controls,
        )
        .map_err(CassieError::from);

        if let Some(keys) = feedback_keys.clone() {
            let after = self.runtime.snapshot();
            let before = feedback_before.expect("feedback snapshot");
            let observation = RuntimeFeedbackObservation {
                rows_in: physical.estimates.scan_rows,
                rows_out: execution
                    .as_ref()
                    .map(|result| result.rows.len() as u64)
                    .unwrap_or(0),
                elapsed_ms: feedback_started_at
                    .elapsed()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
                storage_reads: after
                    .storage
                    .data
                    .reads
                    .saturating_sub(before.storage.data.reads),
                storage_writes: after
                    .storage
                    .data
                    .writes
                    .saturating_sub(before.storage.data.writes),
                temp_writes: after
                    .storage
                    .temp
                    .writes
                    .saturating_sub(before.storage.temp.writes),
                candidate_count: after
                    .search
                    .candidate_count_total
                    .saturating_sub(before.search.candidate_count_total)
                    .saturating_add(
                        after
                            .vector
                            .candidate_count_total
                            .saturating_sub(before.vector.candidate_count_total),
                    )
                    .saturating_add(
                        after
                            .hybrid
                            .candidate_count_total
                            .saturating_sub(before.hybrid.candidate_count_total),
                    ),
                result_count: after
                    .search
                    .result_count_total
                    .saturating_sub(before.search.result_count_total)
                    .saturating_add(
                        after
                            .vector
                            .result_count_total
                            .saturating_sub(before.vector.result_count_total),
                    )
                    .saturating_add(
                        after
                            .hybrid
                            .result_count_total
                            .saturating_sub(before.hybrid.result_count_total),
                    ),
                error_class: execution
                    .as_ref()
                    .err()
                    .map(|error| crate::runtime::error_class(error).to_string()),
            };
            self.record_feedback_for_keys(keys, observation);
        }

        let result = execution?;

        if result.rows.len() > controls.max_result_rows {
            return Err(CassieError::Execution(format!(
                "query result row limit exceeded: {} > {}",
                result.rows.len(),
                controls.max_result_rows
            )));
        }

        if is_select {
            let exec_cache_key = crate::runtime::ExecutionResultCacheKey {
                sql_fingerprint,
                params_hash: params_hash.unwrap(),
                schema_epoch: self.runtime.schema_epoch(),
                data_epoch: self.runtime.data_epoch(),
                database: session.database.clone(),
                mode,
            };
            self.runtime
                .execution_result_cache_store(exec_cache_key, result.clone());
        }

        let command = result.command.as_str();
        if command.starts_with("INSERT")
            || command.starts_with("UPDATE")
            || command.starts_with("DELETE")
        {
            self.runtime.bump_data_epoch();
        }

        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(result)
    }

    fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
    ) -> Result<QueryResult, CassieError> {
        let command = match &statement.action {
            TransactionAction::Begin => {
                session.begin_transaction(statement.isolation)?;
                "BEGIN"
            }
            TransactionAction::Commit => {
                if session.is_transaction_failed() {
                    return Err(CassieError::Execution(
                        "transaction is failed; rollback required".to_string(),
                    ));
                }
                for (collection, writes) in session.transaction_writes() {
                    for (id, change) in writes {
                        let result = match change {
                            TransactionRowChange::Upsert(payload) => self
                                .midge
                                .put_document(&collection, Some(id), payload)
                                .map(|_| ()),
                            TransactionRowChange::Delete => {
                                self.midge.delete_document(&collection, &id).map(|_| ())
                            }
                        };
                        if let Err(error) = result {
                            session.mark_transaction_failed();
                            return Err(error);
                        }
                        self.refresh_cardinality_stats(&collection)?;
                    }
                }
                session.commit_transaction();
                "COMMIT"
            }
            TransactionAction::Rollback => {
                session.rollback_transaction();
                "ROLLBACK"
            }
            TransactionAction::Savepoint { name } => {
                session.create_savepoint(name)?;
                "SAVEPOINT"
            }
            TransactionAction::RollbackTo { name } => {
                session.rollback_to_savepoint(name)?;
                "ROLLBACK"
            }
            TransactionAction::Release { name } => {
                session.release_savepoint(name)?;
                "RELEASE"
            }
        };

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: command.to_string(),
        })
    }

    fn explain_statement(
        &self,
        session: &CassieSession,
        statement: crate::sql::ast::ParsedStatement,
        params: Vec<crate::types::Value>,
        analyze: bool,
        controls: &QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let sql_fingerprint = crate::runtime::sql_fingerprint(&statement);
        let before = analyze.then(|| self.runtime.snapshot());
        let physical = self.compile_physical_plan(statement, Some(controls))?;
        let operators = physical
            .operators
            .iter()
            .map(|operator| format!("{operator:?}"))
            .collect::<Vec<_>>()
            .join(">");
        let projection_pruning = !physical.projected_scan_fields.is_empty();
        let scan_fields = if projection_pruning {
            physical.projected_scan_fields.join(",")
        } else {
            "all".to_string()
        };
        let limit_pushdown = physical.scan_limit.is_some();
        let scan_limit = physical
            .scan_limit
            .map(|limit| limit.to_string())
            .unwrap_or_else(|| "none".to_string());
        let index_aware = physical.selected_index.is_some();
        let index = physical.selected_index.as_deref().unwrap_or("none");
        let covered_index = physical.covered_index;
        let prefilter = match physical.logical.filter.as_ref() {
            None => "none".to_string(),
            Some(filter) => {
                if let Some(index) = physical.selected_index.as_deref() {
                    format!("index={index}")
                } else if let Some(schema) = self.catalog.get_schema(&physical.collection) {
                    if vector_prefilter_supported(filter, &schema) {
                        "row-scan".to_string()
                    } else {
                        format!(
                            "fallback={}",
                            vector_prefilter_fallback_reason(filter, &schema)
                        )
                    }
                } else {
                    "fallback=missing-schema".to_string()
                }
            }
        };
        let top_k_limit = physical
            .top_k_limit
            .map(|limit| limit.to_string())
            .unwrap_or_else(|| "none".to_string());
        let join_strategy = physical.join_strategy.as_deref().unwrap_or("none");
        let candidate_budget = physical.top_k_limit.map(|top_needed| {
            let limits = self.runtime.limits();
            let feedback_budget = self
                .runtime
                .feedback_candidate_budget(&physical.collection)
                .unwrap_or_default();
            top_needed
                .max(limits.adaptive_candidate_min)
                .max(feedback_budget)
                .min(limits.adaptive_candidate_max)
        });
        let candidate_budget = candidate_budget
            .map(|budget| budget.to_string())
            .unwrap_or_else(|| "none".to_string());
        let estimates = &physical.estimates;
        let mut plan = format!(
            "collection={} operators={} predicate_pushdown={} projection_pruning={} scan_fields={} limit_pushdown={} scan_limit={} index_aware={} index={} covered_index={} prefilter={} top_k={} top_k_limit={} candidate_budget={} join_strategy={} estimates=scan:{} index:{} join:{} search:{} vector:{} aggregate:{}",
            physical.collection,
            if operators.is_empty() {
                "Command".to_string()
            } else {
                operators
            },
            physical.predicate_pushdown,
            projection_pruning,
            scan_fields,
            limit_pushdown,
            scan_limit,
            index_aware,
            index,
            covered_index,
            prefilter,
            physical.top_k,
            top_k_limit,
            candidate_budget,
            join_strategy,
            estimates.scan_rows,
            estimates.index_rows,
            estimates.join_rows,
            estimates.search_rows,
            estimates.vector_rows,
            estimates.aggregate_rows
        );

        if analyze {
            let feedback_keys =
                self.feedback_keys_for_plan(sql_fingerprint, session.database.clone(), &physical);
            self.observe_feedback_lookup(&feedback_keys);
            let started_at = Instant::now();
            let result = crate::executor::run_with_session_controls(
                self,
                Some(session),
                physical.clone(),
                params,
                controls,
            )
            .map_err(CassieError::from)?;
            let elapsed_ms = started_at.elapsed().as_millis();
            let after = self.runtime.snapshot();
            let before = before.expect("analyze snapshot");
            let plan_cache_hits_delta =
                after.plan_cache.hits.saturating_sub(before.plan_cache.hits);
            let plan_cache_misses_delta = after
                .plan_cache
                .misses
                .saturating_sub(before.plan_cache.misses);
            let storage_reads_delta = after
                .storage
                .data
                .reads
                .saturating_sub(before.storage.data.reads);
            let storage_writes_delta = after
                .storage
                .data
                .writes
                .saturating_sub(before.storage.data.writes);
            let temp_writes_delta = after
                .storage
                .temp
                .writes
                .saturating_sub(before.storage.temp.writes);
            let candidate_count_delta = after
                .search
                .candidate_count_total
                .saturating_sub(before.search.candidate_count_total)
                .saturating_add(
                    after
                        .vector
                        .candidate_count_total
                        .saturating_sub(before.vector.candidate_count_total),
                )
                .saturating_add(
                    after
                        .hybrid
                        .candidate_count_total
                        .saturating_sub(before.hybrid.candidate_count_total),
                );
            let result_count_delta = after
                .search
                .result_count_total
                .saturating_sub(before.search.result_count_total)
                .saturating_add(
                    after
                        .vector
                        .result_count_total
                        .saturating_sub(before.vector.result_count_total),
                )
                .saturating_add(
                    after
                        .hybrid
                        .result_count_total
                        .saturating_sub(before.hybrid.result_count_total),
                );
            self.record_feedback_for_keys(
                feedback_keys,
                RuntimeFeedbackObservation {
                    rows_in: physical.estimates.scan_rows,
                    rows_out: result.rows.len() as u64,
                    elapsed_ms: elapsed_ms.min(u128::from(u64::MAX)) as u64,
                    storage_reads: storage_reads_delta,
                    storage_writes: storage_writes_delta,
                    temp_writes: temp_writes_delta,
                    candidate_count: candidate_count_delta,
                    result_count: result_count_delta,
                    error_class: None,
                },
            );
            let actual_operators = if physical.operators.is_empty() {
                "Command".to_string()
            } else {
                physical
                    .operators
                    .iter()
                    .map(|operator| {
                        format!("{operator:?}:rows_in:{} rows_out:{} elapsed_ms:{} storage_reads:{} storage_writes:{} temp_writes:{} candidates:{} results:{}",
                            physical.estimates.scan_rows,
                            result.rows.len(),
                            elapsed_ms,
                            storage_reads_delta,
                            storage_writes_delta,
                            temp_writes_delta,
                            candidate_count_delta,
                            result_count_delta
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            };
            plan.push_str(&format!(
                " analyze=true actual_rows={} actual_ms={} operator_actuals={} diagnostics=plan_cache_hits_delta:{},plan_cache_misses_delta:{},storage_reads_delta:{},storage_writes_delta:{},temp_writes_delta:{},candidate_count_delta:{},result_count_delta:{}",
                result.rows.len(),
                elapsed_ms,
                actual_operators,
                plan_cache_hits_delta,
                plan_cache_misses_delta,
                storage_reads_delta,
                storage_writes_delta,
                temp_writes_delta,
                candidate_count_delta,
                result_count_delta
            ));
        }

        Ok(QueryResult {
            columns: vec![ColumnMeta::text("QUERY PLAN")],
            rows: vec![vec![Value::String(plan)]],
            command: "EXPLAIN".to_string(),
        })
    }

    pub fn execute_vector_search(
        &self,
        collection: &str,
        vector_field: &str,
        query: &str,
        metric: Option<DistanceMetric>,
        limit: usize,
        offset: usize,
    ) -> Result<QueryResult, CassieError> {
        let index = self
            .catalog
            .get_vector_index(collection, vector_field)
            .ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "vector index not found for collection '{collection}', field '{vector_field}'"
                ))
            })?;

        self.validate_embedding_compatibility(&index, metric.as_ref())?;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .map_err(CassieError::from)?;
        self.validate_embedding_payload(&index, &embedding)?;

        let metric = metric.unwrap_or(index.metadata.metric.clone());
        self.execute_projected_vector_search(
            &index,
            collection,
            vector_field,
            &embedding.values,
            metric,
            limit,
            offset,
        )
    }

    fn execute_projected_vector_search(
        &self,
        index: &VectorIndexRecord,
        collection: &str,
        vector_field: &str,
        query: &[f32],
        metric: DistanceMetric,
        limit: usize,
        offset: usize,
    ) -> Result<QueryResult, CassieError> {
        let limit = limit.max(1);
        let top_needed = limit.saturating_add(offset).max(1);
        let schema = self
            .catalog
            .get_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let candidates = self.midge.scan_rows_for_rebuild(
            collection,
            RowDecode::Projected(vec![vector_field.to_string()]),
        )?;
        let normalized_vectors = if matches!(&metric, DistanceMetric::Cosine | DistanceMetric::Dot)
        {
            Some(
                self.midge
                    .list_normalized_vectors(collection, vector_field)?
                    .into_iter()
                    .map(|record| (record.id.clone(), record))
                    .collect::<BTreeMap<_, _>>(),
            )
        } else {
            None
        };
        let normalized_query = if matches!(&metric, DistanceMetric::Cosine) {
            normalize_vector(query)
        } else {
            None
        };

        if index.metadata.index_type == VectorIndexType::Hnsw {
            let metric_fn: fn(&[f32], &[f32]) -> f64 = match metric {
                DistanceMetric::Cosine => crate::vector::cosine_distance,
                DistanceMetric::Dot => crate::vector::dot_distance,
                DistanceMetric::L2 => crate::vector::l2_distance,
            };
            let hnsw_candidates = candidates
                .into_iter()
                .filter_map(|candidate| {
                    candidate
                        .payload
                        .get(vector_field)
                        .and_then(vector_from_json)
                        .map(|vector| (candidate.id, vector))
                })
                .collect::<Vec<_>>();
            let selected =
                crate::vector::hnsw::search(query, hnsw_candidates, top_needed, metric_fn)
                    .into_iter()
                    .skip(offset)
                    .take(limit);
            let mut rows = Vec::new();
            for candidate in selected {
                if let Some(document) = self.midge.get_document(collection, &candidate.id)? {
                    rows.push(vector_search_row(&schema, document));
                }
            }
            self.runtime
                .record_vector_normalization_usage(0, rows.len());
            return Ok(QueryResult {
                columns: vector_search_columns(&schema),
                rows,
                command: "SELECT".to_string(),
            });
        }

        let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
        let mut normalized_candidate_count = 0usize;
        let mut fallback_candidate_count = 0usize;
        for candidate in candidates {
            let vector = candidate
                .payload
                .get(vector_field)
                .and_then(vector_from_json)
                .unwrap_or_default();
            let normalized_record = normalized_vectors
                .as_ref()
                .and_then(|records| records.get(candidate.id.as_str()));
            let can_use_normalized = normalized_record.is_some_and(|record| {
                record.payload_available
                    && record.normalization_version
                        == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
                    && record.metric == metric
                    && record.dimensions == query.len()
                    && record.values.len() == query.len()
            });
            let (distance, used_normalized) = if can_use_normalized {
                match &metric {
                    DistanceMetric::Cosine => normalized_query
                        .as_ref()
                        .map(|normalized_query| {
                            let record = normalized_record.expect("normalized record");
                            (
                                cosine_distance_from_normalized_query(
                                    normalized_query.values.as_slice(),
                                    record.values.as_slice(),
                                ),
                                true,
                            )
                        })
                        .unwrap_or_else(|| {
                            (vector_distance_for_metric(&metric, query, &vector), false)
                        }),
                    DistanceMetric::Dot => {
                        let record = normalized_record.expect("normalized record");
                        (
                            dot_distance_from_normalized_target(
                                query,
                                record.values.as_slice(),
                                record.magnitude,
                            ),
                            true,
                        )
                    }
                    DistanceMetric::L2 => {
                        (vector_distance_for_metric(&metric, query, &vector), false)
                    }
                }
            } else {
                (vector_distance_for_metric(&metric, query, &vector), false)
            };
            if used_normalized {
                normalized_candidate_count += 1;
            } else {
                fallback_candidate_count += 1;
            }
            let scored = ScoredVectorCandidate {
                distance,
                id: candidate.id,
            };
            if top.len() < top_needed {
                top.push(scored);
            } else if let Some(worst) = top.peek() {
                if scored.is_better_than(worst) {
                    top.pop();
                    top.push(scored);
                }
            }
        }

        let mut ranked = top.into_vec();
        ranked.sort_by(compare_scored_vector_candidates);
        let selected = ranked.into_iter().skip(offset).take(limit);
        let mut rows = Vec::new();
        for candidate in selected {
            if let Some(document) = self.midge.get_document(collection, &candidate.id)? {
                rows.push(vector_search_row(&schema, document));
            }
        }

        self.runtime.record_vector_normalization_usage(
            normalized_candidate_count,
            fallback_candidate_count,
        );

        Ok(QueryResult {
            columns: vector_search_columns(&schema),
            rows,
            command: "SELECT".to_string(),
        })
    }

    pub fn ingest_document(
        &self,
        collection: &str,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        self.write_document(collection, None, payload, true, None)
    }

    pub(crate) fn write_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        self.write_document_for_session(None, collection, id, payload, apply_defaults, exclude_id)
    }

    pub(crate) fn write_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        let payload = self.prepare_document_write_for_session(
            session,
            collection,
            payload,
            apply_defaults,
            exclude_id,
        )?;

        if let Some(session) = session {
            if session.is_transaction_active() {
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                session.stage_document_write(collection, id.clone(), payload);
                return Ok(id);
            }
        }

        let row_id = self.midge.put_document(collection, id, payload)?;
        self.refresh_cardinality_stats(collection)?;
        Ok(row_id)
    }

    pub(crate) fn prepare_document_write_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        mut payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<serde_json::Value, CassieError> {
        let constraints = self.catalog.get_constraints(collection);
        if apply_defaults && !constraints.is_empty() {
            self.apply_default_values(&mut payload, &constraints)?;
        }

        self.validate_payload_schema(collection, &payload)?;

        let indexes = self.catalog.list_vector_indexes(collection);
        if !indexes.is_empty() {
            self.apply_vector_indexes(collection, &mut payload, indexes.as_slice())?;
        }

        self.validate_constraints_for_session(
            session,
            collection,
            &payload,
            &constraints,
            exclude_id,
        )?;
        self.validate_unique_indexes_for_session(session, collection, &payload, exclude_id)?;

        Ok(payload)
    }

    pub(crate) fn put_prepared_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: String,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        if let Some(session) = session {
            if session.is_transaction_active() {
                session.stage_document_write(collection, id.clone(), payload);
                return Ok(id);
            }
        }

        let row_id = self.midge.put_document(collection, Some(id), payload)?;
        self.refresh_cardinality_stats(collection)?;
        Ok(row_id)
    }

    pub(crate) fn delete_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: &str,
    ) -> Result<bool, CassieError> {
        if let Some(session) = session {
            if session.is_transaction_active() {
                let existed = self
                    .get_document_for_session(Some(session), collection, id)?
                    .is_some();
                session.stage_document_delete(collection, id.to_string());
                return Ok(existed);
            }
        }

        let removed = self.midge.delete_document(collection, id)?;
        if removed {
            self.refresh_cardinality_stats(collection)?;
        }
        Ok(removed)
    }

    pub(crate) fn get_document_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        if let Some(session) = session {
            if let Some(change) = session.document_change(collection, id) {
                return Ok(match change {
                    TransactionRowChange::Upsert(payload) => Some(DocumentRef {
                        id: id.to_string(),
                        payload,
                    }),
                    TransactionRowChange::Delete => None,
                });
            }
        }

        self.midge.get_document(collection, id)
    }

    pub(crate) fn scan_documents_batched_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        let mut rows = self
            .midge
            .scan_documents(collection)?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();

        if let Some(session) = session {
            for (id, change) in session.collection_changes(collection) {
                match change {
                    TransactionRowChange::Upsert(payload) => {
                        rows.insert(id.clone(), DocumentRef { id, payload });
                    }
                    TransactionRowChange::Delete => {
                        rows.remove(&id);
                    }
                }
            }
        }

        let batch_size = batch_size.max(1);
        let mut batches = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        for document in rows.into_values() {
            current.push(document);
            if current.len() >= batch_size {
                batches.push(current);
                current = Vec::with_capacity(batch_size);
            }
        }
        if !current.is_empty() {
            batches.push(current);
        }

        Ok(batches)
    }

    pub(crate) fn scan_projected_documents_batched_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
    ) -> Result<Vec<Vec<DocumentRef>>, CassieError> {
        self.scan_projected_documents_batched_for_session_with_timings(
            session, collection, batch_size, fields, limit,
        )
        .map(|(batches, _)| batches)
    }

    pub(crate) fn scan_projected_documents_batched_for_session_with_timings(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_projected_documents_batched_for_session_with_filter_and_timings(
            session, collection, batch_size, fields, None, limit,
        )
    }

    pub(crate) fn scan_projected_documents_batched_for_session_with_filter_and_timings(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let started = Instant::now();
        let mut timings = MidgeScanTimings::default();
        let collection_changes = if let Some(session) = session {
            session.collection_changes(collection)
        } else {
            BTreeMap::new()
        };
        if collection_changes.is_empty() {
            let (batches, scan_timings) = self
                .midge
                .scan_projected_rows_batched_filter_limit_with_timings(
                    collection,
                    batch_size,
                    fields.to_vec(),
                    filter,
                    limit,
                )?;
            let measured = scan_timings.scan.saturating_add(scan_timings.row_decode);
            timings = scan_timings;
            timings.scan = timings
                .scan
                .saturating_add(started.elapsed().saturating_sub(measured));
            return Ok((batches, timings));
        }

        let mut rows = self
            .midge
            .scan_rows_for_rebuild(collection, RowDecode::Projected(fields.to_vec()))?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect::<BTreeMap<_, _>>();

        for (id, change) in collection_changes {
            match change {
                TransactionRowChange::Upsert(payload) => {
                    rows.insert(
                        id.clone(),
                        DocumentRef {
                            id,
                            payload: project_payload_fields(&payload, fields),
                        },
                    );
                }
                TransactionRowChange::Delete => {
                    rows.remove(&id);
                }
            }
        }

        let batch_size = batch_size.max(1);
        let mut batches = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        for document in rows.into_values() {
            current.push(document);
            if current.len() >= batch_size {
                batches.push(current);
                current = Vec::with_capacity(batch_size);
            }
        }
        if !current.is_empty() {
            batches.push(current);
        }

        let measured = timings.scan.saturating_add(timings.row_decode);
        timings.scan = timings
            .scan
            .saturating_add(started.elapsed().saturating_sub(measured));

        Ok((batches, timings))
    }

    fn validate_payload_schema(
        &self,
        collection: &str,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let schema = self
            .catalog
            .get_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;

        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for (field, value) in object {
            let expected = schema
                .fields
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(field))
                .ok_or_else(|| {
                    CassieError::InvalidVector(format!(
                        "field '{field}' is not defined on collection '{collection}'"
                    ))
                })?
                .data_type
                .clone();
            Self::validate_value_against_data_type(field, &expected, value)?;
        }

        Ok(())
    }

    fn validate_value_against_data_type(
        field: &str,
        expected: &crate::types::DataType,
        value: &serde_json::Value,
    ) -> Result<(), CassieError> {
        if value.is_null() {
            if matches!(expected, crate::types::DataType::Null) {
                return Ok(());
            }
            return Ok(());
        }

        match expected {
            crate::types::DataType::Null => Err(CassieError::InvalidVector(format!(
                "field '{field}' expects null"
            ))),
            crate::types::DataType::SmallInt => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .ok_or_else(|| {
                        CassieError::InvalidVector(format!("field '{field}' expects smallint"))
                    })?;

                if i16::try_from(number).is_ok() {
                    return Ok(());
                }

                Err(CassieError::InvalidVector(format!(
                    "field '{field}' expects smallint"
                )))
            }
            crate::types::DataType::Int => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .ok_or_else(|| {
                        CassieError::InvalidVector(format!("field '{field}' expects int"))
                    })?;

                if i32::try_from(number).is_ok() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects int"
                    )))
                }
            }
            crate::types::DataType::BigInt => {
                if value.is_i64() || value.as_u64().is_some() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects bigint"
                    )))
                }
            }
            crate::types::DataType::Float => {
                if value.is_number() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects float"
                    )))
                }
            }
            crate::types::DataType::Boolean => {
                if value.is_boolean() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects boolean"
                    )))
                }
            }
            crate::types::DataType::Text | crate::types::DataType::Uuid => {
                if !value.is_string() {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects {}",
                        expected.type_name()
                    )));
                }

                if let crate::types::DataType::Uuid = expected {
                    let value = value.as_str().unwrap_or_default();
                    if Uuid::parse_str(value).is_err() {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects UUID"
                        )));
                    }
                }

                Ok(())
            }
            crate::types::DataType::Char { length } => {
                let value = value.as_str().ok_or_else(|| {
                    CassieError::InvalidVector(format!("field '{field}' expects char"))
                })?;

                let max = length.unwrap_or(1) as usize;
                if value.chars().count() <= max {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects char({max})"
                    )))
                }
            }
            crate::types::DataType::Varchar { length } => {
                let value = value.as_str().ok_or_else(|| {
                    CassieError::InvalidVector(format!("field '{field}' expects varchar"))
                })?;

                if let Some(length) = length {
                    if value.chars().count() <= (*length as usize) {
                        Ok(())
                    } else {
                        Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects varchar({length})"
                        )))
                    }
                } else {
                    Ok(())
                }
            }
            crate::types::DataType::Bytea => {
                if !value.is_string() {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects bytea"
                    )));
                }

                Self::decode_bytea(value.as_str().unwrap_or_default())?;
                Ok(())
            }
            crate::types::DataType::Date
            | crate::types::DataType::Time
            | crate::types::DataType::Timestamp => {
                if value.is_string() {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects {}",
                        expected.type_name()
                    )))
                }
            }
            crate::types::DataType::Json => {
                if value.is_object()
                    || value.is_array()
                    || value.is_string()
                    || value.is_number()
                    || value.is_boolean()
                    || value.is_null()
                {
                    Ok(())
                } else {
                    Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects json"
                    )))
                }
            }
            crate::types::DataType::Vector(size) => {
                let Some(array) = value.as_array() else {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                };
                if array.len() != *size {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                }
                if array.iter().any(|value| value.as_f64().is_none()) {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects vector({size})"
                    )));
                }
                Ok(())
            }
            crate::types::DataType::Array(inner) => {
                let Some(values) = value.as_array() else {
                    return Err(CassieError::InvalidVector(format!(
                        "field '{field}' expects array"
                    )));
                };

                for value in values {
                    Self::validate_value_against_data_type(field, inner, value)?;
                }

                Ok(())
            }
        }
    }

    fn decode_bytea(value: &str) -> Result<Vec<u8>, CassieError> {
        if !value.starts_with("\\x") {
            return Err(CassieError::InvalidVector(
                "bytea expects hex format '\\x'".to_string(),
            ));
        }

        if value.len() == 2 {
            return Ok(Vec::new());
        }

        if (value.len() - 2).rem_euclid(2) != 0 {
            return Err(CassieError::InvalidVector(
                "bytea expects an even number of hex digits".to_string(),
            ));
        }

        let raw = value.as_bytes();
        let mut out = Vec::with_capacity((value.len() - 2) / 2);
        let mut index = 2;
        while index < value.len() {
            let high = Self::decode_hex_digit(raw[index])
                .ok_or_else(|| CassieError::InvalidVector("invalid bytea hex digit".to_string()))?;
            let low = Self::decode_hex_digit(raw[index + 1])
                .ok_or_else(|| CassieError::InvalidVector("invalid bytea hex digit".to_string()))?;
            out.push(high << 4 | low);
            index += 2;
        }

        Ok(out)
    }

    fn decode_hex_digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    fn apply_default_values(
        &self,
        payload: &mut serde_json::Value,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for constraint in constraints {
            if object.contains_key(&constraint.field) {
                continue;
            }

            if let Some(default) = &constraint.default_value {
                object.insert(constraint.field.clone(), default.clone());
            }
        }

        Ok(())
    }

    fn validate_constraints_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        constraints: &[FieldConstraint],
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for constraint in constraints {
            let existing = object.get(&constraint.field);

            if (constraint.not_null || constraint.primary_key)
                && (existing.is_none() || existing.is_some_and(|value| value.is_null()))
            {
                return Err(CassieError::InvalidVector(format!(
                    "field '{}' cannot be null",
                    constraint.field
                )));
            }

            if let Some(check) = &constraint.check {
                let Some(value) = existing else {
                    continue;
                };
                if !self.satisfies_check_constraint(value, check) {
                    return Err(CassieError::InvalidVector(format!(
                        "check constraint failed for '{}' field",
                        check.field
                    )));
                }
            }
        }

        self.validate_uniques(session, collection, object, constraints, exclude_id)
    }

    fn satisfies_check_constraint(
        &self,
        value: &serde_json::Value,
        check: &ConstraintCheck,
    ) -> bool {
        match check.operator {
            ConstraintOperator::Eq => value == &check.value,
            ConstraintOperator::NotEq => value != &check.value,
            ConstraintOperator::Lt => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_lt()),
            ConstraintOperator::Lte => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_le()),
            ConstraintOperator::Gt => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_gt()),
            ConstraintOperator::Gte => self
                .compare_constraint_values(value, &check.value)
                .is_some_and(|order| order.is_ge()),
            ConstraintOperator::Like => {
                let Some(value) = value.as_str() else {
                    return false;
                };
                let Some(expected) = check.value.as_str() else {
                    return false;
                };
                self.string_like_match(expected, value)
            }
        }
    }

    fn compare_constraint_values(
        &self,
        left: &serde_json::Value,
        right: &serde_json::Value,
    ) -> Option<std::cmp::Ordering> {
        match (left, right) {
            (serde_json::Value::Number(left), serde_json::Value::Number(right)) => left
                .as_f64()
                .and_then(|left| right.as_f64().map(|right| left.partial_cmp(&right)))
                .flatten(),
            (serde_json::Value::String(left), serde_json::Value::String(right)) => {
                Some(left.cmp(right))
            }
            (serde_json::Value::Bool(left), serde_json::Value::Bool(right)) => {
                Some(left.cmp(right))
            }
            _ => None,
        }
    }

    fn string_like_match(&self, pattern: &str, value: &str) -> bool {
        if pattern == "%" {
            return true;
        }

        let starts_with_wildcard = pattern.starts_with('%');
        let ends_with_wildcard = pattern.ends_with('%');
        let normalized = pattern.trim_matches('%');

        if starts_with_wildcard && ends_with_wildcard {
            value.contains(normalized)
        } else if starts_with_wildcard {
            value.ends_with(normalized)
        } else if ends_with_wildcard {
            value.starts_with(normalized)
        } else {
            value == pattern
        }
    }

    fn validate_uniques(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Map<String, serde_json::Value>,
        constraints: &[FieldConstraint],
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        for constraint in constraints {
            if !(constraint.unique || constraint.primary_key) {
                continue;
            }

            let Some(value) = payload.get(&constraint.field) else {
                continue;
            };
            if value.is_null() {
                continue;
            }

            if self.value_exists_for_collection_field(
                session,
                collection,
                &constraint.field,
                value,
                exclude_id,
            )? {
                return Err(CassieError::InvalidVector(format!(
                    "unique constraint failed for '{}'",
                    constraint.field
                )));
            }
        }

        Ok(())
    }

    fn validate_unique_indexes_for_session(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        payload: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<(), CassieError> {
        let object = payload.as_object().ok_or_else(|| {
            CassieError::InvalidVector("document payload must be a JSON object".to_string())
        })?;

        for index in self.catalog.list_indexes(collection) {
            if !index.unique || index.kind != crate::catalog::IndexKind::Scalar {
                continue;
            }

            let fields = index.normalized_fields();
            let mut values = Vec::with_capacity(fields.len());
            for field in &fields {
                let Some(value) = object.get(field) else {
                    values.clear();
                    break;
                };
                if value.is_null() {
                    values.clear();
                    break;
                }
                values.push((field.as_str(), value));
            }

            if values.is_empty() {
                continue;
            }

            if self.values_exist_for_collection_fields(session, collection, &values, exclude_id)? {
                return Err(CassieError::InvalidVector(format!(
                    "unique index '{}' failed",
                    index.name
                )));
            }
        }

        Ok(())
    }

    fn value_exists_for_collection_field(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        field: &str,
        value: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<bool, CassieError> {
        for document in self
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if document.payload.get(field) == Some(value) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn values_exist_for_collection_fields(
        &self,
        session: Option<&CassieSession>,
        collection: &str,
        values: &[(&str, &serde_json::Value)],
        exclude_id: Option<&str>,
    ) -> Result<bool, CassieError> {
        for document in self
            .scan_documents_batched_for_session(session, collection, 1024)?
            .into_iter()
            .flatten()
        {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if values
                .iter()
                .all(|(field, value)| document.payload.get(*field) == Some(*value))
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn apply_vector_indexes(
        &self,
        _collection: &str,
        payload: &mut serde_json::Value,
        indexes: &[VectorIndexRecord],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidEmbedding("document payload must be a JSON object".to_string())
        })?;

        for index in indexes {
            self.validate_embedding_compatibility(index, None)?;

            let source_value = object.get(&index.source_field).ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "missing source field '{}' for vector index '{}' on collection '{}'",
                    index.source_field, index.field, index.collection
                ))
            })?;

            let source = if let Some(value) = source_value.as_str() {
                value.to_string()
            } else {
                source_value.to_string()
            };

            let embedding = self
                .embedding_provider
                .embed_query(&source)
                .map_err(CassieError::from)?;
            self.validate_embedding_payload(index, &embedding)?;

            object.insert(
                index.field.clone(),
                serde_json::Value::Array(
                    embedding
                        .values
                        .into_iter()
                        .map(serde_json::Value::from)
                        .collect(),
                ),
            );
        }

        Ok(())
    }

    fn validate_embedding_compatibility(
        &self,
        index: &VectorIndexRecord,
        requested_metric: Option<&DistanceMetric>,
    ) -> Result<(), CassieError> {
        if self.embedding_provider.provider_name() != index.metadata.provider {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding provider mismatch: index requires '{}', active is '{}'",
                index.metadata.provider,
                self.embedding_provider.provider_name()
            )));
        }

        if self.embedding_provider.model_name() != index.metadata.model {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding model mismatch: index requires '{}', active is '{}'",
                index.metadata.model,
                self.embedding_provider.model_name()
            )));
        }

        if self.embedding_provider.dimensions() != index.metadata.dimensions {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding dimension mismatch: index requires {}, active provider has {}",
                index.metadata.dimensions,
                self.embedding_provider.dimensions()
            )));
        }

        if let Some(metric) = requested_metric {
            if *metric != index.metadata.metric {
                return Err(CassieError::InvalidEmbedding(format!(
                    "embedding metric mismatch: index requires '{}', request requested '{}'",
                    index.metadata.metric.as_str(),
                    metric.as_str()
                )));
            }
        }

        Ok(())
    }

    fn validate_embedding_payload(
        &self,
        index: &VectorIndexRecord,
        embedding: &Embedding,
    ) -> Result<(), CassieError> {
        if embedding.values.len() != index.metadata.dimensions {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding dimension mismatch: index requires {} and got {}",
                index.metadata.dimensions,
                embedding.values.len()
            )));
        }

        Ok(())
    }

    pub fn register_collection(&self, name: impl Into<String>, schema: crate::types::Schema) {
        let name = name.into();
        self.catalog.register_collection(
            &name,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        self.invalidate_plan_cache();
    }

    pub fn register_vector_index(&self, index: VectorIndexRecord) {
        self.catalog.register_vector_index(index);
        self.invalidate_plan_cache();
    }

    pub fn health(&self) -> serde_json::Value {
        let ready = self.is_started();
        let collections = self.midge.list_collections();
        serde_json::json!({
            "status": if ready { "ok" } else { "starting" },
            "ready": ready,
            "collections": collections.len(),
            "version": env!("CARGO_PKG_VERSION")
        })
    }

    pub fn create_session(&self, user: &str, database: Option<String>) -> CassieSession {
        let database = database.or_else(|| Some(self.default_database.clone()));
        CassieSession::new(user.to_string(), database)
    }

    pub(crate) fn lookup_role(&self, name: &str) -> Result<Option<RoleMeta>, CassieError> {
        let normalized = normalize_role_name(name);
        if normalized.is_empty() {
            return Ok(None);
        }

        if let Some(role) = self.catalog.get_role(&normalized) {
            return Ok(Some(role));
        }

        self.midge
            .get_role(&normalized)
            .map_err(|error| CassieError::Storage(format!("load role '{normalized}': {error}")))
    }

    pub fn authenticate_role(
        &self,
        user: &str,
        password: Option<&str>,
        database: Option<String>,
    ) -> Result<CassieSession, CassieError> {
        let normalized = normalize_role_name(user);
        let Some(role) = self.lookup_role(&normalized)? else {
            return Err(CassieError::Unauthorized);
        };
        if !role.can_login {
            return Err(CassieError::Unauthorized);
        }

        if let Some(hash) = role.password_hash.as_deref() {
            let Some(password) = password else {
                return Err(CassieError::Unauthorized);
            };
            if !verify_password(hash, password)? {
                return Err(CassieError::Unauthorized);
            }
        } else if password.is_some_and(|value| !value.is_empty()) {
            return Err(CassieError::Unauthorized);
        }

        Ok(CassieSession::new(
            role.name,
            database.or_else(|| Some(self.default_database.clone())),
        ))
    }

    pub fn create_role(
        &self,
        name: &str,
        login: bool,
        password: Option<String>,
        if_not_exists: bool,
    ) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        if normalized.is_empty() {
            return Err(CassieError::Planner(
                "CREATE ROLE requires a name".to_string(),
            ));
        }

        if self.lookup_role(&normalized)?.is_some() {
            if if_not_exists {
                return Ok(());
            }
            return Err(CassieError::Planner(format!(
                "role '{normalized}' already exists"
            )));
        }

        let password_hash = match (login, password) {
            (true, Some(password)) => Some(hash_password(&password)?),
            (true, None) => {
                return Err(CassieError::Planner(
                    "login roles require a password".into(),
                ));
            }
            (false, Some(_)) => {
                return Err(CassieError::Unsupported(
                    "PASSWORD is only supported for login roles".into(),
                ));
            }
            (false, None) => None,
        };

        let role = RoleMeta::new(normalized, login, false, password_hash);
        self.midge
            .put_role(role.clone())
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

    pub fn alter_role(
        &self,
        name: &str,
        login: Option<bool>,
        password: Option<String>,
    ) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        let Some(mut role) = self.lookup_role(&normalized)? else {
            return Err(CassieError::NotFound(format!(
                "role '{normalized}' not found"
            )));
        };

        if role.is_admin {
            if let Some(false) = login {
                return Err(CassieError::Unsupported(
                    "cannot disable the bootstrap admin role".into(),
                ));
            }
        }

        if let Some(login) = login {
            role.can_login = login;
        }

        if let Some(password) = password {
            role.password_hash = Some(hash_password(&password)?);
        }

        if role.can_login && role.password_hash.is_none() {
            return Err(CassieError::Planner(
                "login roles require a password".into(),
            ));
        }

        self.midge
            .put_role(role.clone())
            .map_err(|error| CassieError::Storage(format!("persist role '{name}': {error}")))?;
        self.catalog.register_role(role);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

    pub fn drop_role(&self, name: &str, if_exists: bool) -> Result<(), CassieError> {
        let normalized = normalize_role_name(name);
        let Some(role) = self.lookup_role(&normalized)? else {
            if if_exists {
                return Ok(());
            }
            return Err(CassieError::NotFound(format!(
                "role '{normalized}' not found"
            )));
        };

        if role.is_admin {
            return Err(CassieError::Unsupported(
                "cannot drop the bootstrap admin role".into(),
            ));
        }

        self.midge
            .delete_role(&normalized)
            .map_err(|error| CassieError::Storage(format!("delete role '{name}': {error}")))?;
        self.catalog.unregister_role(&normalized);
        self.bump_schema_epoch_and_invalidate_query_cache()?;
        Ok(())
    }

    pub fn metrics(&self) -> serde_json::Value {
        let snapshot = self.runtime.snapshot();
        serde_json::json!({
            "uptime_seconds": snapshot.runtime.uptime_seconds,
            "running_queries": snapshot.runtime.running_queries,
            "ready": self.is_started(),
            "auth_user": &self.auth_user,
            "runtime": snapshot.runtime,
            "query": snapshot.query,
            "rest": snapshot.rest,
            "pgwire": snapshot.pgwire,
            "search": snapshot.search,
            "vector": snapshot.vector,
            "hybrid": snapshot.hybrid,
            "storage": snapshot.storage,
            "plan_cache": snapshot.plan_cache,
            "query_cache": snapshot.query_cache,
            "cardinality": snapshot.cardinality,
            "feedback": snapshot.feedback,
            "adaptive_candidates": snapshot.adaptive_candidates,
            "covering_indexes": snapshot.covering_indexes,
            "parallel_scans": snapshot.parallel_scans,
        })
    }

    pub(crate) fn invalidate_plan_cache(&self) {
        self.runtime.invalidate_plan_cache();
    }

    pub(crate) fn bump_schema_epoch_and_invalidate_query_cache(&self) -> Result<(), CassieError> {
        let schema_epoch = self
            .midge
            .bump_schema_epoch()
            .map_err(|error| CassieError::Storage(format!("bump schema epoch: {error}")))?;
        self.runtime.record_storage_access("schema", true, true);
        self.runtime.set_schema_epoch(schema_epoch);
        self.runtime.invalidate_plan_cache();
        Ok(())
    }
}

fn build_embedding_provider(
    config: &CassieRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    match &config.embeddings {
        EmbeddingsRuntimeConfig::Disabled => Ok(Arc::new(LocalProvider)),
        EmbeddingsRuntimeConfig::Voyage => Ok(Arc::new(VoyageProvider)),
        EmbeddingsRuntimeConfig::Cohere => Ok(Arc::new(CohereProvider)),
        EmbeddingsRuntimeConfig::Local => Ok(Arc::new(LocalProvider)),
        EmbeddingsRuntimeConfig::OpenAI(runtime) => build_openai_provider(runtime),
        EmbeddingsRuntimeConfig::OpenAiCompatible(runtime) => {
            build_openai_compatible_provider(runtime)
        }
        EmbeddingsRuntimeConfig::Tei(runtime) => build_tei_provider(runtime),
        EmbeddingsRuntimeConfig::Ollama(runtime) => build_ollama_provider(runtime),
    }
}

fn build_openai_provider(
    runtime: &OpenAiRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let config = OpenAiProviderConfig {
        api_key: runtime.config.api_key.clone(),
        model: runtime.config.model.clone(),
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
    };

    let provider = OpenAiProvider::with_config(config)?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_openai_compatible_provider(
    runtime: &OpenAiCompatibleRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        api_key: runtime.api_key.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime.base_url.clone(),
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_tei_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_ollama_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

#[derive(Debug, Clone, PartialEq)]
struct ScoredVectorCandidate {
    distance: f64,
    id: String,
}

impl ScoredVectorCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_scored_vector_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for ScoredVectorCandidate {}

impl PartialOrd for ScoredVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredVectorCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_scored_vector_candidates(self, other)
    }
}

fn compare_scored_vector_candidates(
    left: &ScoredVectorCandidate,
    right: &ScoredVectorCandidate,
) -> CmpOrdering {
    left.distance
        .total_cmp(&right.distance)
        .then_with(|| left.id.cmp(&right.id))
}

fn vector_distance_for_metric(metric: &DistanceMetric, query: &[f32], target: &[f32]) -> f64 {
    if query.is_empty() || target.is_empty() || query.len() != target.len() {
        return f64::INFINITY;
    }

    match metric {
        DistanceMetric::Cosine => crate::vector::cosine_distance(query, target),
        DistanceMetric::L2 => crate::vector::l2_distance(query, target),
        DistanceMetric::Dot => crate::vector::dot_distance(query, target),
    }
}

fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut vector = Vec::with_capacity(values.len());
    for value in values {
        vector.push(value.as_f64()? as f32);
    }
    Some(vector)
}

fn vector_search_columns(schema: &CollectionSchema) -> Vec<ColumnMeta> {
    let mut columns = Vec::with_capacity(schema.fields.len() + 1);
    columns.push(ColumnMeta::from_data_type(
        "id".to_string(),
        crate::types::DataType::Text,
    ));
    for field in &schema.fields {
        if field.name != "id" {
            columns.push(ColumnMeta::from_data_type(
                field.name.clone(),
                field.data_type.clone(),
            ));
        }
    }
    columns
}

fn vector_search_row(schema: &CollectionSchema, document: DocumentRef) -> Vec<Value> {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push(Value::String(document.id));
    for field in &schema.fields {
        if field.name == "id" {
            continue;
        }
        let value = document
            .payload
            .get(&field.name)
            .map(|value| json_to_query_value(value, &field.data_type))
            .unwrap_or(Value::Null);
        row.push(value);
    }
    row
}

fn json_to_query_value(value: &serde_json::Value, data_type: &crate::types::DataType) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if matches!(data_type, crate::types::DataType::Vector(_)) {
        return vector_from_json(value)
            .map(|vector| Value::Vector(Vector::new(vector)))
            .unwrap_or(Value::Null);
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

fn project_payload_fields(payload: &serde_json::Value, fields: &[String]) -> serde_json::Value {
    let Some(object) = payload.as_object() else {
        return serde_json::Value::Object(serde_json::Map::new());
    };

    let mut projected = serde_json::Map::new();
    for field in fields {
        if let Some((_, value)) = object
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
        {
            projected.insert(field.clone(), value.clone());
        }
    }

    serde_json::Value::Object(projected)
}

fn hash_password(password: &str) -> Result<String, CassieError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| CassieError::Execution(format!("failed to hash role password: {error}")))
}

fn verify_password(hash: &str, password: &str) -> Result<bool, CassieError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|error| CassieError::Execution(format!("invalid password hash: {error}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

impl From<QueryError> for CassieError {
    fn from(value: QueryError) -> Self {
        CassieError::Execution(format!("{value:?}"))
    }
}

impl From<EmbeddingError> for CassieError {
    fn from(value: EmbeddingError) -> Self {
        match value {
            EmbeddingError::InvalidConfiguration(message) | EmbeddingError::ParseError(message) => {
                CassieError::InvalidEmbedding(message)
            }
            EmbeddingError::Unavailable { provider, reason } => {
                CassieError::EmbeddingUnavailable(format!("{provider}: {reason}"))
            }
            EmbeddingError::NotImplemented { provider } => CassieError::EmbeddingUnavailable(
                format!("embedding provider '{provider}' is not implemented"),
            ),
            EmbeddingError::Timeout { provider, message } => {
                CassieError::EmbeddingUnavailable(format!("{provider}: {message}"))
            }
            EmbeddingError::RetryExhausted {
                provider,
                attempts,
                message,
            } => CassieError::EmbeddingUnavailable(format!(
                "{provider}: exhausted retry attempts ({attempts}) after: {message}"
            )),
            EmbeddingError::RequestError(message) => CassieError::EmbeddingUnavailable(message),
        }
    }
}

impl From<std::string::FromUtf8Error> for CassieError {
    fn from(value: std::string::FromUtf8Error) -> Self {
        CassieError::Parse(value.to_string())
    }
}

impl From<crate::sql::SqlError> for CassieError {
    fn from(value: crate::sql::SqlError) -> Self {
        CassieError::Parse(value.0)
    }
}

impl From<cntryl_midge::MidgeError> for CassieError {
    fn from(value: cntryl_midge::MidgeError) -> Self {
        match value {
            cntryl_midge::MidgeError::WriteStall(message) => {
                CassieError::StorageRetryable(format!("midge write stalled: {message}"))
            }
            cntryl_midge::MidgeError::Fenced(message) => {
                CassieError::StorageRetryable(format!("midge fenced: {message}"))
            }
            cntryl_midge::MidgeError::NotFound => {
                CassieError::StorageMissingFamily("midge key not found".to_string())
            }
            cntryl_midge::MidgeError::InvalidArgument(message) => {
                if message.to_ascii_lowercase().contains("does not exist") {
                    CassieError::StorageMissingFamily(format!(
                        "midge family missing or invalid argument: {message}"
                    ))
                } else {
                    CassieError::Storage(message)
                }
            }
            other => CassieError::Storage(other.to_string()),
        }
    }
}
