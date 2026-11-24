//! `MERGE INTO target USING source ON condition WHEN ...`.
//!
//! The target snapshot and the source are joined once. Source rows are
//! processed in order: each matched pair takes the first `WHEN MATCHED` clause
//! whose condition holds, and an unmatched source row the first `WHEN NOT
//! MATCHED [BY TARGET]` clause. Target rows no source row matched then take
//! the first `WHEN NOT MATCHED BY SOURCE` clause. A target row that a second
//! source row would update or delete again raises 21000. All actions run in the
//! statement's mutation batch, so any failure leaves no partial effect.

use std::collections::HashSet;

use super::dml_joined::{
    combined_row, joined_returning_schema, match_rows, payload_row, row_image_entries, source_rows,
    target_rows, ExecutionScope, JoinedTarget, SourceRows,
};
use super::dml_joined_update::{delete_target_row, update_target_row};
use super::{
    build_dml_result, check_timeout, row_id_from_batch_row, BatchRow, Cassie, CassieSession,
    CollectionSchema, DmlResultContext, Expr, FunctionMeta, HashMap, QueryError,
    QueryExecutionControls, QueryResult, Value,
};
use crate::sql::ast::{
    InsertSource, InsertStatement, MergeAction, MergeClause, MergeMatch, MergeStatement,
    MERGE_ACTION_COLUMN,
};

struct MergeRun<'a> {
    scope: ExecutionScope<'a>,
    statement: &'a MergeStatement,
    target: JoinedTarget<'a>,
    source: SourceRows,
    targets: Vec<BatchRow>,
    prepared_row_subqueries: Vec<Vec<super::dml_row_subquery::PreparedRowSubquery>>,
    modified: HashSet<usize>,
    affected: usize,
    returning: Vec<BatchRow>,
}

pub(in crate::executor::execution) fn execute_merge(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &MergeStatement,
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
    // Every read of the statement, including the subqueries in its
    // conditions and actions, sees the data as it was when the statement
    // began; the working batch's own writes stay invisible to them.
    let statement_controls = crate::executor::execution::entrypoints::statement_read_controls(
        cassie, session, controls,
    )?;
    let controls = &statement_controls;
    let scope = ExecutionScope {
        cassie,
        session,
        params,
        user_functions,
        controls,
    };
    let source = source_rows(&scope, &statement.source)?;
    let outer_qualifiers = source
        .fields
        .iter()
        .map(|field| field.qualifier.clone())
        .chain(std::iter::once(qualifier.clone()))
        .map(|qualifier| qualifier.to_ascii_lowercase())
        .chain(std::iter::once(String::new()))
        .collect::<HashSet<_>>();
    let folded_statement = prepare_merge_statement(&scope, statement, &outer_qualifiers)?;
    let statement = &folded_statement;
    let targets = target_rows(&scope, &statement.table)?;
    let prepared_row_subqueries =
        prepare_merge_row_subqueries(&scope, statement, &outer_qualifiers)?;
    let mut run = MergeRun {
        target: JoinedTarget {
            table: &statement.table,
            qualifier: &qualifier,
            schema: &schema,
        },
        scope,
        statement,
        source,
        targets,
        prepared_row_subqueries,
        modified: HashSet::new(),
        affected: 0,
        returning: Vec::new(),
    };
    let matches = match_rows(
        &run.scope,
        &run.target,
        &run.targets,
        &run.source,
        Some(&statement.on),
    )?;
    let mut matched_targets = vec![false; run.targets.len()];
    for (source_index, target_indexes) in matches.iter().enumerate() {
        if target_indexes.is_empty() {
            run.not_matched_by_target(source_index)?;
            continue;
        }
        for target_index in target_indexes {
            matched_targets[*target_index] = true;
            run.matched(*target_index, Some(source_index), MergeMatch::Matched)?;
        }
    }
    if statement
        .clauses
        .iter()
        .any(|clause| clause.kind == MergeMatch::NotMatchedBySource)
    {
        for (target_index, matched) in matched_targets.into_iter().enumerate() {
            if !matched {
                run.matched(target_index, None, MergeMatch::NotMatchedBySource)?;
            }
        }
    }
    let returning_schema = joined_returning_schema(
        &schema,
        &qualifier,
        &run.source.fields,
        statement.returning_aliases.as_ref(),
        true,
    );
    build_dml_result(
        &DmlResultContext {
            cassie,
            session,
            table: &statement.table,
            returning: &statement.returning,
            params,
            user_functions,
            command_prefix: "MERGE",
            controls,
            statement_read_controls: controls,
            returning_schema: Some(&returning_schema),
        },
        run.affected,
        run.returning,
    )
}

