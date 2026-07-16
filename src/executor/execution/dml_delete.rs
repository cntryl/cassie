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

    let batches = scan::scan(cassie, session, &statement.table)?;
    ensure_query_memory_budget(controls, &batches)?;
    let rows = batch::flatten_batches(batches);
    let matched_rows = if let Some(filter_expr) = &statement.filter {
        filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?
    } else {
        rows
    };

    let mut delete_ids = Vec::with_capacity(matched_rows.len());
    let mut returning_rows = Vec::new();
    for row in &matched_rows {
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
        dml_referential_actions::preflight_delete_actions(
            cassie,
            session,
            &statement.table,
            &current.payload,
        )?;
        dml_referential_actions::assert_no_referencing_rows(
            cassie,
            session,
            &statement.table,
            &current.payload,
        )?;
        if !statement.returning.is_empty() {
            returning_rows.push(inserted_row_to_batch_row(
                &row_id,
                &schema,
                &current.payload,
            ));
        }
        delete_ids.push(row_id);
    }

    for row_id in &delete_ids {
        cassie
            .delete_document_for_session(session, &statement.table, row_id)
            .map_err(QueryError::from)?;
    }

    let deleted_count = delete_ids.len();
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
