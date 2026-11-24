use serde::{Deserialize, Serialize};

pub use cassie_vector::types::{
    DistanceMetric, HnswGraphNode, HnswGraphState, HnswIndexOptions, IvfFlatIndexOptions,
    IvfFlatTrainingState, NormalizedVectorRecord, VectorIndexMetadata, VectorIndexRecord,
    VectorIndexState, VectorIndexType,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub values: Vec<f32>,
}
