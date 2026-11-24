use serde::{Deserialize, Serialize};

use super::IdentifierPath;
use crate::declarations::FieldConstraint;
use cassie_core::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct AlterTableStatement<A = ()> {
    pub table: IdentifierPath,
    #[serde(default)]
    pub if_exists: bool,
    pub operation: AlterTableOperation<A>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub enum AlterTableOperation<A = ()> {
    Multiple {
        operations: Vec<AlterTableOperation<A>>,
    },
    AddColumn {
        field: String,
        data_type: DataType,
        #[serde(default)]
        if_not_exists: bool,
        #[serde(default)]
        constraints: Vec<FieldConstraint<A>>,
    },
    AddConstraint {
        constraints: Vec<FieldConstraint<A>>,
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
        #[serde(default)]
        cascade: bool,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    RenameTo {
        table: String,
    },
    SetSchema {
        schema: String,
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
    /// `ALTER COLUMN ... SET STORAGE` or `SET COMPRESSION` (catalog metadata only).
    AlterColumnStorage {
        field: String,
        setting: cassie_core::catalog::column_storage::ColumnStorageSetting,
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
    AlterColumnType {
        field: String,
        data_type: DataType,
        using: super::Expr<A>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IdentityAlteration {
    Add {
        generation: cassie_core::catalog::declarations::IdentityGeneration,
        options: cassie_core::catalog::declarations::SequenceOptions,
    },
    Set {
        generation: Option<cassie_core::catalog::declarations::IdentityGeneration>,
        options: cassie_core::catalog::declarations::SequenceOptions,
    },
    Drop {
        if_exists: bool,
    },
}
