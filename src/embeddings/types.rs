use serde::{Deserialize, Serialize};

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

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "cosine" => Some(Self::Cosine),
            "l2" | "euclidean" | "euclidean_distance" => Some(Self::L2),
            "dot" | "dot_product" => Some(Self::Dot),
            _ => None,
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
