use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;

use crate::catalog::Catalog;
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
        Ok(Self {
            midge,
            catalog: Catalog::new(),
            embedding_provider,
            started: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn hydrate_catalog(&self) -> Result<(), CassieError> {
        self.catalog.clear().await;

        let mut collections = self.midge.list_collections().await;
        if collections.is_empty() {
            collections = self.midge.list_collections_from_schema().await;
        }

        for name in collections {
            if let Some(schema) = self.midge.collection_schema(&name).await {
                self.catalog
                    .register_collection(
                        &name,
                        schema
                            .fields
                            .into_iter()
                            .map(|field| (field.name, field.data_type))
                            .collect(),
                    )
                    .await;
            }
        }

        let indexes = self.midge.list_vector_indexes().await?;
        for index in indexes {
            self.catalog.register_vector_index(index).await;
        }

        Ok(())
    }

    pub async fn startup(&self) -> Result<(), CassieError> {
        self.midge.ensure_families_ready().map_err(|error| {
            CassieError::StorageBootstrap(format!("bootstrap families: {error}"))
        })?;
        self.midge
            .clear_temp_family()
            .await
            .map_err(|error| CassieError::Storage(format!("clear temp family: {error}")))?;
        self.hydrate_catalog()
            .await
            .map_err(|error| CassieError::Storage(format!("catalog hydration: {error}")))?;
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub async fn execute_sql(
        &self,
        session: &CassieSession,
        sql: &str,
        params: Vec<crate::types::Value>,
    ) -> Result<QueryResult, CassieError> {
        if session.user.is_empty() {
            return Err(CassieError::Unauthorized);
        }

        let parsed = parser::parse_statement(sql)?;
        let bound = binder::bind(parsed, &self.catalog).await?;
        let logical = crate::planner::logical::plan(&bound)?;
        let optimized = crate::planner::optimizer::optimize(logical);
        let physical = crate::planner::physical::build(optimized);
        crate::executor::run(self, physical, params)
            .await
            .map_err(CassieError::from)
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
        self.execute_sql(&session, &sql, Vec::new()).await
    }

    pub async fn ingest_document(
        &self,
        collection: &str,
        mut payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        let indexes = self.catalog.list_vector_indexes(collection).await;
        if !indexes.is_empty() {
            self.apply_vector_indexes(collection, &mut payload, indexes.as_slice())
                .await?;
        }

        self.midge.put_document(collection, None, payload).await
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
    }

    pub async fn register_vector_index(&self, index: VectorIndexRecord) {
        self.catalog.register_vector_index(index).await;
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
        serde_json::json!({
            "uptime_seconds": 0,
            "running_queries": 0,
            "ready": self.is_started(),
            "auth_user": "postgres"
        })
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
