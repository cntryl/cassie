use serde::{Deserialize, Serialize};

use super::IdentifierPath;

/// One parsed `LIKE source_table` element in a CREATE TABLE body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTableLikeClause {
    pub source: IdentifierPath,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<CreateTableLikeOption>,
}

/// An ordered option action following a CREATE TABLE LIKE clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreateTableLikeOption {
    Including(CreateTableLikeFeature),
    Excluding(CreateTableLikeFeature),
}

/// A PostgreSQL 18 CREATE TABLE LIKE option name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreateTableLikeFeature {
    All,
    Comments,
    Compression,
    Constraints,
    Defaults,
    Generated,
    Identity,
    Indexes,
    Statistics,
    Storage,
}
