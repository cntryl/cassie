use serde::{Deserialize, Serialize};

use crate::types::DataType;

/// One attribute of a `CREATE TYPE ... AS (...)` definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAttribute {
    pub name: String,
    pub data_type: DataType,
}

/// One composite type lifecycle operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeAction {
    Create {
        name: String,
        attributes: Vec<TypeAttribute>,
    },
    Drop {
        names: Vec<String>,
        if_exists: bool,
        cascade: bool,
    },
    /// `CREATE`, `ALTER` or `DROP DOMAIN` (#828).
    Domain(super::DomainAction),
}

/// A finite `CREATE TYPE ... AS (...)` or `DROP TYPE` statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeStatement {
    pub action: TypeAction,
}

impl TypeStatement {
    /// The first object name the statement names.
    #[must_use]
    pub fn name(&self) -> &str {
        match &self.action {
            TypeAction::Create { name, .. } => name,
            TypeAction::Drop { names, .. } => names.first().map_or("", String::as_str),
            TypeAction::Domain(action) => action.name(),
        }
    }
}
