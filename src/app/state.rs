use super::{
    Arc, AtomicBool, BTreeMap, Catalog, Midge, Mutex, NormalizedVectorCacheEntry,
    NormalizedVectorCacheKey, QueryEmbeddingCacheKey, QueryResult, RuntimeState, Serialize,
    VectorSearchResultCacheKey,
};
use crate::embeddings::EmbeddingProvider;

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
    pub(super) normalized_vector_cache:
        Arc<Mutex<BTreeMap<NormalizedVectorCacheKey, Arc<NormalizedVectorCacheEntry>>>>,
    pub(super) query_embedding_cache: Arc<Mutex<BTreeMap<QueryEmbeddingCacheKey, Arc<Vec<f32>>>>>,
    pub(super) vector_search_result_cache:
        Arc<Mutex<BTreeMap<VectorSearchResultCacheKey, Arc<QueryResult>>>>,
    pub(crate) auth_user: String,
    pub(crate) auth_password: String,
    pub(crate) default_database: String,
    pub(crate) rest_tls_cert_file: Option<String>,
    pub(crate) rest_tls_key_file: Option<String>,
    pub started: Arc<AtomicBool>,
}

impl Cassie {}
