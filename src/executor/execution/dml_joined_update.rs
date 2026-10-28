//! `UPDATE ... FROM` and `DELETE ... USING`, plus the row update every joined
//! mutation (including `MERGE ... THEN UPDATE`) applies.

use super::dml_joined::{
    combined_row, joined_returning_schema, match_rows, payload_row, row_image_entries, source_rows,
    target_rows, ExecutionScope, JoinedTarget, SourceRows,
};
use super::dml_update::{updated_payload_from_row, AssignmentTarget};
use super::{
    build_dml_result, check_timeout, dml_referential_actions, row_id_from_batch_row,
    update_assignment_to_json, BatchRow, Cassie, CassieSession, DmlResultContext, Expr,
    FunctionMeta, HashMap, QueryError, QueryExecutionControls, QueryResult, Value,
};
use crate::sql::ast::{DeleteStatement, RowSubqueryAssignment, UpdateStatement};

/// Whether an `UPDATE` executes through the joined path.
#[must_use]
pub(crate) fn update_is_joined(statement: &UpdateStatement) -> bool {
    statement.from.is_some()
        || !statement.row_subqueries.is_empty()
        || statement.returning_aliases.is_some()
}

/// Whether a `DELETE` executes through the joined path.
#[must_use]
pub(crate) fn delete_is_joined(statement: &DeleteStatement) -> bool {
    statement.using.is_some() || statement.returning_aliases.is_some()
}

/// One target row's stored state before and after an update.
pub(super) struct UpdatedRow {
    pub(super) before: serde_json::Value,
    pub(super) after: serde_json::Value,
}

/// Applies `assignments` and `row_subqueries`, evaluated against `row`, to the
/// stored target row `row_id`; `None` when the row is no longer visible.
pub(super) fn update_target_row(
    scope: &ExecutionScope<'_>,
    target: &JoinedTarget<'_>,
    row_id: &str,
    row: &BatchRow,
    assignments: &[(String, Expr)],
    array_targets: &[crate::sql::ast::UpdateTarget],
    row_subqueries: &[RowSubqueryAssignment],
) -> Result<Option<UpdatedRow>, QueryError> {
    check_timeout(scope.controls)?;
    let Some(current) = scope
        .cassie
        .get_document_for_session(scope.session, target.table, row_id)
        .map_err(QueryError::from)?
    else {
        return Ok(None);
    };
    let mut payload = updated_payload_from_row(
        &AssignmentTarget {
            cassie: scope.cassie,
            table: target.table,
            schema: target.schema,
            subqueries: None,
            array_targets,
        },
        row,
        assignments,
        scope.params,
        scope.user_functions,
        scope.session,
        &current.payload,
    )?;
    for assignment in row_subqueries {
        let values = super::dml_row_subquery::evaluate_row_subquery(scope, assignment, row)?;
        let object = payload
            .as_object_mut()
            .ok_or_else(|| QueryError::General("stored row payload must be object".to_string()))?;
        for (column, value) in assignment.columns.iter().zip(values) {
            object.insert(
                column.clone(),
                update_assignment_to_json(column, &value, target.schema)?,
            );
        }
    }
    let assigned = assignments
        .iter()
        .map(|(column, _)| column.as_str())
        .chain(
            row_subqueries
                .iter()
                .flat_map(|assignment| assignment.columns.iter().map(String::as_str)),
        )
        .collect::<Vec<_>>();
    scope
        .cassie
        .discard_stale_vector_embeddings(target.table, &mut payload, assigned);
    let prepared = scope
        .cassie
        .prepare_document_write_for_session(
            scope.session,
            target.table,
            payload,
            false,
            Some(row_id),
        )
        .map_err(QueryError::from)?;
    let document = dml_referential_actions::update_existing_row(
        scope.cassie,
        scope.session,
        target.table,
        row_id,
        &current.payload,
        prepared,
        scope.controls,
    )?;
    Ok(Some(UpdatedRow {
        before: current.payload,
        after: document.payload,
    }))
}

/// The first source row that matched each target row, in target order.
fn first_matches(matches: &[Vec<usize>], target_count: usize) -> Vec<Option<usize>> {
    let mut chosen = vec![None; target_count];
    for (source_index, targets) in matches.iter().enumerate() {
        for target_index in targets {
            chosen[*target_index].get_or_insert(source_index);
        }
    }
    chosen
}

fn joined_source(
    scope: &ExecutionScope<'_>,
    source: Option<&crate::sql::ast::MutationSource>,
) -> Result<SourceRows, QueryError> {
    source.map_or_else(
        || Ok(SourceRows::single_empty_row()),
        |source| source_rows(scope, source),
    )
}

