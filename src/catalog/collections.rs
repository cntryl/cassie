use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub name: String,
    pub description: Option<String>,
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

impl NamespaceMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            description,
        }
    }
}
