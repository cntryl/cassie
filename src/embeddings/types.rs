use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DistanceMetric {
    Cosine,
    L2,
    Dot,
}

impl DistanceMetric {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cosine => "cosine",
            Self::L2 => "l2",
            Self::Dot => "dot",
        }
    }

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VectorIndexType {
    BruteForce,
    Hnsw,
}

impl VectorIndexType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BruteForce => "bruteforce",
            Self::Hnsw => "hnsw",
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
