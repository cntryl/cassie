use serde::{Deserialize, Serialize};

use crate::types::DataType;

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
