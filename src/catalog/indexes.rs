use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    Scalar,
    FullText,
    Vector,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub collection: String,
    pub name: String,
    pub field: String,
    pub kind: IndexKind,
    pub unique: bool,
    pub options: std::collections::BTreeMap<String, String>,
}
