//! Subquery expressions in a single-table UPDATE or DELETE.
//!
//! An uncorrelated subquery is run once for the statement. One that
//! references the target row is run for that row, with the row as its outer
//! scope. Every subquery reads the data as it was when the statement began.
use super::{
    filter, BatchRow, Cassie, CassieSession, CteContext, Expr, FunctionMeta, HashMap, QueryError,
    QueryExecutionControls, QuerySource, Value,
};
use std::collections::HashSet;

pub(super) struct RowSubqueries<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    /// Controls carrying the statement's starting data view.
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) table: &'a crate::sql::ast::IdentifierPath,
    pub(super) alias: Option<&'a str>,
}

impl RowSubqueries<'_> {
    /// Runs every uncorrelated subquery of `expr` once; a correlated one is
    /// left for [`Self::evaluate`].
    pub(super) fn fold(&self, expr: &Expr) -> Result<Expr, QueryError> {
        if !expr.contains_subquery() {
            return Ok(expr.clone());
        }
        resolve_expression_subqueries(
            expr,
            self.cassie,
            self.session,
            self.user_functions,
            self.params,
            self.controls,
            &self
                .alias
                .into_iter()
                .map(str::to_ascii_lowercase)
                .chain(std::iter::once(
                    crate::catalog::local_name(self.table).to_ascii_lowercase(),
                ))
                .chain(std::iter::once(String::new()))
                .collect(),
        )
    }

    /// Evaluates `expr` for `row`, running a correlated subquery with the row
    /// as its outer scope.
    pub(super) fn evaluate(&self, row: &BatchRow, expr: &Expr) -> Result<Value, QueryError> {
        if !expr.contains_subquery() {
            return filter::evaluate_expr_value(
                row,
                expr,
                self.params,
                None,
                self.user_functions,
                self.session,
                None,
            );
        }
        let (_read, _overlay) =
            crate::executor::execution::entrypoints::enter_statement_read(self.controls);
        let env = crate::executor::execution::source::SourceExecutionEnv {
            cassie: self.cassie,
            session: self.session,
            binding_context: None,
            user_functions: self.user_functions,
            params: self.params,
            parameter_type_oids: &[],
            controls: self.controls,
        };
        crate::executor::execution::exists_phase::evaluate(
            &env,
            &CteContext::new(),
            &self.source(),
            row,
            expr,
            None,
        )
    }

    /// Keeps the rows for which `filter_expr` is true.
    pub(super) fn filter(
        &self,
        rows: Vec<BatchRow>,
        filter_expr: &Expr,
    ) -> Result<Vec<BatchRow>, QueryError> {
        let declares_id = crate::planner::logical::collection_declares_id(
            &self.cassie.catalog,
            self.table.as_ref(),
        );
        let mut filter_expr = self.fold(filter_expr)?;
        crate::planner::logical::rewrite_expr_for_schema(&mut filter_expr, declares_id);
        if !filter_expr.contains_subquery() || all_resolved(&filter_expr) {
            return filter::filter_rows(
                rows,
                &filter_expr,
                self.params,
                None,
                self.user_functions,
                self.session,
            );
        }
        let mut kept = Vec::new();
        for row in rows {
            super::check_timeout(self.controls)?;
            match self.evaluate(&row, &filter_expr)? {
                Value::Bool(true) => kept.push(row),
                Value::Bool(false) | Value::Null => {}
                _ => {
                    return Err(QueryError::General(
                        "argument of WHERE must be type boolean".into(),
                    ));
                }
            }
        }
        Ok(kept)
    }

    fn source(&self) -> QuerySource {
        let source = QuerySource::Collection(self.table.clone());
        match self.alias {
            Some(alias) => QuerySource::Aliased {
                source: Box::new(source),
                alias: alias.to_string(),
                column_aliases: Vec::new(),
            },
            None => source,
        }
    }
}

/// Returns whether every subquery of `expr` already carries its rows.
fn all_resolved(expr: &Expr) -> bool {
    !expr.any_descendant_or_self(&mut |node| match node {
        Expr::Exists(_) => true,
        Expr::Subquery(subquery) => subquery.outcome.is_none(),
        _ => false,
    })
}

fn should_defer_correlated_resolution(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::Cassie(crate::app::CassieError::DeferredCorrelatedResolution { .. })
    )
}

