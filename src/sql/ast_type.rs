use serde::{Deserialize, Serialize};

use crate::types::DataType;

/// One attribute of a `CREATE TYPE ... AS (...)` definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAttribute {
    pub name: String,
    pub data_type: DataType,
}

/// One `ALTER TYPE` change of a composite type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeAlteration {
    RenameTo { new_name: String },
    RenameAttribute { from: String, to: String },
    AddAttribute { attribute: TypeAttribute },
    DropAttribute { name: String, if_exists: bool },
    AlterAttributeType { name: String, data_type: DataType },
    SetSchema { schema: String },
    OwnerTo { owner: String },
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
    Alter {
        name: String,
        alteration: TypeAlteration,
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
            TypeAction::Create { name, .. } | TypeAction::Alter { name, .. } => name,
            TypeAction::Drop { names, .. } => names.first().map_or("", String::as_str),
            TypeAction::Domain(action) => action.name(),
        }
    }
}
