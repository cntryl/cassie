use super::{
    batch, check_timeout, dml_referential_actions, dml_returning_columns,
    ensure_query_memory_budget, filter, inserted_row_to_batch_row, projection,
    row_id_from_batch_row, scan, BatchRow, Cassie, CassieSession, FunctionMeta, HashMap,
    QueryError, QueryExecutionControls, QueryResult, Value,
};

pub(in crate::executor::execution) fn execute_delete(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let collections = cassie.referential_write_collections(&statement.table);
    cassie.midge.with_collection_write_gates(&collections, || {
        execute_delete_with_held_referential_gates(
            cassie,
            session,
            statement,
            params,
            user_functions,
            controls,
        )
    })
}

fn execute_delete_with_held_referential_gates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;

    let batches = scan::scan(cassie, session, &statement.table, controls)?;
    ensure_query_memory_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut deleted_count = 0usize;
    let mut returning_rows = Vec::new();
    for row in &matched_rows {
        check_timeout(controls)?;
        let row_id = row_id_from_batch_row(row)?;
        let current = cassie
            .get_document_for_session(session, &statement.table, &row_id)
            .map_err(QueryError::from)?;
        let Some(current) = current else {
            if session.is_some_and(|session| {
                matches!(
                    session.document_change(&statement.table, &row_id),
                    Some(crate::app::TransactionRowChange::Delete)
                )
            }) {
                continue;
            }
            return Err(QueryError::General(format!(
                "row '{row_id}' was not found in '{}'",
                statement.table
            )));
        };
        dml_referential_actions::preflight_delete_actions(
            cassie,
            session,
            &statement.table,
            &current.payload,
            controls,
        )?;
        dml_referential_actions::assert_no_referencing_rows(
            cassie,
            session,
            &statement.table,
            &row_id,
            &current.payload,
            controls,
        )?;
        if !statement.returning.is_empty() {
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &current.payload,
            ));
        }
        check_timeout(controls)?;
        cassie
            .delete_document_for_session(session, &statement.table, &row_id)
            .map_err(QueryError::from)?;
        deleted_count += 1;
    }

    if statement.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("DELETE {deleted_count}"),
        });
    }

    let projected = projection::project_rows(
        returning_rows,
        &statement.returning,
        params,
        None,
        user_functions,
        session,
    )?;

    let column_schema = cassie.catalog.get_schema(&statement.table);
    let columns =
        dml_returning_columns(&statement.returning, column_schema.as_ref(), user_functions);

    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("DELETE {deleted_count}"),
    })
}