/// Replaces every uncorrelated subquery of `expr` by its value, read from the
/// data as it was when the statement began. An expression without a subquery
/// is returned unchanged.
pub(super) fn resolve_statement_subqueries(
    scope: &super::dml_joined::ExecutionScope<'_>,
    expr: &Expr,
) -> Result<Expr, QueryError> {
    resolve_statement_subqueries_with_outer(scope, expr, &HashSet::new())
}

pub(super) fn resolve_statement_subqueries_with_outer(
    scope: &super::dml_joined::ExecutionScope<'_>,
    expr: &Expr,
    outer_qualifiers: &HashSet<String>,
) -> Result<Expr, QueryError> {
    if !expr.contains_subquery() {
        return Ok(expr.clone());
    }
    resolve_expression_subqueries(
        expr,
        scope.cassie,
        scope.session,
        scope.user_functions,
        scope.params,
        scope.controls,
        outer_qualifiers,
    )
}

fn resolve_expression_subqueries(
    expr: &Expr,
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_qualifiers: &HashSet<String>,
) -> Result<Expr, QueryError> {
    if matches!(expr, Expr::Subquery(_) | Expr::Exists(_)) {
        let (_read, _overlay) =
            crate::executor::execution::entrypoints::enter_statement_read(controls);
        return match crate::executor::execution::resolve_statement_exists(
            cassie,
            session,
            expr,
            user_functions,
            params,
            controls,
        ) {
            Ok(resolved) => Ok(resolved),
            Err(error) if crate::executor::execution::subquery_eval::is_interruption(&error) => {
                Err(error)
            }
            Err(error) if should_defer_correlated_resolution(&error) => Ok(expr.clone()),
            Err(QueryError::Cassie(crate::app::CassieError::UndefinedColumn(name)))
                if undefined_outer_qualifier(&name, outer_qualifiers) =>
            {
                Ok(expr.clone())
            }
            Err(error) => Err(error),
        };
    }

    let (_read, _overlay) = crate::executor::execution::entrypoints::enter_statement_read(controls);
    let mut resolved = expr.clone();
    resolved.try_visit_children_mut(|child| {
        *child = resolve_expression_subqueries(
            child,
            cassie,
            session,
            user_functions,
            params,
            controls,
            outer_qualifiers,
        )?;
        Ok::<(), QueryError>(())
    })?;
    Ok(resolved)
}

fn undefined_outer_qualifier(name: &str, outer_qualifiers: &HashSet<String>) -> bool {
    let parsed = crate::sql::ColumnIdentifierPath::parse(name)
        .ok()
        .and_then(|column| column.namespace_qualifier());
    parsed
        .as_ref()
        .map(|qualifier| qualifier.to_ascii_lowercase())
        .is_some_and(|qualifier| outer_qualifiers.contains(&qualifier))
        || (outer_qualifiers.contains("") && parsed.is_none())
}

/// Evaluates a subquery expression against a joined DML row. The row already
/// carries the target and source under their bound qualified names.
pub(super) fn evaluate_joined(
    scope: &super::dml_joined::ExecutionScope<'_>,
    row: &BatchRow,
    expr: &Expr,
) -> Result<Value, QueryError> {
    if !expr.contains_subquery() {
        return filter::evaluate_expr_value(
            row,
            expr,
            scope.params,
            None,
            scope.user_functions,
            scope.session,
            None,
        );
    }
    let (_read, _overlay) =
        crate::executor::execution::entrypoints::enter_statement_read(scope.controls);
    let ctes = CteContext::new();
    let context = super::ExistsResolutionContext {
        cassie: scope.cassie,
        session: scope.session,
        binding_context: None,
        cte_context: &ctes,
        user_functions: scope.user_functions,
        params: scope.params,
        parameter_type_oids: &[],
        controls: scope.controls,
        outer_row: Some(row),
    };
    let resolve = |expr: &Expr, _: Option<&HashMap<String, Value>>| {
        super::resolve_exists_expr(&context, expr)
    };
    filter::evaluate_resolving_exists(
        row,
        expr,
        filter::ExistsValueContext {
            params: scope.params,
            search: None,
            functions: scope.user_functions,
            session: scope.session,
            resolver: &resolve,
        },
    )
}
