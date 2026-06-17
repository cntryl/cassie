use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    FullText,
    Vector,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub collection: String,
    pub field: String,
    pub kind: IndexKind,
    pub options: std::collections::BTreeMap<String, String>,
}
