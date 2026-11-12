use serde::{Deserialize, Serialize};

use crate::types::{DataType, Schema};

/// One registered finite built-in extension profile in a database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionMeta {
    pub format_version: u16,
    pub database: String,
    pub name: String,
    pub version: String,
    pub schema: String,
    pub owner_oid: u32,
    pub object_id: u64,
    pub digest_text_object_id: Option<u64>,
    pub digest_bytea_object_id: Option<u64>,
    pub vector_type_object_id: Option<u64>,
    pub vector_catalog_identities: Option<VectorCatalogIdentitySet>,
}

/// PostgreSQL catalog object IDs owned by the selected pgvector profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorCatalogIdentitySet {
    pub operators: VectorDistanceOperatorIds,
    pub hnsw: VectorAnnCatalogIdentityIds,
    pub ivfflat: VectorAnnCatalogIdentityIds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorDistanceOperatorIds {
    pub l2: u64,
    pub inner_product: u64,
    pub cosine: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorAnnCatalogIdentityIds {
    pub access_method: u64,
    pub l2_operator_family: u64,
    pub inner_product_operator_family: u64,
    pub cosine_operator_family: u64,
    pub l2_operator_class: u64,
    pub inner_product_operator_class: u64,
    pub cosine_operator_class: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Volatility {
    Immutable,
    Stable,
    Volatile,
}

impl From<crate::sql::ast::Volatility> for Volatility {
    fn from(value: crate::sql::ast::Volatility) -> Self {
        match value {
            crate::sql::ast::Volatility::Immutable => Self::Immutable,
            crate::sql::ast::Volatility::Stable => Self::Stable,
            crate::sql::ast::Volatility::Volatile => Self::Volatile,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionArgMeta {
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionMeta {
    pub name: String,
    pub args: Vec<FunctionArgMeta>,
    pub return_type: DataType,
    pub volatility: Volatility,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcedureMeta {
    pub name: String,
    pub args: Vec<FunctionArgMeta>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewMeta {
    /// Durable expression contract: zero is legacy; one admits selected null-safe predicates.
    #[serde(
        default,
        skip_serializing_if = "crate::sql::definition_guard::is_legacy"
    )]
    pub definition_semantics_version: u32,
    pub name: String,
    pub query: String,
    pub schema: Schema,
    /// Stable relation identity allocated at creation; renames keep it.
    #[serde(default)]
    pub object_id: u64,
    /// Selected extension identities required by this query, including dependencies
    /// inherited from referenced stored views.
    pub extension_dependency_ids: Vec<u64>,
}

impl ViewMeta {
    pub fn new(name: impl Into<String>, query: impl Into<String>, schema: Schema) -> Self {
        Self {
            definition_semantics_version: 0,
            name: name.into(),
            query: query.into(),
            schema,
            object_id: 0,
            extension_dependency_ids: Vec::new(),
        }
    }
}
