use super::command_output::empty as empty_command;
use super::{Cassie, QueryError, QueryResult};
use crate::catalog::DefaultSequenceOwnership;
use crate::runtime::QueryExecutionControls;

pub(super) fn create_sequence(
    cassie: &Cassie,
    statement: &crate::sql::ast::CreateSequenceStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.sequence_exists(&statement.name) {
        return empty_command("CREATE SEQUENCE", controls);
    }
    if cassie.catalog.sequence_exists(&statement.name) {
        return Err(QueryError::General(format!(
            "sequence '{}' already exists",
            statement.name
        )));
    }

    let parameters =
        crate::catalog::SequenceParameters::resolve(None, &statement.options, &statement.data_type)
            .map_err(|error| QueryError::Cassie(error.into()))?;
    let mut metadata = crate::catalog::SequenceMeta::with_parameters(&statement.name, parameters);
    metadata.persistence = statement.persistence;
    metadata.owned_by = sequence_owner(statement.owned_by.as_ref());
    let mut metadata = canonicalize_sequence_metadata(cassie, metadata);
    let output = empty_command("CREATE SEQUENCE", controls)?;
    metadata.object_id = cassie
        .midge
        .allocate_object_id()
        .map_err(QueryError::from)?;
    cassie
        .midge
        .put_sequence(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_sequence(metadata);
    Ok(output)
}

/// `ALTER SEQUENCE`: re-resolves the options over the current parameters,
/// applies `RESTART`, ownership and persistence, and persists atomically.
pub(super) fn alter_sequence(
    cassie: &Cassie,
    statement: &crate::sql::ast::AlterSequenceStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.sequence_exists(&statement.name) {
        return empty_command("ALTER SEQUENCE", controls);
    }
    let output = empty_command("ALTER SEQUENCE", controls)?;
    let ((), updated) = cassie
        .midge
        .update_sequence(&statement.name, |metadata| {
            let current = metadata.parameters();
            let parameters = crate::catalog::SequenceParameters::resolve(
                Some(&current),
                &statement.options,
                &current.data_type,
            )?;
            let mut cursor = metadata.cursor();
            if let Some(restart) = statement.options.restart {
                cursor.restart(&parameters, restart)?;
            }
            metadata.set_parameters(parameters);
            metadata.set_cursor(cursor);
            if let Some(owned_by) = statement.owned_by.as_ref() {
                metadata.owned_by = sequence_owner(Some(owned_by)).map(|mut owner| {
                    owner.table = cassie.midge.canonical_collection_name(&owner.table);
                    owner
                });
            }
            if let Some(persistence) = statement.persistence {
                metadata.persistence = persistence;
            }
            Ok(())
        })
        .map_err(QueryError::from)?;
    cassie
        .catalog
        .register_sequence(canonicalize_sequence_metadata(cassie, updated));
    Ok(output)
}

/// An owned column sequence: identity options apply over the column type.
fn identity_sequence(
    name: &str,
    data_type: &crate::types::DataType,
    constraint: &crate::catalog::FieldConstraint,
) -> Result<crate::catalog::SequenceMeta, QueryError> {
    let options = constraint.identity_options.clone().unwrap_or_default();
    let parameters = crate::catalog::SequenceParameters::resolve(None, &options, data_type)
        .map_err(|error| QueryError::Cassie(error.into()))?;
    Ok(crate::catalog::SequenceMeta::with_parameters(
        name, parameters,
    ))
}

fn sequence_owner(
    owned_by: Option<&crate::sql::ast::SequenceOwnedBy>,
) -> Option<crate::catalog::SequenceOwner> {
    match owned_by {
        Some(crate::sql::ast::SequenceOwnedBy::Column { table, column }) => {
            Some(crate::catalog::SequenceOwner {
                table: table.clone(),
                column: column.clone(),
            })
        }
        Some(crate::sql::ast::SequenceOwnedBy::None) | None => None,
    }
}

pub(super) fn drop_sequence(
    cassie: &Cassie,
    statement: &crate::sql::ast::DropSequenceStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let names = std::iter::once(&statement.name)
        .chain(&statement.additional_names)
        .filter(|name| !statement.if_exists || cassie.catalog.sequence_exists(name))
        .collect::<Vec<_>>();
    if let Some(missing) = names
        .iter()
        .find(|name| !cassie.catalog.sequence_exists(name))
    {
        return Err(QueryError::General(format!(
            "sequence '{missing}' does not exist"
        )));
    }

    let output = empty_command("DROP SEQUENCE", controls)?;
    for name in names {
        cassie
            .midge
            .delete_sequence(name)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_sequence(name);
    }
    Ok(output)
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
                let mut metadata = identity_sequence(sequence, &field.data_type, constraint)?;
                metadata.owned_by = Some(crate::catalog::SequenceOwner {
                    table: statement.table.to_string(),
                    column: field.name.clone(),
                });
                created.push(metadata);
            } else if !cassie.catalog.sequence_exists(sequence) {
                return Err(QueryError::General(format!(
                    "sequence '{sequence}' does not exist"
                )));
            }
        }
    }
    Ok(created)
}

pub(super) fn prepare_add_column_sequences(
    cassie: &Cassie,
    data_type: &crate::types::DataType,
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<Vec<crate::catalog::SequenceMeta>, QueryError> {
    let mut created = Vec::new();
    for constraint in constraints {
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
                data_type.clone(),
            ));
        } else if !cassie.catalog.sequence_exists(sequence) {
            return Err(QueryError::General(format!(
                "sequence '{sequence}' does not exist"
            )));
        }
    }
    Ok(created)
}

pub(super) fn persist_created_sequences(
    cassie: &Cassie,
    sequences: Vec<crate::catalog::SequenceMeta>,
) -> Result<(), QueryError> {
    for mut sequence in sequences {
        sequence.object_id = cassie
            .midge
            .allocate_object_id()
            .map_err(QueryError::from)?;
        let sequence = canonicalize_sequence_metadata(cassie, sequence);
        cassie
            .midge
            .put_sequence(&sequence)
            .map_err(QueryError::from)?;
        cassie.catalog.register_sequence(sequence);
    }
    Ok(())
}

pub(super) fn canonicalize_sequence_metadata(
    cassie: &Cassie,
    mut sequence: crate::catalog::SequenceMeta,
) -> crate::catalog::SequenceMeta {
    sequence.name = cassie.midge.canonical_collection_name(&sequence.name);
    if let Some(owner) = sequence.owned_by.as_mut() {
        owner.table = cassie.midge.canonical_collection_name(&owner.table);
    }
    sequence
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

pub(super) fn mutate_field_constraint(
    cassie: &Cassie,
    table: &str,
    field: &str,
    mutate: impl FnOnce(&mut crate::catalog::FieldConstraint),
) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let position = constraints.iter().position(|constraint| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    });
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
        || constraint.identity.is_some()
}
