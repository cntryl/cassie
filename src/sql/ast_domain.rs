use serde::{Deserialize, Serialize};

use crate::types::DataType;

/// A parsed domain `DEFAULT` clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainDefaultDefinition {
    /// The expression as written.
    pub sql: String,
    /// The literal value, before coercion to the base type.
    pub value: Option<serde_json::Value>,
    /// The canonical volatile expression such as `now()`.
    pub expression: Option<String>,
}

/// A parsed domain `CHECK` constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainCheckDefinition {
    pub name: Option<String>,
    /// The boolean expression text, with `VALUE` naming the checked value.
    pub sql: String,
}

/// One `ALTER DOMAIN` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DomainAlteration {
    SetDefault(DomainDefaultDefinition),
    DropDefault,
    SetNotNull,
    DropNotNull,
    AddCheck {
        check: DomainCheckDefinition,
        not_valid: bool,
    },
    DropConstraint {
        name: String,
        if_exists: bool,
    },
    ValidateConstraint {
        name: String,
    },
    RenameConstraint {
        from: String,
        to: String,
    },
    RenameTo(String),
}

/// One domain lifecycle operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DomainAction {
    Create {
        name: String,
        base: DataType,
        default: Option<DomainDefaultDefinition>,
        not_null: bool,
        checks: Vec<DomainCheckDefinition>,
    },
    Alter {
        name: String,
        alteration: DomainAlteration,
    },
    Drop {
        names: Vec<String>,
        if_exists: bool,
        cascade: bool,
    },
}

impl DomainAction {
    /// The first object name the action names.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Create { name, .. } | Self::Alter { name, .. } => name,
            Self::Drop { names, .. } => names.first().map_or("", String::as_str),
        }
    }

    /// The PostgreSQL command tag of the action.
    #[must_use]
    pub const fn command_tag(&self) -> &'static str {
        match self {
            Self::Create { .. } => "CREATE DOMAIN",
            Self::Alter { .. } => "ALTER DOMAIN",
            Self::Drop { .. } => "DROP DOMAIN",
        }
    }
}
