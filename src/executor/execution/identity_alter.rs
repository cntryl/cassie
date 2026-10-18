//! `ALTER TABLE ... ALTER COLUMN` identity changes.
use super::{Cassie, QueryError};
use crate::catalog::{
    DefaultSequenceOwnership, SequenceError, SequenceMeta, SequenceOptions, SequenceOwner,
    SequenceParameters,
};
use crate::sql::ast::IdentityAlteration;

fn object_state(message: String) -> QueryError {
    QueryError::Cassie(
        SequenceError {
            sqlstate: "55000",
            message,
        }
        .into(),
    )
}

pub(super) fn alter_column_identity(
    cassie: &Cassie,
    table: &str,
    field: &str,
    action: &IdentityAlteration,
) -> Result<(), QueryError> {
    let constraint = cassie
        .catalog
        .get_constraints(table)
        .into_iter()
        .find(|constraint| constraint.field.eq_ignore_ascii_case(field));
    match action {
        IdentityAlteration::Add {
            generation,
            options,
        } => add_identity(
            cassie,
            table,
            field,
            constraint.as_ref(),
            *generation,
            options,
        ),
        IdentityAlteration::Set {
            generation,
            options,
        } => set_identity(cassie, table, field, constraint, *generation, options),
        IdentityAlteration::Drop { if_exists } => {
            drop_identity(cassie, table, field, constraint, *if_exists)
        }
    }
}

fn add_identity(
    cassie: &Cassie,
    table: &str,
    field: &str,
    constraint: Option<&crate::catalog::FieldConstraint>,
    generation: crate::catalog::IdentityGeneration,
    options: &SequenceOptions,
) -> Result<(), QueryError> {
    let relation = crate::catalog::local_name(table);
    if constraint
        .and_then(|constraint| constraint.identity)
        .is_some()
    {
        return Err(object_state(format!(
            "column \"{field}\" of relation \"{relation}\" is already an identity column"
        )));
    }
    if !constraint.is_some_and(|constraint| constraint.not_null) {
        return Err(object_state(format!(
            "column \"{field}\" of relation \"{relation}\" must be declared NOT NULL before identity can be added"
        )));
    }
    let data_type = cassie
        .catalog
        .get_schema(table)
        .and_then(|schema| {
            schema
                .fields
                .into_iter()
                .find(|meta| meta.name.eq_ignore_ascii_case(field))
        })
        .map_or(crate::types::DataType::BigInt, |meta| meta.data_type);
    let sequence = create_identity_sequence(cassie, table, field, &data_type, options)?;
    super::sequence_command::mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.identity = Some(generation);
        constraint.default_value = None;
        constraint.default_expression =
            Some(crate::catalog::canonical_nextval_expression(&sequence));
        constraint.default_sequence = Some(sequence);
        constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
    })
}

fn set_identity(
    cassie: &Cassie,
    table: &str,
    field: &str,
    constraint: Option<crate::catalog::FieldConstraint>,
    generation: Option<crate::catalog::IdentityGeneration>,
    options: &SequenceOptions,
) -> Result<(), QueryError> {
    let relation = crate::catalog::local_name(table);
    let Some(constraint) = constraint.filter(|constraint| constraint.identity.is_some()) else {
        return Err(object_state(format!(
            "column \"{field}\" of relation \"{relation}\" is not an identity column"
        )));
    };
    if *options != SequenceOptions::default() {
        let sequence = constraint
            .default_sequence
            .clone()
            .ok_or_else(|| object_state(format!("identity column \"{field}\" has no sequence")))?;
        let ((), updated) = cassie
            .midge
            .update_sequence(&sequence, |metadata| {
                let current = metadata.parameters();
                let parameters =
                    SequenceParameters::resolve(Some(&current), options, &current.data_type)?;
                let mut cursor = metadata.cursor();
                if let Some(restart) = options.restart {
                    cursor.restart(&parameters, restart)?;
                }
                metadata.set_parameters(parameters);
                metadata.set_cursor(cursor);
                Ok(())
            })
            .map_err(QueryError::from)?;
        cassie.catalog.register_sequence(updated);
    }
    if let Some(generation) = generation {
        super::sequence_command::mutate_field_constraint(cassie, table, field, |constraint| {
            constraint.identity = Some(generation);
        })?;
    }
    Ok(())
}

fn drop_identity(
    cassie: &Cassie,
    table: &str,
    field: &str,
    constraint: Option<crate::catalog::FieldConstraint>,
    if_exists: bool,
) -> Result<(), QueryError> {
    let relation = crate::catalog::local_name(table);
    let Some(constraint) = constraint.filter(|constraint| constraint.identity.is_some()) else {
        if if_exists {
            return Ok(());
        }
        return Err(object_state(format!(
            "column \"{field}\" of relation \"{relation}\" is not an identity column"
        )));
    };
    if let Some(sequence) = constraint.default_sequence.as_deref() {
        cassie
            .midge
            .delete_sequence(sequence)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_sequence(sequence);
    }
    super::sequence_command::mutate_field_constraint(cassie, table, field, |constraint| {
        constraint.identity = None;
        constraint.identity_options = None;
        constraint.default_expression = None;
        constraint.default_sequence = None;
        constraint.default_sequence_owned = DefaultSequenceOwnership::shared();
    })
}

fn create_identity_sequence(
    cassie: &Cassie,
    table: &str,
    field: &str,
    data_type: &crate::types::DataType,
    options: &SequenceOptions,
) -> Result<String, QueryError> {
    let name = crate::catalog::serial_sequence_name(table, field);
    let parameters = SequenceParameters::resolve(None, options, data_type)
        .map_err(|error| QueryError::Cassie(error.into()))?;
    let mut metadata = SequenceMeta::with_parameters(&name, parameters);
    metadata.owned_by = Some(SequenceOwner {
        table: table.to_string(),
        column: field.to_string(),
    });
    metadata.object_id = cassie
        .midge
        .allocate_object_id()
        .map_err(QueryError::from)?;
    cassie
        .midge
        .put_sequence(&metadata)
        .map_err(QueryError::from)?;
    cassie.catalog.register_sequence(metadata);
    Ok(name)
}
