use serde::{Deserialize, Serialize};

use super::IdentifierPath;
use crate::catalog::FieldConstraint;
use crate::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterTableStatement {
    pub table: IdentifierPath,
    pub operation: AlterTableOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlterTableOperation {
    AddColumn {
        field: String,
        data_type: DataType,
        #[serde(default)]
        if_not_exists: bool,
        #[serde(default)]
        constraints: Vec<FieldConstraint>,
    },
    AddConstraint {
        constraints: Vec<FieldConstraint>,
    },
    AddUniqueConstraintUsingIndex {
        constraint_name: Option<String>,
        index_name: String,
    },
    DropConstraint {
        name: String,
        if_exists: bool,
    },
    DropColumn {
        field: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    RenameTo {
        table: String,
    },
    SetAccessMethod {
        access_method: String,
    },
    AlterColumnSetDefault {
        field: String,
        default_value: Option<serde_json::Value>,
        default_expression: Option<String>,
        default_sequence: Option<String>,
    },
    AlterColumnDropDefault {
        field: String,
    },
    AlterColumnSetNotNull {
        field: String,
    },
    AlterColumnDropNotNull {
        field: String,
    },
    /// `ADD GENERATED ... AS IDENTITY`, `SET GENERATED`/sequence options, or
    /// `DROP IDENTITY`.
    AlterColumnIdentity {
        field: String,
        action: IdentityAlteration,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IdentityAlteration {
    Add {
        generation: crate::catalog::IdentityGeneration,
        options: crate::catalog::SequenceOptions,
    },
    Set {
        generation: Option<crate::catalog::IdentityGeneration>,
        options: crate::catalog::SequenceOptions,
    },
    Drop {
        if_exists: bool,
    },
}
