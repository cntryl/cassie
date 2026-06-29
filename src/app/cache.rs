use super::{DistanceMetric, NormalizedVectorRecord};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NormalizedVectorCacheKey {
    pub(super) catalog_version: u64,
    pub(super) collection: String,
    pub(super) field: String,
    pub(super) cardinality: usize,
}

#[derive(Debug)]
pub(super) struct NormalizedVectorCacheEntry {
    pub(super) ids: Vec<String>,
    pub(super) values: Vec<f32>,
    pub(super) magnitudes: Vec<f64>,
    pub(super) dimensions: usize,
    pub(super) metric: DistanceMetric,
    pub(super) first_record: Option<NormalizedVectorRecord>,
    pub(super) last_record: Option<NormalizedVectorRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct QueryEmbeddingCacheKey {
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) dimensions: usize,
    pub(super) query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct VectorSearchResultCacheKey {
    pub(super) catalog_version: u64,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) dimensions: usize,
    pub(super) collection: String,
    pub(super) field: String,
    pub(super) metric: String,
    pub(super) limit: usize,
    pub(super) offset: usize,
    pub(super) query: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PlanCacheProvenance {
    L1 {
        durable: bool,
        candidate_expires_at_ms: Option<u64>,
    },
    L2,
    Compiled,
}

pub(super) fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
