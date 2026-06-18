use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::catalog::{Catalog, ConstraintCheck, ConstraintOperator, FieldConstraint};
use crate::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use crate::embeddings::{
    cohere::CohereProvider,
    local::LocalProvider,
    openai::{OpenAiProvider, OpenAiProviderConfig},
    voyage::VoyageProvider,
    DistanceMetric, Embedding, EmbeddingError, EmbeddingProvider, VectorIndexRecord,
};
use crate::executor::{QueryError, QueryResult};
use crate::midge::adapter::Midge;
use crate::runtime::{ExecutionMode, PlanCacheKey, RuntimeState};
use crate::sql::{binder, parser};

#[derive(Debug, Clone, Serialize)]
pub struct CassieSession {
    pub user: String,
    pub database: Option<String>,
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
        Ok(Self {
            midge,
            catalog: Catalog::new(),
            embedding_provider,
            runtime,
            auth_user,
            started: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn hydrate_catalog(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        self.catalog.clear().await;
        self.invalidate_plan_cache();

        let namespaces = self.midge.list_namespaces().await;
        self.runtime.record_storage_access("schema", false, true);
        for namespace in namespaces {
            self.catalog.register_namespace(&namespace, None).await;
        }

        let mut collections = self.midge.list_collections().await;
        self.runtime.record_storage_access("schema", false, true);
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema().await;
            self.runtime.record_storage_access("schema", false, true);
        }

        for name in collections {
            self.runtime.record_storage_access("schema", false, true);
            if let Some(schema) = self.midge.collection_schema(&name).await {
                let constraints = self.midge.load_constraints(&name).await.map_err(|error| {
                    self.runtime.record_storage_access("schema", false, false);
                    CassieError::Storage(format!(
                        "load constraints for collection '{name}': {error}"
                    ))
                })?;
                self.runtime.record_storage_access("schema", false, true);
                self.catalog
                    .register_collection_with_constraints(
                        &name,
                        schema
                            .fields
                            .into_iter()
                            .map(|field| (field.name, field.data_type))
                            .collect(),
                        constraints,
                    )
                    .await;
            }
        }

        let indexes = self.midge.list_vector_indexes().await.map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list vector indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_vector_index(index).await;
        }

        let indexes = self.midge.list_indexes().await.map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list indexes: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for index in indexes {
            self.catalog.register_index(index).await;
        }

        let functions = self.midge.list_functions().await.map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list functions: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in functions {
            self.catalog.register_function(metadata).await;
        }

        let procedures = self.midge.list_procedures().await.map_err(|error| {
            self.runtime.record_storage_access("schema", false, false);
            CassieError::Storage(format!("list procedures: {error}"))
        })?;
        self.runtime.record_storage_access("schema", false, true);
        for metadata in procedures {
            self.catalog.register_procedure(metadata).await;
        }

        self.runtime.record_catalog_hydration(started_at.elapsed());
        Ok(())
    }