fn prepare_merge_statement(
    scope: &ExecutionScope<'_>,
    statement: &MergeStatement,
    outer_qualifiers: &HashSet<String>,
) -> Result<MergeStatement, QueryError> {
    let mut folded = statement.clone();
    folded.on = super::dml_subqueries::resolve_statement_subqueries_with_outer(
        scope,
        &statement.on,
        outer_qualifiers,
    )?;
    for clause in &mut folded.clauses {
        if let Some(condition) = &clause.condition {
            clause.condition = Some(
                super::dml_subqueries::resolve_statement_subqueries_with_outer(
                    scope,
                    condition,
                    outer_qualifiers,
                )?,
            );
        }
        match &mut clause.action {
            MergeAction::Update { assignments, .. } => {
                for (_, expression) in assignments {
                    *expression = super::dml_subqueries::resolve_statement_subqueries_with_outer(
                        scope,
                        expression,
                        outer_qualifiers,
                    )?;
                }
            }
            MergeAction::Insert {
                values: Some(values),
                ..
            } => {
                for value in values {
                    *value = super::dml_subqueries::resolve_statement_subqueries_with_outer(
                        scope,
                        value,
                        outer_qualifiers,
                    )?;
                }
            }
            MergeAction::Delete
            | MergeAction::DoNothing
            | MergeAction::Insert { values: None, .. } => {}
        }
    }
    Ok(folded)
}

fn prepare_merge_row_subqueries(
    scope: &ExecutionScope<'_>,
    statement: &MergeStatement,
    outer_qualifiers: &HashSet<String>,
) -> Result<Vec<Vec<super::dml_row_subquery::PreparedRowSubquery>>, QueryError> {
    statement
        .clauses
        .iter()
        .map(|clause| match &clause.action {
            MergeAction::Update { row_subqueries, .. } => {
                super::dml_row_subquery::prepare_row_subqueries(
                    scope,
                    row_subqueries,
                    outer_qualifiers,
                )
            }
            _ => Ok(Vec::new()),
        })
        .collect()
}

impl<'a> MergeRun<'a> {
    fn source_row(&self, index: Option<usize>) -> Option<&[Value]> {
        index.map(|index| self.source.rows[index].as_slice())
    }

    /// The first clause of `kind` whose condition holds for `row`.
    fn select_clause(
        &self,
        kind: MergeMatch,
        row: &BatchRow,
    ) -> Result<Option<(usize, &'a MergeClause)>, QueryError> {
        let statement: &'a MergeStatement = self.statement;
        for (index, clause) in statement
            .clauses
            .iter()
            .enumerate()
            .filter(|(_, clause)| clause.kind == kind)
        {
            let Some(condition) = clause.condition.as_ref() else {
                return Ok(Some((index, clause)));
            };
            match super::dml_subqueries::evaluate_joined(&self.scope, row, condition)? {
                Value::Bool(true) => return Ok(Some((index, clause))),
                Value::Bool(false) | Value::Null => {}
                _ => {
                    return Err(QueryError::General(
                        "argument of MERGE WHEN must be type boolean".into(),
                    ));
                }
            }
        }
        Ok(None)
    }