pub(super) fn execute_joined_update(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &UpdateStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let qualifier = statement
        .alias
        .clone()
        .unwrap_or_else(|| crate::catalog::local_name(&statement.table));
    let target = JoinedTarget {
        table: &statement.table,
        qualifier: &qualifier,
        schema: &schema,
    };
    let scope = ExecutionScope {
        cassie,
        session,
        params,
        user_functions,
        controls,
    };
    let targets = target_rows(&scope, &statement.table)?;
    let source = joined_source(&scope, statement.from.as_ref())?;
    let matches = match_rows(
        &scope,
        &target,
        &targets,
        &source,
        statement.filter.as_ref(),
    )?;
    let mut updated = 0usize;
    let mut returning_rows = Vec::new();
    for (target_index, source_index) in first_matches(&matches, targets.len())
        .into_iter()
        .enumerate()
    {
        let Some(source_index) = source_index else {
            continue;
        };
        let source_row = source.rows[source_index].as_slice();
        let row = combined_row(
            &target,
            Some(&targets[target_index]),
            &source,
            Some(source_row),
            Vec::new(),
        );
        let row_id = row_id_from_batch_row(&targets[target_index])?;
        let Some(change) = update_target_row(
            &scope,
            &target,
            &row_id,
            &row,
            &statement.assignments,
            &statement.array_targets,
            &statement.row_subqueries,
        )?
        else {
            continue;
        };
        updated += 1;
        if !statement.returning.is_empty() {
            let images = row_image_entries(
                &schema,
                statement.returning_aliases.as_ref(),
                Some(&change.before),
                Some(&change.after),
            );
            returning_rows.push(combined_row(
                &target,
                Some(&payload_row(&row_id, &schema, &change.after)),
                &source,
                Some(source_row),
                images,
            ));
        }
    }
    let returning_schema = joined_returning_schema(
        &schema,
        &qualifier,
        &source.fields,
        statement.returning_aliases.as_ref(),
        false,
    );
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "UPDATE",
            controls,
            statement_read_controls: controls,
            returning_schema: Some(&returning_schema),
        },
        updated,
        returning_rows,
    )
}

pub(super) fn execute_joined_delete(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &DeleteStatement,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let schema = cassie.catalog.get_schema(&statement.table).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", statement.table))
    })?;
    let qualifier = statement
        .alias
        .clone()
        .unwrap_or_else(|| crate::catalog::local_name(&statement.table));
    let target = JoinedTarget {
        table: &statement.table,
        qualifier: &qualifier,
        schema: &schema,
    };
    let scope = ExecutionScope {
        cassie,
        session,
        params,
        user_functions,
        controls,
    };
    let targets = target_rows(&scope, &statement.table)?;
    let source = joined_source(&scope, statement.using.as_ref())?;
    let matches = match_rows(
        &scope,
        &target,
        &targets,
        &source,
        statement.filter.as_ref(),
    )?;
    let mut deleted = 0usize;
    let mut returning_rows = Vec::new();
    for (target_index, source_index) in first_matches(&matches, targets.len())
        .into_iter()
        .enumerate()
    {
        let Some(source_index) = source_index else {
            continue;
        };
        let row_id = row_id_from_batch_row(&targets[target_index])?;
        let Some(before) = delete_target_row(&scope, &target, &row_id)? else {
            continue;
        };
        deleted += 1;
        if !statement.returning.is_empty() {
            let images = row_image_entries(
                &schema,
                statement.returning_aliases.as_ref(),
                Some(&before),
                None,
            );
            returning_rows.push(combined_row(
                &target,
                Some(&payload_row(&row_id, &schema, &before)),
                &source,
                Some(source.rows[source_index].as_slice()),
                images,
            ));
        }
    }
    let returning_schema = joined_returning_schema(
        &schema,
        &qualifier,
        &source.fields,
        statement.returning_aliases.as_ref(),
        false,
    );
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "DELETE",
            controls,
            statement_read_controls: controls,
            returning_schema: Some(&returning_schema),
        },
        deleted,
        returning_rows,
    )
}

/// Deletes the stored target row `row_id` with its referential actions and
/// returns its prior payload; `None` when the row is no longer visible.
pub(super) fn delete_target_row(
    scope: &ExecutionScope<'_>,
    target: &JoinedTarget<'_>,
    row_id: &str,
) -> Result<Option<serde_json::Value>, QueryError> {
    check_timeout(scope.controls)?;
    let Some(current) = scope
        .cassie
        .get_document_for_session(scope.session, target.table, row_id)
        .map_err(QueryError::from)?
    else {
        return Ok(None);
    };
    match dml_referential_actions::delete_existing_row(
        scope.cassie,
        scope.session,
        target.table,
        row_id,
        &current.payload,
        scope.controls,
    )? {
        dml_referential_actions::DeleteOutcome::Deleted(true) => Ok(Some(current.payload)),
        dml_referential_actions::DeleteOutcome::Deleted(false) => Ok(None),
        dml_referential_actions::DeleteOutcome::Restricted(error) => Err(error),
    }
}
