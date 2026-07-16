use super::{Cassie, QueryError, QueryResult};
use crate::catalog::DefaultSequenceOwnership;

pub(super) fn create_sequence(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateSequenceStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.sequence_exists(&statement.name) {
        return Ok(empty_command("CREATE SEQUENCE"));
    }
    if cassie.catalog.sequence_exists(&statement.name) {
        return Err(QueryError::General(format!(
            "sequence '{}' already exists",
            statement.name
        )));
    }

    let metadata = crate::catalog::SequenceMeta::new(&statement.name, statement.data_type.clone());
    cassie
        .midge
        .put_sequence(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_sequence(metadata);
    Ok(empty_command("CREATE SEQUENCE"))
}

pub(super) fn drop_sequence(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropSequenceStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.sequence_exists(&statement.name) {
        return Ok(empty_command("DROP SEQUENCE"));
    }
    if !cassie.catalog.sequence_exists(&statement.name) {
        return Err(QueryError::General(format!(
            "sequence '{}' does not exist",
            statement.name
        )));
    }

    cassie
        .midge
        .delete_sequence(&statement.name)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_sequence(&statement.name);
    Ok(empty_command("DROP SEQUENCE"))
}

pub(super) fn prepare_create_table_sequences(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateTableStatement,
) -> Result<Vec<crate::catalog::SequenceMeta>, QueryError> {
    let mut created = Vec::new();
    for field in &statement.fields {
        for constraint in &field.constraints {
            let Some(sequence) = constraint.default_sequence.as_deref() else {
                continue;
            };
            if constraint.default_sequence_owned.is_owned() {
                if cassie.catalog.sequence_exists(sequence) {
                    return Err(QueryError::General(format!(
                        "sequence '{sequence}' already exists"
                    )));
                }
                created.push(crate::catalog::SequenceMeta::new(
                    sequence,
                    field.data_type.clone(),
                ));
            } else if !cassie.catalog.sequence_exists(sequence) {
                return Err(QueryError::General(format!(
                    "sequence '{sequence}' does not exist"
                )));
            }
        }
    }
    Ok(created)
}

pub(super) fn persist_created_sequences(
    cassie: &Cassie,
    sequences: Vec<crate::catalog::SequenceMeta>,
) -> Result<(), QueryError> {
    for sequence in sequences {
        cassie
            .midge
            .put_sequence(&sequence)
            .map_err(QueryError::from)?;
        cassie.catalog.register_sequence(sequence);
    }
    Ok(())
}

pub(super) fn alter_column_set_default(
    cassie: &Cassie,
    table: &str,
    field: &str,
    default_value: Option<serde_json::Value>,
    default_expression: Option<String>,
    default_sequence: Option<String>,
) -> Result<(), QueryError> {
    if let Some(sequence) = default_sequence.as_deref() {
        if !cassie.catalog.sequence_exists(sequence) {
            return Err(QueryError::General(format!(
                "sequence '{sequence}' does not exist"
            )));
        }
    }

    mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.default_value = default_value;
        constraint.default_expression = default_expression;
        constraint.default_sequence = default_sequence;
        constraint.default_sequence_owned = DefaultSequenceOwnership::shared();
    })
}

pub(super) fn alter_column_drop_default(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.default_value = None;
        constraint.default_expression = None;
        constraint.default_sequence = None;
        constraint.default_sequence_owned = DefaultSequenceOwnership::shared();
    })
}

pub(super) fn alter_column_set_not_null(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    for document in cassie
        .midge
        .scan_documents(table)
        .map_err(QueryError::from)?
    {
        let value = document.payload.get(field);
        if value.is_none_or(serde_json::Value::is_null) {
            return Err(QueryError::Cassie(
                crate::app::CassieError::NotNullViolation {
                    table: table.to_string(),
                    column: field.to_string(),
                    constraint: Some(crate::catalog::generated_constraint_name(
                        table, field, "NOT NULL",
                    )),
                },
            ));
        }
    }

    mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.not_null = true;
        constraint.not_null_ownership = constraint.not_null_ownership.with_explicit();
    })
}

pub(super) fn alter_column_drop_not_null(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.not_null = false;
        constraint.not_null_ownership = crate::catalog::NotNullOwnership::None;
    })
}

fn mutate_field_constraint(
    cassie: &Cassie,
    table: &str,
    field: &str,
    mutate: impl FnOnce(&mut crate::catalog::FieldConstraint),
) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let position = constraints
        .iter()
        .position(|constraint| constraint.field.eq_ignore_ascii_case(field));
    let mut constraint = position
        .and_then(|position| constraints.get(position).cloned())
        .unwrap_or_else(|| crate::catalog::FieldConstraint::new(field));
    mutate(&mut constraint);

    if let Some(position) = position {
        if constraint_is_populated(&constraint) {
            constraints[position] = constraint;
        } else {
            constraints.remove(position);
        }
    } else if constraint_is_populated(&constraint) {
        constraints.push(constraint);
    }

    cassie
        .midge
        .save_constraints(table, constraints.as_slice())
        .map_err(QueryError::from)?;
    cassie.catalog.register_constraints(table, constraints);
    Ok(())
}

fn constraint_is_populated(constraint: &crate::catalog::FieldConstraint) -> bool {
    constraint.primary_key
        || constraint.unique
        || constraint.not_null
        || constraint.default_value.is_some()
        || constraint.default_expression.is_some()
        || constraint.default_sequence.is_some()
        || constraint.check.is_some()
        || constraint.references_table.is_some()
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
