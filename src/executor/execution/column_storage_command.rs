//! `ALTER TABLE ... ALTER COLUMN ... SET STORAGE | SET COMPRESSION` (#826).
//!
//! The settings are catalog metadata only; no stored row changes.
use super::{Cassie, QueryError};
use crate::app::CassieError;
use crate::catalog::ColumnStorageSetting;
use crate::sql::ast::AlterTableOperation;

/// Runs the `ALTER COLUMN` operations that change one column's default or
/// nullability flag, or its storage metadata.
pub(super) fn alter_column_flag(
    cassie: &Cassie,
    table: &str,
    operation: &AlterTableOperation,
) -> Result<(), QueryError> {
    match operation {
        AlterTableOperation::AlterColumnDropDefault { field } => {
            super::sequence_command::alter_column_drop_default(cassie, table, field)
        }
        AlterTableOperation::AlterColumnSetNotNull { field } => {
            super::sequence_command::alter_column_set_not_null(cassie, table, field)
        }
        AlterTableOperation::AlterColumnDropNotNull { field } => {
            super::sequence_command::alter_column_drop_not_null(cassie, table, field)
        }
        AlterTableOperation::AlterColumnStorage { field, setting } => {
            alter_column_storage(cassie, table, field, *setting)
        }
        _ => Err(QueryError::General(
            "unsupported ALTER COLUMN operation".to_owned(),
        )),
    }
}

fn alter_column_storage(
    cassie: &Cassie,
    table: &str,
    field: &str,
    setting: ColumnStorageSetting,
) -> Result<(), QueryError> {
    let field_key = crate::sql::ColumnIdentifierPath::stored_field_key(field);
    let data_type = cassie
        .catalog
        .get_schema(table)
        .and_then(|schema| {
            schema.fields.into_iter().find(|candidate| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.name) == field_key
            })
        })
        .map(|candidate| candidate.data_type)
        .ok_or_else(|| CassieError::Planner(format!("column '{field}' does not exist")))?;
    let (storage, compression) = match setting {
        ColumnStorageSetting::Storage(storage) => (storage, None),
        ColumnStorageSetting::Compression(compression) => (None, compression),
    };
    crate::catalog::column_storage::validate_column_storage(&data_type, storage, compression)
        .map_err(|message| crate::app::DiagnosticError::new("42804", message))
        .map_err(CassieError::from)?;
    super::sequence_command::mutate_field_constraint(cassie, table, field, |constraint| {
        match setting {
            ColumnStorageSetting::Storage(storage) => constraint.storage = storage,
            ColumnStorageSetting::Compression(compression) => {
                constraint.compression = compression;
            }
        }
    })
}
