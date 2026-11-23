use super::{catalog, refresh_table_cardinality_stats, Cassie, QueryError};
use crate::app::{CassieError, CassieSession};
use crate::types::{DataType, FieldSchema};

/// The column of an `ALTER TABLE ... ADD COLUMN`.
pub(super) struct NewColumn<'a> {
    pub(super) field: &'a str,
    pub(super) data_type: &'a DataType,
    pub(super) constraints: &'a [crate::catalog::FieldConstraint],
    pub(super) if_not_exists: bool,
}

struct AddColumnContext<'a> {
    session: Option<&'a CassieSession>,
    table: &'a str,
    column: &'a NewColumn<'a>,
    field_schema: FieldSchema,
    existing: bool,
    atomic_storage: bool,
    sequences: &'a [crate::catalog::SequenceMeta],
    merged_constraints: &'a [crate::catalog::FieldConstraint],
    default_value: Option<serde_json::Value>,
}

pub(super) fn alter_table_add_column(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    column: &NewColumn<'_>,
) -> Result<(), QueryError> {
    let NewColumn {
        field,
        data_type,
        constraints,
        if_not_exists,
    } = *column;
    let field_schema = FieldSchema {
        name: field.to_string(),
        data_type: data_type.clone(),
        nullable: true,
    };
    let existing_field = cassie.catalog.get_schema(table).and_then(|schema| {
        schema.fields.into_iter().find(|existing| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&existing.name)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        })
    });
    let existing = existing_field.is_some();
    let pending = if existing {
        cassie
            .midge
            .has_pending_field_add(table, field)
            .map_err(QueryError::from)?
    } else {
        false
    };
    if if_not_exists && existing && !pending {
        return Ok(());
    }

    reject_unverifiable_legacy_default(
        field,
        data_type,
        constraints,
        pending,
        existing_field.as_ref(),
    )?;

    let atomic_storage = session.is_none_or(|session| session.transaction_status() == "idle")
        && !cassie.midge.legacy_field_add_failure_injected();
    let sequences = super::super::sequence_command::prepare_add_column_sequences(
        cassie,
        data_type,
        constraints,
    )?;
    let mut merged_constraints = cassie.catalog.get_constraints(table);
    crate::catalog::merge_constraint_set(&mut merged_constraints, constraints.to_vec());
    // A volatile default is evaluated once and backfilled like PostgreSQL.
    let default_value = constraints
        .iter()
        .find(|constraint| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        })
        .and_then(|constraint| {
            constraint.default_value.clone().or_else(|| {
                constraint
                    .default_expression
                    .as_deref()
                    .and_then(crate::catalog::evaluate_volatile_default)
            })
        });
    let mut gated_collections = vec![table.to_string()];
    gated_collections.extend(
        constraints
            .iter()
            .filter_map(|constraint| constraint.references_table.clone()),
    );
    let context = AddColumnContext {
        session,
        table,
        column,
        field_schema,
        existing,
        atomic_storage,
        sequences: &sequences,
        merged_constraints: &merged_constraints,
        default_value,
    };
    cassie
        .midge
        .with_collection_write_gates(&gated_collections, || {
            apply_add_column_under_gates(cassie, &context)
        })?;
    Ok(())
}

fn reject_unverifiable_legacy_default(
    field: &str,
    data_type: &DataType,
    constraints: &[crate::catalog::FieldConstraint],
    pending: bool,
    existing_field: Option<&FieldSchema>,
) -> Result<(), QueryError> {
    let unverifiable_legacy_default = constraints.iter().find_map(|constraint| {
        if crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
            != crate::sql::ColumnIdentifierPath::stored_field_key(field)
        {
            return None;
        }
        if constraint.default_sequence.is_some() && !constraint.default_sequence_owned.is_owned() {
            return Some(
                "shared sequence identity cannot be recovered from the legacy record; restart Cassie to recover the recorded field/backfill, then reapply the intended default after recovery",
            );
        }
        constraint
            .default_expression
            .as_deref()
            .and_then(crate::catalog::parse_volatile_default_expression)
            .map(|_| {
                "legacy default expression identity cannot be verified; restart Cassie to recover the recorded default"
            })
    });
    if let Some(reason) = unverifiable_legacy_default.filter(|_| {
        pending && existing_field.is_some_and(|existing| existing.data_type == *data_type)
    }) {
        return Err(CassieError::from(crate::app::DiagnosticError::new(
            "55000",
            format!(
                "column '{field}' is durable with a pending backfill; {}",
                reason
            ),
        ))
        .into());
    }
    Ok(())
}

