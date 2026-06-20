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
