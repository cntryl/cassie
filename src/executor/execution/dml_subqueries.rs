//! Subquery expressions in a single-table UPDATE or DELETE.
//!
//! An uncorrelated subquery is run once for the statement. One that
//! references the target row is run for that row, with the row as its outer
//! scope. Every subquery reads the data as it was when the statement began.
use super::{
    filter, BatchRow, Cassie, CassieSession, CteContext, Expr, FunctionMeta, HashMap, QueryError,
    QueryExecutionControls, QuerySource, Value,
};

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
        let (_read, _overlay) =
            crate::executor::execution::entrypoints::enter_statement_read(self.controls);
        match crate::executor::execution::resolve_statement_exists(
            self.cassie,
            self.session,
            expr,
            self.user_functions,
            self.params,
            self.controls,
        ) {
            Ok(resolved) => Ok(resolved),
            Err(error) if crate::executor::execution::subquery_eval::is_interruption(&error) => {
                Err(error)
            }
            Err(_) => Ok(expr.clone()),
        }
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
            user_functions: self.user_functions,
            params: self.params,
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
                    ))
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
