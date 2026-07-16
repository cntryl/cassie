use super::{
    batch, build_dml_result, check_timeout, dml_referential_actions, ensure_query_memory_budget,
    filter, inserted_row_to_batch_row, row_id_from_batch_row, scan, update_assignment_to_json,
    BatchRow, Cassie, CassieSession, CollectionSchema, DmlResultContext, Expr, FunctionMeta,
    HashMap, QueryError, QueryExecutionControls, QueryResult, Value,
};
use std::collections::BTreeSet;

struct PreparedUpdateRow {
    row_id: String,
    before_payload: serde_json::Value,
    payload: serde_json::Value,
}

pub(in crate::executor::execution) fn execute_update(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let matched_rows = matched_dml_rows(
        cassie,
        session,
        &statement.table,
        statement.filter.as_ref(),
        params,
        user_functions,
        controls,
    )?;
    let prepared_rows = prepare_update_rows(
        cassie,
        session,
        statement,
        params,
        user_functions,
        &schema,
        &matched_rows,
    )?;
    if let Some(session) = session.filter(|session| session.is_transaction_active()) {
        let mut collections = BTreeSet::from([statement.table.clone()]);
        for prepared in &prepared_rows {
            dml_referential_actions::preflight_update_actions(
                cassie,
                session,
                &statement.table,
                &prepared.before_payload,
                &prepared.payload,
                &mut collections,
            )?;
        }
        let collections = collections.into_iter().collect::<Vec<_>>();
        session
            .preflight_transaction_collections(&collections)
            .map_err(QueryError::from)?;
    }
    let mut returning_rows = Vec::new();
    apply_update_rows(
        cassie,
        session,
        statement,
        &schema,
        prepared_rows,
        &mut returning_rows,
    )?;
    let updated_count = if statement.returning.is_empty() {
        matched_rows.len()
    } else {
        returning_rows.len()
    };
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "UPDATE",
        },
        updated_count,
        returning_rows,
    )
}

fn matched_dml_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    filter_expr: Option<&Expr>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let batches = scan::scan(cassie, session, table)?;
    ensure_query_memory_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    if let Some(filter_expr) = filter_expr {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)
    } else {
        Ok(rows)
    }
}

fn prepare_update_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    schema: &CollectionSchema,
    matched_rows: &[BatchRow],
) -> Result<Vec<PreparedUpdateRow>, QueryError> {
    let mut prepared_rows = Vec::with_capacity(matched_rows.len());
    for row in matched_rows {
        prepared_rows.push(prepare_update_row(
            cassie,
            session,
            statement,
            params,
            user_functions,
            schema,
            row,
        )?);
    }
    Ok(prepared_rows)
}

fn prepare_update_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    schema: &CollectionSchema,
    row: &BatchRow,
) -> Result<PreparedUpdateRow, QueryError> {
    let row_id = row_id_from_batch_row(row)?;
    let current = cassie
        .get_document_for_session(session, &statement.table, &row_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "row '{row_id}' was not found in '{}'",
                statement.table
            ))
        })?;
    let payload = updated_payload_from_row(
        row,
        &statement.assignments,
        params,
        user_functions,
        session,
        schema,
        &current.payload,
    )?;
    let payload = cassie
        .prepare_document_write_for_session(session, &statement.table, payload, true, Some(&row_id))
        .map_err(QueryError::from)?;
    dml_referential_actions::assert_referenced_values_can_change(
        cassie,
        session,
        &statement.table,
        &current.payload,
        &payload,
    )?;
    Ok(PreparedUpdateRow {
        row_id,
        before_payload: current.payload,
        payload,
    })
}

fn updated_payload_from_row(
    row: &BatchRow,
    assignments: &[(String, Expr)],
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    schema: &CollectionSchema,
    current_payload: &serde_json::Value,
) -> Result<serde_json::Value, QueryError> {
    let mut payload = current_payload
        .as_object()
        .cloned()
        .ok_or_else(|| QueryError::General("stored row payload must be object".to_string()))?;
    for (field, expr) in assignments {
        let value =
            filter::evaluate_expr_value(row, expr, params, None, user_functions, session, None)?;
        payload.insert(
            field.clone(),
            update_assignment_to_json(field, &value, schema),
        );
    }
    Ok(serde_json::Value::Object(payload))
}

fn apply_update_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::UpdateStatement,
    schema: &CollectionSchema,
    prepared_rows: Vec<PreparedUpdateRow>,
    returning_rows: &mut Vec<BatchRow>,
) -> Result<(), QueryError> {
    for prepared in prepared_rows {
        let before_payload = prepared.before_payload.clone();
        let row_id = prepared.row_id.clone();
        let document = write_updated_row(cassie, session, &statement.table, prepared)?;
        dml_referential_actions::apply_referenced_update_actions(
            cassie,
            session,
            &statement.table,
            &before_payload,
            &document.payload,
        )?;
        if !statement.returning.is_empty() {
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                schema,
                &document.payload,
            ));
        }
    }
    Ok(())
}

fn write_updated_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    prepared: PreparedUpdateRow,
) -> Result<crate::midge::adapter::DocumentRef, QueryError> {
    cassie
        .put_prepared_document_for_session(
            session,
            table,
            prepared.row_id.clone(),
            prepared.payload,
        )
        .map_err(QueryError::from)?;
    cassie
        .get_document_for_session(session, table, &prepared.row_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!(
                "updated row '{}' was not found in '{}'",
                prepared.row_id, table
            ))
        })
}
