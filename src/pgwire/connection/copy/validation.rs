//! COPY option validation against the selected descriptor.
use super::{request, Cassie, CassieError, CopyRequest, CopyStatement};

pub(super) fn validate_force_columns(
    cassie: &Cassie,
    request: &CopyRequest,
    statement: &CopyStatement,
) -> Result<(), CassieError> {
    let fields = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".to_string()))?;
    let columns = crate::app::copy_columns(statement, &fields.fields)?;
    for (option, selection) in [
        ("FORCE_NULL", &request.force_null),
        ("FORCE_NOT_NULL", &request.force_not_null),
    ] {
        let request::ColumnSelection::Columns(names) = selection else {
            continue;
        };
        for name in names {
            let copied = columns.iter().any(|column| match column {
                crate::app::CopyColumn::Field(field) => field.name == *name,
                crate::app::CopyColumn::RowId => name == "id" || name == "_id",
            });
            if copied {
                continue;
            }
            let exists = fields.fields.iter().any(|field| field.name == *name)
                || crate::types::row_identity::declares_id(
                    fields.fields.iter().map(|field| field.name.as_str()),
                ) && (name == "id" || name == "_id");
            let (code, message) = if exists {
                (
                    "42P10",
                    format!("{option} column \"{name}\" not referenced by COPY"),
                )
            } else {
                ("42703", format!("column \"{name}\" does not exist"))
            };
            return Err(CassieError::Diagnostic(Box::new(
                crate::app::DiagnosticError::new(code, message),
            )));
        }
    }
    Ok(())
}

pub(super) fn copy_response_column_count(cassie: &Cassie, statement: &CopyStatement) -> usize {
    if !statement.columns.is_empty() {
        return statement.columns.len();
    }
    cassie
        .catalog
        .get_schema(&statement.table)
        .map_or(0, |schema| schema.fields.len())
}

pub(super) fn copy_target_columns(
    cassie: &Cassie,
    statement: &CopyStatement,
) -> Result<Vec<crate::app::CopyColumn>, CassieError> {
    let fields = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".to_string()))?;
    crate::app::copy_columns(statement, &fields.fields)
}
