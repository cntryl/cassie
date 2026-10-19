//! `ON CONFLICT DO UPDATE`: the conflicting row's update.

use super::conflict::{excluded_local_args, find_insert_conflict_row_id};
use super::{
    dml_referential_actions, filter, inserted_row_to_batch_row, update_assignment_to_json,
    BatchRow, CassieSession, CollectionSchema, Expr, FunctionMeta, HashMap, InsertExecutionContext,
    QueryError, Value,
};

/// Like PostgreSQL, one INSERT ... ON CONFLICT DO UPDATE may not affect the
/// same row twice, whether it inserted that row earlier in the statement or
/// already updated it.
pub(super) fn reject_second_conflict_update(
    conflict_id: &str,
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<(), QueryError> {
    if affected_row_ids.contains(conflict_id) {
        return Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_string(),
            ),
        ));
    }
    Ok(())
}

pub(super) fn resolve_autocommit_insert_conflict(
    context: &InsertExecutionContext<'_>,
    payload: &serde_json::Value,
    original_error: crate::app::CassieError,
    affected_row_ids: &std::collections::HashSet<String>,
) -> Result<Option<String>, QueryError> {
    let on_conflict = context
        .statement
        .on_conflict
        .as_ref()
        .expect("conflict clause checked by caller");
    let Some(conflict_id) = find_insert_conflict_row_id(
        context.cassie,
        context.session,
        context.statement,
        payload,
        context.schema,
    )?
    else {
        // Untargeted DO NOTHING skips a conflict on any unique key, including
        // expression indexes whose conflicting row cannot be looked up here.
        if on_conflict.target_fields.is_empty()
            && matches!(
                on_conflict.action,
                crate::sql::ast::InsertConflictAction::DoNothing
            )
        {
            return Ok(None);
        }
        // The violation is on a key other than the ON CONFLICT target, so it
        // stands, and keeps its 23505 SQLSTATE.
        return Err(QueryError::from(original_error));
    };
    match &on_conflict.action {
        crate::sql::ast::InsertConflictAction::DoNothing => Ok(None),
        crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
            ..
        } => {
            reject_second_conflict_update(&conflict_id, affected_row_ids)?;
            execute_insert_conflict_update(
                context,
                payload,
                &conflict_id,
                assignments,
                filter.as_ref(),
            )
        }
    }
}

struct ConflictAssignmentContext<'a> {
    existing_row: &'a BatchRow,
    excluded_args: &'a HashMap<String, Value>,
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
    schema: &'a CollectionSchema,
    cassie: &'a crate::app::Cassie,
    table: &'a str,
}

/// Whether the `DO UPDATE ... WHERE` condition holds for the conflicting row.
fn conflict_filter_holds(
    context: &InsertExecutionContext<'_>,
    conflict_filter: Option<&Expr>,
    existing_row: &BatchRow,
    excluded_args: &HashMap<String, Value>,
) -> Result<bool, QueryError> {
    let Some(filter_expr) = conflict_filter else {
        return Ok(true);
    };
    let filter_expr = crate::executor::execution::resolve_statement_exists(
        context.cassie,
        context.session,
        filter_expr,
        context.user_functions,
        context.params,
        context.controls,
    )?;
    filter::eval_scalar(
        existing_row,
        &filter_expr,
        context.params,
        None,
        context.user_functions,
        Some(excluded_args),
        context.session,
    )?
    .is_true()
}

/// Assigns each `SET (...) = (SELECT ...)` result into `payload`.
fn apply_row_subqueries(
    context: &InsertExecutionContext<'_>,
    row_subqueries: &[crate::sql::ast::RowSubqueryAssignment],
    outer: &BatchRow,
    payload: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), QueryError> {
    if row_subqueries.is_empty() {
        return Ok(());
    }
    let scope = super::super::dml_joined::ExecutionScope {
        cassie: context.cassie,
        session: context.session,
        params: context.params,
        user_functions: context.user_functions,
        controls: context.controls,
    };
    for assignment in row_subqueries {
        let values =
            super::super::dml_row_subquery::evaluate_row_subquery(&scope, assignment, outer)?;
        for (column, value) in assignment.columns.iter().zip(values) {
            payload.insert(
                column.clone(),
                update_assignment_to_json(column, &value, context.schema)?,
            );
        }
    }
    Ok(())
}

