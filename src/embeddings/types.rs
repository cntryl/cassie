use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DistanceMetric {
    Cosine,
    L2,
    Dot,
}

impl DistanceMetric {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cosine => "cosine",
            Self::L2 => "l2",
            Self::Dot => "dot",
        }
    }

    #[must_use]
    pub fn sql_operator(&self) -> &'static str {
        match self {
            Self::Cosine => "<=>",
            Self::L2 => "<->",
            Self::Dot => "<#>",
        }
    }
}

impl FromStr for DistanceMetric {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "cosine" => Ok(Self::Cosine),
            "l2" | "euclidean" | "euclidean_distance" => Ok(Self::L2),
            "dot" | "dot_product" => Ok(Self::Dot),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorIndexMetadata {
    pub provider: String,
    pub model: String,
    pub dimensions: usize,
    pub metric: DistanceMetric,
    #[serde(default = "default_vector_index_type")]
    pub index_type: VectorIndexType,
    #[serde(default)]
    pub hnsw: Option<HnswIndexOptions>,
    #[serde(default)]
    pub hnsw_graph: Option<HnswGraphState>,
    #[serde(default)]
    pub ivfflat: Option<IvfFlatIndexOptions>,
    #[serde(default)]
    pub ivfflat_training: Option<IvfFlatTrainingState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VectorIndexType {
    BruteForce,
    Hnsw,
    IvfFlat,
}

impl VectorIndexType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BruteForce => "bruteforce",
            Self::Hnsw => "hnsw",
            Self::IvfFlat => "ivfflat",
        }
    }
}

fn default_vector_index_type() -> VectorIndexType {
    VectorIndexType::BruteForce
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HnswIndexOptions {
    pub version: u32,
    pub m: usize,
    pub ef_construction: usize,
    pub ef_search: usize,
}

impl Default for HnswIndexOptions {
    fn default() -> Self {
        Self {
            version: 1,
            m: 16,
            ef_construction: 64,
            ef_search: 40,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HnswGraphState {
    pub version: u32,
    #[serde(default)]
    pub source_fingerprint: u64,
    pub row_count: usize,
    pub dimensions: usize,
    pub metric: DistanceMetric,
    pub entry_point: Option<String>,
    pub max_layer: usize,
    pub nodes: Vec<HnswGraphNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HnswGraphNode {
    pub id: String,
    pub vector: Vec<f32>,
    pub magnitude: f64,
    pub layers: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IvfFlatIndexOptions {
    pub version: u32,
    pub lists: usize,
    pub probes: usize,
    pub training_sample_size: usize,
    pub training_seed: u64,
}

impl Default for IvfFlatIndexOptions {
    fn default() -> Self {
        Self {
            version: 1,
            lists: 64,
            probes: 1,
            training_sample_size: 2_560,
            training_seed: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IvfFlatTrainingState {
    pub version: u32,
    #[serde(default)]
    pub source_fingerprint: u64,
    pub trained: bool,
    pub row_count: usize,
    pub lists: usize,
    pub probes: usize,
    pub training_seed: u64,
    pub centroid_ids: Vec<String>,
    pub centroids: Vec<Vec<f32>>,
    pub assignments: BTreeMap<String, usize>,
    pub list_sizes: Vec<usize>,
}

/// Mutable, derived accelerator state for one immutable vector-index definition.
///
/// This record is deliberately stored in the data family. Keeping it separate
/// from [`VectorIndexMetadata`] lets document writes publish rows, normalized
/// vectors, and their accelerator state in the same transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VectorIndexState {
    #[serde(default)]
    pub hnsw_graph: Option<HnswGraphState>,
    #[serde(default)]
    pub ivfflat_training: Option<IvfFlatTrainingState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorIndexRecord {
    pub collection: String,
    pub field: String,
    pub source_field: String,
    pub metadata: VectorIndexMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedVectorRecord {
    pub collection: String,
    pub field: String,
    pub id: String,
    pub dimensions: usize,
    pub metric: DistanceMetric,
    pub normalization_version: u32,
    pub payload_available: bool,
    pub magnitude: f64,
    pub values: Vec<f32>,
}

impl NormalizedVectorRecord {
    pub const CURRENT_NORMALIZATION_VERSION: u32 = 1;
}