fn apply_add_column_under_gates(
    cassie: &Cassie,
    context: &AddColumnContext<'_>,
) -> Result<(), QueryError> {
    let NewColumn {
        data_type,
        constraints,
        if_not_exists,
        ..
    } = *context.column;
    let default_value = cassie.domain_add_column_default(
        context.session,
        context.table,
        data_type,
        context.default_value.clone(),
    )?;
    let resumed = if context.existing {
        cassie
            .midge
            .resume_pending_field_add_matching(
                context.table,
                &context.field_schema,
                default_value.as_ref(),
            )
            .map_err(QueryError::from)?
    } else {
        false
    };
    if resumed {
        return finish_resumed_field_add(cassie, context);
    }
    if if_not_exists && context.existing {
        return Ok(());
    }
    let sequence_metadata = if context.atomic_storage {
        super::super::sequence_command::prepare_created_sequences(
            cassie,
            context.sequences.to_vec(),
        )?
    } else {
        context.sequences.to_vec()
    };
    add_column_to_storage(
        cassie,
        context.table,
        context.column,
        default_value,
        context.atomic_storage,
        context.merged_constraints,
        &sequence_metadata,
    )?;

    if context.atomic_storage {
        super::super::sequence_command::register_created_sequences(cassie, sequence_metadata);
        if !constraints.is_empty() {
            cassie
                .catalog
                .register_constraints(context.table, context.merged_constraints.to_vec());
        }
        if let Err(error) = refresh_table_cardinality_stats(cassie, context.table) {
            cassie.catalog.clear_cardinality_stats(context.table);
            tracing::warn!(table = %context.table, %error, "cardinality refresh deferred after atomic ADD COLUMN");
        }
        Ok(())
    } else {
        super::super::sequence_command::persist_created_sequences(
            cassie,
            context.sequences.to_vec(),
        )?;
        if !constraints.is_empty() {
            cassie
                .midge
                .save_constraints_with_unique_reservations(
                    context.table,
                    context.merged_constraints,
                )
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .register_constraints(context.table, context.merged_constraints.to_vec());
        }
        refresh_table_cardinality_stats(cassie, context.table)
    }
}

fn finish_resumed_field_add(
    cassie: &Cassie,
    context: &AddColumnContext<'_>,
) -> Result<(), QueryError> {
    cassie.catalog.unregister_maintenance_debt(
        context.table,
        &catalog::MaintenanceDebtMeta::add_column_backfill_artifact(context.column.field),
    );
    // Legacy pending records predate the atomic metadata bundle and contain no
    // constraint or sequence facts, so complete the original continuation.
    super::super::sequence_command::persist_created_sequences(cassie, context.sequences.to_vec())?;
    if !context.column.constraints.is_empty() {
        cassie
            .midge
            .save_constraints_with_unique_reservations(context.table, context.merged_constraints)
            .map_err(QueryError::from)?;
        cassie
            .catalog
            .register_constraints(context.table, context.merged_constraints.to_vec());
    }
    refresh_table_cardinality_stats(cassie, context.table)
}

/// Adds the column after validating its backfill and constraints.
fn add_column_to_storage(
    cassie: &Cassie,
    table: &str,
    column: &NewColumn<'_>,
    default_value: Option<serde_json::Value>,
    atomic_storage: bool,
    merged_constraints: &[crate::catalog::FieldConstraint],
    sequences: &[crate::catalog::SequenceMeta],
) -> Result<(), QueryError> {
    let NewColumn {
        field,
        data_type,
        constraints,
        ..
    } = *column;
    // Existing rows receive the default, so validate the constraints
    // against that value before anything is committed.
    let backfilled = default_value.clone().unwrap_or(serde_json::Value::Null);
    let added = crate::app::AddedColumnDefault {
        field,
        value: &backfilled,
    };
    cassie.validate_existing_rows_after_added_column(table, constraints, Some(added))?;
    cassie.validate_existing_foreign_key_rows_after_added_column(
        table,
        constraints,
        Some(added),
    )?;

    let field_schema = FieldSchema {
        name: field.to_string(),
        data_type: data_type.clone(),
        nullable: true,
    };
    if atomic_storage {
        cassie
            .midge
            .alter_collection_add_column_atomically(
                table,
                field_schema.clone(),
                default_value,
                merged_constraints,
                sequences,
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
    } else {
        let altered = cassie.midge.alter_collection_add_column_with_default(
            table,
            field_schema.clone(),
            default_value,
        );
        // A transaction-local schema overlay is published by the session later.
        if altered.is_err()
            && cassie.midge.collection_schema(table).is_some_and(|schema| {
                schema
                    .fields
                    .iter()
                    .any(|entry| entry.name == field_schema.name)
            })
        {
            cassie.catalog.add_collection_field(
                table,
                field_schema.name.clone(),
                field_schema.data_type.clone(),
            );
            cassie
                .catalog
                .register_maintenance_debt(catalog::MaintenanceDebtMeta::new(
                    table,
                    catalog::MaintenanceDebtMeta::add_column_backfill_artifact(field),
                    0,
                    0,
                    altered.as_ref().err().map(ToString::to_string),
                ));
        }
        altered.map_err(|error| QueryError::General(error.to_string()))?;
    }
    cassie
        .catalog
        .add_collection_field(table, field_schema.name, field_schema.data_type);
    Ok(())
}