/// The existing row with `EXCLUDED` and the target alias as qualifiers, for
/// outer references of a `SET (...) = (SELECT ...)` subquery.
fn subquery_outer_row(
    existing_row: &BatchRow,
    excluded_args: &HashMap<String, Value>,
    statement: &crate::sql::ast::InsertStatement,
) -> BatchRow {
    let (mut values, mut aliases) = existing_row.clone().into_parts();
    if let Some(alias) = statement.alias.as_deref() {
        for (index, (field, _)) in values.iter().enumerate() {
            aliases.push((format!("{alias}.{field}"), index));
        }
    }
    values.extend(
        excluded_args
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    BatchRow::with_aliases(values, aliases)
}

pub(super) fn execute_insert_conflict_update(
    context: &InsertExecutionContext<'_>,
    payload: &serde_json::Value,
    conflict_id: &str,
    assignments: &[(String, Expr)],
    conflict_filter: Option<&Expr>,
) -> Result<Option<String>, QueryError> {
    let current = context
        .cassie
        .get_document_for_session(context.session, &context.statement.table, conflict_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "conflicting row '{conflict_id}' was not found in '{}'",
                context.statement.table
            ))
        })?;
    let existing_row = conflict_existing_row(
        conflict_id,
        context.schema,
        &current.payload,
        &context.statement.table,
    );
    let _type_memory = context
        .controls
        .reserve_query_memory(crate::executor::batch::row_type_bytes([&existing_row]))?;
    let proposed = context
        .cassie
        .payload_with_column_defaults(&context.statement.table, payload)
        .map_err(QueryError::from)?;
    let excluded_args = excluded_local_args(&proposed, context.schema);
    if !conflict_filter_holds(context, conflict_filter, &existing_row, &excluded_args)? {
        return Ok(None);
    }
    let assignment_context = ConflictAssignmentContext {
        existing_row: &existing_row,
        excluded_args: &excluded_args,
        params: context.params,
        user_functions: context.user_functions,
        session: context.session,
        schema: context.schema,
        cassie: context.cassie,
        table: &context.statement.table,
    };
    let mut merged_payload =
        merged_conflict_payload(&current.payload, assignments, &assignment_context)?;
    let row_subqueries = context
        .statement
        .on_conflict
        .as_ref()
        .map_or(&[][..], |clause| clause.update_row_subqueries.as_slice());
    apply_row_subqueries(
        context,
        row_subqueries,
        &subquery_outer_row(&existing_row, &excluded_args, context.statement),
        &mut merged_payload,
    )?;
    let mut merged_payload = serde_json::Value::Object(merged_payload);
    context.cassie.discard_stale_vector_embeddings(
        &context.statement.table,
        &mut merged_payload,
        assignments.iter().map(|(field, _)| field.as_str()).chain(
            row_subqueries
                .iter()
                .flat_map(|assignment| assignment.columns.iter().map(String::as_str)),
        ),
    );
    let prepared = context
        .cassie
        .prepare_document_write_for_session(
            context.session,
            &context.statement.table,
            merged_payload,
            false,
            Some(conflict_id),
        )
        .map_err(QueryError::from)?;
    dml_referential_actions::update_existing_row(
        context.cassie,
        context.session,
        &context.statement.table,
        conflict_id,
        &current.payload,
        prepared,
        context.controls,
    )?;
    if let Some(images) = context.conflict_images {
        images
            .borrow_mut()
            .insert(conflict_id.to_string(), current.payload);
    }
    Ok(Some(conflict_id.to_string()))
}

fn conflict_existing_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
    table: &str,
) -> BatchRow {
    let mut row = inserted_row_to_batch_row(row_id, schema, payload);
    crate::executor::scan::attach_row_types(&mut row, Some(schema));
    let types = row.shared_data_types();
    let (values, mut aliases) = row.into_parts();
    let local_table = crate::catalog::local_name(table);
    for (index, (field, _)) in values.iter().enumerate() {
        aliases.push((format!("{table}.{field}"), index));
        aliases.push((format!("{local_table}.{field}"), index));
    }
    BatchRow::with_aliases(values, aliases).with_optional_data_types(types)
}

fn merged_conflict_payload(
    current_payload: &serde_json::Value,
    assignments: &[(String, Expr)],
    context: &ConflictAssignmentContext<'_>,
) -> Result<serde_json::Map<String, serde_json::Value>, QueryError> {
    let mut merged_payload = current_payload
        .as_object()
        .cloned()
        .ok_or_else(|| QueryError::General("stored row payload must be object".to_string()))?;
    for (field, expr) in assignments {
        if expr.is_default_assignment() {
            match context
                .cassie
                .column_default_json(context.table, field)
                .map_err(QueryError::from)?
            {
                Some(value) => merged_payload.insert(field.clone(), value),
                None => merged_payload.remove(field),
            };
            continue;
        }
        let value = conflict_assignment_value(
            expr,
            context.existing_row,
            context.excluded_args,
            context.params,
            context.user_functions,
            context.session,
        )?;
        merged_payload.insert(
            field.clone(),
            update_assignment_to_json(field, &value, context.schema)?,
        );
    }
    Ok(merged_payload)
}

fn conflict_assignment_value(
    expr: &Expr,
    existing_row: &BatchRow,
    excluded_args: &HashMap<String, Value>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    match expr {
        Expr::Column(name) => Ok(excluded_args
            .get(
                &crate::sql::ColumnIdentifierPath::parse(name)
                    .map_or_else(|_| name.to_ascii_lowercase(), |column| column.lookup_key()),
            )
            .cloned()
            .or_else(|| existing_row.get(name).cloned())
            .unwrap_or(Value::Null)),
        _ => filter::evaluate_expr_value(
            existing_row,
            expr,
            params,
            None,
            user_functions,
            session,
            Some(excluded_args),
        ),
    }
}
