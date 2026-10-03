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
        constraints: Vec<FieldConstraint>,
    },
    AddConstraint {
        constraints: Vec<FieldConstraint>,
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
}