    pub async fn startup(&self) -> Result<(), CassieError> {
        let started_at = Instant::now();
        let families_ready = self.midge.ensure_families_ready();
        self.runtime
            .record_storage_access("schema", true, families_ready.is_ok());
        families_ready.map_err(|error| {
            CassieError::StorageBootstrap(format!("bootstrap families: {error}"))
        })?;

        let clear_temp = self.midge.clear_temp_family().await;
        self.runtime
            .record_storage_access("temp", true, clear_temp.is_ok());
        clear_temp.map_err(|error| CassieError::Storage(format!("clear temp family: {error}")))?;

        self.hydrate_catalog()
            .await
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.runtime.mark_started();
        self.runtime.record_startup(started_at.elapsed());
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub async fn shutdown(&self) {
        if self.started.swap(false, Ordering::SeqCst) {
            self.runtime.record_shutdown();
            self.runtime.mark_shutdown();
        }
    }

    pub async fn execute_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryResult, CassieError> {
        self.execute_sql_with_mode(session, sql, params, ExecutionMode::SimpleQuery)
            .await
    }

    pub async fn describe_sql(&self, sql: &str) -> Result<Vec<String>, CassieError> {
        let parsed = parser::parse_statement(sql)?;
        let controls = self.runtime.query_controls(Instant::now());
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let key = PlanCacheKey {
            normalized_sql: crate::runtime::normalized_sql(&parsed),
            catalog_version: self.catalog_version(),
            parameter_shape: Vec::new(),
            mode: ExecutionMode::DescribeQuery,
        };

        let physical = if let Some(plan) = self.runtime.plan_cache_lookup(&key) {
            plan
        } else {
            let bound = binder::bind(parsed, &self.catalog).await?;
            let logical = crate::planner::logical::plan(&bound)?;
            let optimized = crate::planner::optimizer::optimize(logical);
            let physical = crate::planner::physical::build(optimized);
            self.runtime.plan_cache_store(key, physical.clone());
            physical
        };

        if physical.logical.command.is_some() {
            return Ok(Vec::new());
        }

        Ok(
            crate::executor::columns_from_projection(&physical.logical.projection)
                .into_iter()
                .map(|column| column.name)
                .collect(),
        )
    }

    pub(crate) async fn execute_sql_with_mode(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
    ) -> Result<QueryResult, CassieError> {
        let query_started = Instant::now();
        let running_guard = self.runtime.begin_running_query();
        let result = self
            .execute_sql_inner(session, sql, params, mode, query_started)
            .await;
        let elapsed = query_started.elapsed();

        match &result {
            Ok(result) => self
                .runtime
                .record_query_success(elapsed, result.rows.len()),
            Err(error) => self.runtime.record_query_error(elapsed, error),
        }

        drop(running_guard);
        result
    }

    async fn execute_sql_inner(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
        mode: ExecutionMode,
        started_at: Instant,
    ) -> Result<QueryResult, CassieError> {
        if session.user.is_empty() {
            return Err(CassieError::Unauthorized);
        }

        let controls = self.runtime.query_controls(started_at);
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let parsed = parser::parse_statement(sql)?;
        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let key = PlanCacheKey {
            normalized_sql: crate::runtime::normalized_sql(&parsed),
            catalog_version: self.catalog_version(),
            parameter_shape: crate::runtime::parameter_shape(&params),
            mode,
        };

        let physical = if let Some(plan) = self.runtime.plan_cache_lookup(&key) {
            plan
        } else {
            let bound = binder::bind(parsed, &self.catalog).await?;
            if controls.is_timed_out() {
                return Err(CassieError::Execution("query timeout exceeded".to_string()));
            }

            let logical = crate::planner::logical::plan(&bound)?;
            let optimized = crate::planner::optimizer::optimize(logical);
            let physical = crate::planner::physical::build(optimized);
            self.runtime.plan_cache_store(key, physical.clone());
            physical
        };

        if controls.is_timed_out() {
            return Err(CassieError::Execution("query timeout exceeded".to_string()));
        }

        let result = crate::executor::run_with_controls(self, physical, params, &controls)
            .await
            .map_err(CassieError::from)?;

        if result.rows.len() > controls.max_result_rows {
            return Err(CassieError::Execution(format!(
                "query result row limit exceeded: {} > {}",
                result.rows.len(),
                controls.max_result_rows
            )));
        }

        Ok(result)
    }

    pub async fn execute_vector_search(
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
            .await
            .ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "vector index not found for collection '{collection}', field '{vector_field}'"
                ))
            })?;

        self.validate_embedding_compatibility(&index, metric.as_ref())
            .await?;

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .map_err(CassieError::from)?;
        self.validate_embedding_payload(&index, &embedding)?;

        let vector_text = serde_json::to_string(&embedding.values).map_err(|error| {
            CassieError::InvalidEmbedding(format!("invalid vector serialization: {error}"))
        })?;
        let metric = metric.unwrap_or(index.metadata.metric.clone());
        let operator = metric.sql_operator();
        let session = self.create_session("postgres", None).await;

        let sql = format!(
            "SELECT * FROM {} ORDER BY {} {} '{}' LIMIT {} OFFSET {}",
            collection,
            vector_field,
            operator,
            vector_text,
            limit.max(1),
            offset
        );
        let result = self.execute_sql(&session, &sql, Vec::new()).await;
        result
    }

    pub async fn ingest_document(
        &self,
        collection: &str,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        self.write_document(collection, None, payload, true, None)
            .await
    }

    pub(crate) async fn write_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<String, CassieError> {
        let payload = self
            .prepare_document_write(collection, payload, apply_defaults, exclude_id)
            .await?;

        self.midge.put_document(collection, id, payload).await
    }

    pub(crate) async fn prepare_document_write(
        &self,
        collection: &str,
        mut payload: serde_json::Value,
        apply_defaults: bool,
        exclude_id: Option<&str>,
    ) -> Result<serde_json::Value, CassieError> {
        let constraints = self.catalog.get_constraints(collection).await;
        if apply_defaults && !constraints.is_empty() {
            self.apply_default_values(&mut payload, &constraints)?;
        }

        self.validate_payload_schema(collection, &payload).await?;

        let indexes = self.catalog.list_vector_indexes(collection).await;
        if !indexes.is_empty() {
            self.apply_vector_indexes(collection, &mut payload, indexes.as_slice())
                .await?;
        }

        self.validate_constraints(collection, &payload, &constraints, exclude_id)
            .await?;

        Ok(payload)
    }

    async fn validate_payload_schema(
        &self,
        collection: &str,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let schema = self
            .catalog
            .get_schema(collection)
            .await
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

            match expected {
                crate::types::DataType::Int => match value {
                    serde_json::Value::Number(number) => {
                        if number.as_i64().is_none() {
                            return Err(CassieError::InvalidVector(format!(
                                "field '{field}' expects int"
                            )));
                        }
                    }
                    serde_json::Value::Null => {}
                    _ => {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects int"
                        )));
                    }
                },
                crate::types::DataType::Float => match value {
                    serde_json::Value::Number(_) => {}
                    serde_json::Value::Null => {}
                    _ => {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects float"
                        )));
                    }
                },
                crate::types::DataType::Boolean => match value {
                    serde_json::Value::Bool(_) => {}
                    serde_json::Value::Null => {}
                    _ => {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects boolean"
                        )));
                    }
                },
                crate::types::DataType::Text => match value {
                    serde_json::Value::String(_) => {}
                    serde_json::Value::Null => {}
                    _ => {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects text"
                        )));
                    }
                },
                crate::types::DataType::Json => {
                    if !value.is_object()
                        && !value.is_array()
                        && !value.is_string()
                        && !value.is_number()
                        && !value.is_boolean()
                        && !value.is_null()
                    {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects json"
                        )));
                    }
                }
                crate::types::DataType::Vector(size) => {
                    let Some(array) = value.as_array() else {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects vector({size})"
                        )));
                    };
                    if array.len() != size {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects vector({size})"
                        )));
                    }
                    if array.iter().any(|value| value.as_f64().is_none()) {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{field}' expects vector({size})"
                        )));
                    }
                }
            }
        }

        Ok(())
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

    async fn validate_constraints(
        &self,
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

        self.validate_uniques(collection, object, constraints, exclude_id)
            .await
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

    async fn validate_uniques(
        &self,
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

            if self
                .value_exists_for_collection_field(collection, &constraint.field, value, exclude_id)
                .await?
            {
                return Err(CassieError::InvalidVector(format!(
                    "unique constraint failed for '{}'",
                    constraint.field
                )));
            }
        }

        Ok(())
    }

    async fn value_exists_for_collection_field(
        &self,
        collection: &str,
        field: &str,
        value: &serde_json::Value,
        exclude_id: Option<&str>,
    ) -> Result<bool, CassieError> {
        for document in self.midge.scan_documents(collection).await? {
            if exclude_id.is_some_and(|id| document.id == id) {
                continue;
            }

            if document.payload.get(field) == Some(value) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn apply_vector_indexes(
        &self,
        _collection: &str,
        payload: &mut serde_json::Value,
        indexes: &[VectorIndexRecord],
    ) -> Result<(), CassieError> {
        let object = payload.as_object_mut().ok_or_else(|| {
            CassieError::InvalidEmbedding("document payload must be a JSON object".to_string())
        })?;

        for index in indexes {
            self.validate_embedding_compatibility(index, None).await?;

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

    async fn validate_embedding_compatibility(
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

    pub async fn register_collection(&self, name: impl Into<String>, schema: crate::types::Schema) {
        let name = name.into();
        self.catalog
            .register_collection(
                &name,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;
        self.invalidate_plan_cache();
    }

    pub async fn register_vector_index(&self, index: VectorIndexRecord) {
        self.catalog.register_vector_index(index).await;
        self.invalidate_plan_cache();
    }

    pub async fn health(&self) -> serde_json::Value {
        let ready = self.is_started();
        let collections = self.midge.list_collections().await;
        serde_json::json!({
            "status": if ready { "ok" } else { "starting" },
            "ready": ready,
            "collections": collections.len(),
            "version": env!("CARGO_PKG_VERSION")
        })
    }

    pub async fn create_session(&self, user: &str, database: Option<String>) -> CassieSession {
        CassieSession {
            user: user.to_string(),
            database,
        }
    }

    pub async fn metrics(&self) -> serde_json::Value {
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
        })
    }

    pub(crate) fn invalidate_plan_cache(&self) {
        self.runtime.invalidate_plan_cache();
    }

    fn catalog_version(&self) -> u64 {
        self.catalog.version()
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
