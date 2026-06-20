use serde::{Deserialize, Serialize};

pub fn is_reserved_namespace(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "information_schema" | "pg_catalog" | "public"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRebuildState {
    Idle,
    Rebuilding,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionMeta {
    pub collection: String,
    pub schema_version: u32,
    pub offset: u64,
    pub lag: u64,
    pub rebuild_state: ProjectionRebuildState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMeta {
    pub name: String,
    pub description: Option<String>,
}

impl CollectionMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
}

impl ProjectionMeta {
    pub fn new(collection: impl Into<String>, schema_version: u32) -> Self {
        Self {
            collection: collection.into(),
            schema_version,
            offset: 0,
            lag: 0,
            rebuild_state: ProjectionRebuildState::Idle,
        }
    }
}

impl NamespaceMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
}