    fn matched(
        &mut self,
        target_index: usize,
        source_index: Option<usize>,
        kind: MergeMatch,
    ) -> Result<(), QueryError> {
        check_timeout(self.scope.controls)?;
        let row = combined_row(
            &self.target,
            Some(&self.targets[target_index]),
            &self.source,
            self.source_row(source_index),
            Vec::new(),
        );
        let Some((clause_index, clause)) = self.select_clause(kind, &row)? else {
            return Ok(());
        };
        if matches!(clause.action, MergeAction::DoNothing) {
            return Ok(());
        }
        if !self.modified.insert(target_index) {
            return Err(QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "21000",
                    "MERGE command cannot affect row a second time",
                )
                .with_hint("Ensure that not more than one source row matches any one target row.")
                .into(),
            ));
        }
        let row_id = row_id_from_batch_row(&self.targets[target_index])?;
        match &clause.action {
            MergeAction::Update { assignments, .. } => {
                let Some(change) = update_target_row(
                    &self.scope,
                    &self.target,
                    &row_id,
                    &row,
                    assignments,
                    &[],
                    &self.prepared_row_subqueries[clause_index],
                )?
                else {
                    return Ok(());
                };
                self.affected += 1;
                self.push_returning(
                    "UPDATE",
                    &row_id,
                    source_index,
                    Some(&change.before),
                    Some(&change.after),
                );
            }
            MergeAction::Delete => {
                let Some(before) = delete_target_row(&self.scope, &self.target, &row_id)? else {
                    return Ok(());
                };
                self.affected += 1;
                self.push_returning("DELETE", &row_id, source_index, Some(&before), None);
            }
            MergeAction::DoNothing | MergeAction::Insert { .. } => {}
        }
        Ok(())
    }

    fn not_matched_by_target(&mut self, source_index: usize) -> Result<(), QueryError> {
        check_timeout(self.scope.controls)?;
        let row = combined_row(
            &self.target,
            None,
            &self.source,
            self.source_row(Some(source_index)),
            Vec::new(),
        );
        let Some((_, clause)) = self.select_clause(MergeMatch::NotMatchedByTarget, &row)? else {
            return Ok(());
        };
        let MergeAction::Insert {
            columns,
            values,
            overriding,
        } = &clause.action
        else {
            return Ok(());
        };
        let (columns, values) = self.insert_values(columns.as_slice(), values.as_deref(), &row)?;
        let insert = InsertStatement {
            table: self.statement.table.clone(),
            columns,
            source: InsertSource::Values(Vec::new()),
            on_conflict: None,
            returning: Vec::new(),
            overriding: *overriding,
            alias: None,
            returning_aliases: None,
            ctes: Vec::new(),
        };
        let row_id = super::dml_insert::insert_evaluated_row(
            self.scope.cassie,
            self.scope.session,
            &insert,
            values,
            self.scope.params,
            self.scope.user_functions,
            self.scope.controls,
        )?;
        self.affected += 1;
        if !self.statement.returning.is_empty() {
            let stored = self
                .scope
                .cassie
                .get_document_for_session(self.scope.session, self.target.table, &row_id)
                .map_err(QueryError::from)?
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "inserted row '{row_id}' was not found in '{}'",
                        self.target.table
                    ))
                })?;
            self.push_returning(
                "INSERT",
                &row_id,
                Some(source_index),
                None,
                Some(&stored.payload),
            );
        }
        Ok(())
    }

    /// Evaluates `INSERT` values; a `DEFAULT` value leaves its column to the
    /// column default.
    fn insert_values(
        &self,
        columns: &[String],
        values: Option<&[Expr]>,
        row: &BatchRow,
    ) -> Result<(Vec<String>, Vec<Value>), QueryError> {
        let Some(values) = values else {
            return Ok((Vec::new(), Vec::new()));
        };
        let columns = if columns.is_empty() {
            self.target
                .schema
                .fields
                .iter()
                .take(values.len())
                .map(|field| field.name.clone())
                .collect()
        } else {
            columns.to_vec()
        };
        let (_read, _overlay) =
            crate::executor::execution::entrypoints::enter_statement_read(self.scope.controls);
        let mut kept_columns = Vec::with_capacity(columns.len());
        let mut kept_values = Vec::with_capacity(values.len());
        for (column, value) in columns.into_iter().zip(values) {
            if value.is_default_assignment() {
                continue;
            }
            kept_values.push(super::dml_subqueries::evaluate_joined(
                &self.scope,
                row,
                value,
            )?);
            kept_columns.push(column);
        }
        Ok((kept_columns, kept_values))
    }

    fn push_returning(
        &mut self,
        action: &str,
        row_id: &str,
        source_index: Option<usize>,
        before: Option<&serde_json::Value>,
        after: Option<&serde_json::Value>,
    ) {
        if self.statement.returning.is_empty() {
            return;
        }
        let schema: &CollectionSchema = self.target.schema;
        let mut extras = vec![(
            MERGE_ACTION_COLUMN.to_string(),
            Value::String(action.to_string()),
        )];
        extras.extend(row_image_entries(
            schema,
            self.statement.returning_aliases.as_ref(),
            before,
            after,
        ));
        let image = after
            .or(before)
            .map(|payload| payload_row(row_id, schema, payload));
        let row = combined_row(
            &self.target,
            image.as_ref(),
            &self.source,
            self.source_row(source_index),
            extras,
        );
        self.returning.push(row);
    }
}
