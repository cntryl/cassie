//! Plain catalog values shared with SQL syntax.

use serde::{Deserialize, Serialize};

use crate::types::DataType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    Scalar,
    FullText,
    Vector,
    Hybrid,
    Column,
    TimeSeries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollectionStorageMode {
    #[default]
    RowStore,
    ColumnIndexed,
    ColumnStore,
}

impl CollectionStorageMode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RowStore => "row-store",
            Self::ColumnIndexed => "column-indexed",
            Self::ColumnStore => "column-store",
        }
    }

    #[must_use]
    pub fn parse_option(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "row_store" | "row-store" => Some(Self::RowStore),
            "column_indexed" | "column-indexed" => Some(Self::ColumnIndexed),
            "column_store" | "column-store" => Some(Self::ColumnStore),
            _ => None,
        }
    }

    #[must_use]
    pub fn uses_column_store_storage(self) -> bool {
        matches!(self, Self::ColumnStore)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SequencePersistence {
    #[default]
    Permanent,
    Unlogged,
    Temporary,
}

impl SequencePersistence {
    /// `pg_class.relpersistence`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Permanent => "p",
            Self::Unlogged => "u",
            Self::Temporary => "t",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleObjectPrivilege {
    Usage,
    Select,
    Insert,
    Update,
    Delete,
}

/// Option clauses from `CREATE SEQUENCE`, `ALTER SEQUENCE` or an identity column.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SequenceOptions {
    pub data_type: Option<DataType>,
    pub increment: Option<i64>,
    /// `Some(None)` is `NO MINVALUE`.
    pub min_value: Option<Option<i64>>,
    /// `Some(None)` is `NO MAXVALUE`.
    pub max_value: Option<Option<i64>>,
    pub start: Option<i64>,
    /// `Some(None)` is `RESTART` without a value.
    pub restart: Option<Option<i64>>,
    pub cache: Option<i64>,
    pub cycle: Option<bool>,
}

/// How an identity column treats explicit values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityGeneration {
    Always,
    ByDefault,
}

impl IdentityGeneration {
    /// `information_schema.columns.identity_generation`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Always => "ALWAYS",
            Self::ByDefault => "BY DEFAULT",
        }
    }
}

/// Whether unique constraints treat null values as equal.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UniqueNullPolicy {
    #[default]
    Distinct,
    NotDistinct,
}
