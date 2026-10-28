//! `SET (a, b) = (SELECT ...)`: evaluates a one-row subquery per target row.
//!
//! References the subquery cannot resolve against its own relations are
//! outer references to the joined mutation row; they are replaced with that
//! row's values before the subquery is bound and run.

use std::collections::HashSet;

use super::dml_joined::ExecutionScope;
use super::{
    check_timeout, execute_plan, BatchRow, CteContext, Expr, QueryError, QuerySource, Value,
};
use crate::sql::ast::{QueryStatement, RowSubqueryAssignment, SelectItem, SelectStatement};

/// The values one row subquery assigns to its columns.
pub(super) fn evaluate_row_subquery(
    scope: &ExecutionScope<'_>,
    assignment: &RowSubqueryAssignment,
    outer: &BatchRow,
) -> Result<Vec<Value>, QueryError> {
    check_timeout(scope.controls)?;
    let mut statement = (*assignment.query).clone();
    let mut params = scope.params.to_vec();
    if let QueryStatement::Select(select) = &mut statement.statement {
        substitute_select(select, outer, &mut params);
    }
    let plan = crate::executor::execution::dispatch::build_logical_plan_in_session(
        scope.cassie,
        scope.session,
        &statement,
    )?;
    let controls = crate::executor::execution::entrypoints::statement_read_controls(
        scope.cassie,
        scope.session,
        scope.controls,
    )?;
    let (_read_scope, _overlay_scope) =
        crate::executor::execution::entrypoints::enter_statement_read(&controls);
    let mut cte_context = CteContext::new();
    let rows = execute_plan(
        scope.cassie,
        scope.session,
        &plan,
        &mut cte_context,
        scope.user_functions,
        &params,
        &controls,
    )?;
    let mut rows = rows.into_iter();
    let Some(first) = rows.next() else {
        return Ok(vec![Value::Null; assignment.columns.len()]);
    };
    if rows.next().is_some() {
        return Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(
                "more than one row returned by a subquery used as an expression".to_string(),
            ),
        ));
    }
    let values = first
        .into_entries()
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    if values.len() != assignment.columns.len() {
        return Err(QueryError::Cassie(crate::app::CassieError::Parse(
            "number of columns does not match number of values".to_string(),
        )));
    }
    Ok(values)
}

fn substitute_select(select: &mut SelectStatement, outer: &BatchRow, params: &mut Vec<Value>) {
    let mut inner = HashSet::new();
    source_qualifiers(&select.source, &mut inner);
    for cte in &select.ctes {
        inner.insert(cte.name.to_ascii_lowercase());
    }
    let bare_outer = matches!(select.source, QuerySource::SingleRow);
    let context = Substitution {
        outer,
        inner: &inner,
        bare_outer,
    };
    for item in &mut select.projection {
        match item {
            SelectItem::Column { name, alias } => {
                let expr = context.substitute(&Expr::Column(name.clone()), params);
                if !matches!(expr, Expr::Column(_)) {
                    let alias = alias.take().or_else(|| Some(output_label(name)));
                    *item = SelectItem::Expr { expr, alias };
                }
            }
            SelectItem::Expr { expr, .. } => *expr = context.substitute(expr, params),
            SelectItem::Function { function, .. } => {
                for argument in function.expressions_mut() {
                    *argument = context.substitute(argument, params);
                }
            }
            SelectItem::Wildcard | SelectItem::WindowFunction { .. } => {}
        }
    }
    for expr in select
        .filter
        .iter_mut()
        .chain(select.having.iter_mut())
        .chain(select.group_by.iter_mut())
        .chain(select.distinct_on.iter_mut())
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        *expr = context.substitute(expr, params);
    }
    substitute_source(&mut select.source, &context, params);
}

fn output_label(name: &str) -> String {
    crate::sql::ColumnIdentifierPath::parse(name).map_or_else(
        |_| name.to_string(),
        |column| column.implicit_output_label(),
    )
}

fn source_qualifiers(source: &QuerySource, out: &mut HashSet<String>) {
    match source {
        QuerySource::Collection(name) => {
            out.insert(crate::catalog::local_name(name).to_ascii_lowercase());
            out.insert(name.to_string().to_ascii_lowercase());
        }
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            out.insert(name.to_ascii_lowercase());
        }
        QuerySource::Aliased { alias, .. } | QuerySource::Subquery { alias, .. } => {
            out.insert(alias.to_ascii_lowercase());
        }
        QuerySource::TableSample { source, .. } => source_qualifiers(source, out),
        QuerySource::Join { left, right, .. } => {
            source_qualifiers(left, out);
            source_qualifiers(right, out);
        }
        QuerySource::Values { .. } | QuerySource::SingleRow => {}
    }
}

fn substitute_source(
    source: &mut QuerySource,
    context: &Substitution<'_>,
    params: &mut Vec<Value>,
) {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => {
            *on = context.substitute(on, params);
            substitute_source(left, context, params);
            substitute_source(right, context, params);
        }
        QuerySource::Aliased { source, .. } => substitute_source(source, context, params),
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            substitute_source(source, context, params);
            *rate = context.substitute(rate, params);
            if let Some(seed) = repeatable {
                *seed = context.substitute(seed, params);
            }
        }
        QuerySource::Values { rows } => {
            for value in rows.iter_mut().flatten() {
                *value = context.substitute(value, params);
            }
        }
        QuerySource::Subquery { select, .. } => substitute_select(select, context.outer, params),
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::SingleRow => {}
    }
}

struct Substitution<'a> {
    outer: &'a BatchRow,
    inner: &'a HashSet<String>,
    bare_outer: bool,
}

impl Substitution<'_> {
    fn substitute(&self, expr: &Expr, params: &mut Vec<Value>) -> Expr {
        match expr {
            Expr::Column(name) => self
                .outer_value(name)
                .map_or_else(|| expr.clone(), |value| literal(value, params)),
            Expr::Exists(_) => expr.clone(),
            _ => expr.map_children(|child| self.substitute(child, params)),
        }
    }

    fn outer_value(&self, name: &str) -> Option<Value> {
        let column = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
        if column.is_qualified() {
            let field = column.field_lookup_key();
            let lookup = column.lookup_key();
            let qualifier = lookup.strip_suffix(&format!(".{field}"))?;
            if self.inner.contains(&qualifier.to_ascii_lowercase()) {
                return None;
            }
        } else if !self.bare_outer {
            return None;
        }
        self.outer.get(name).cloned()
    }
}

fn literal(value: Value, params: &mut Vec<Value>) -> Expr {
    match value {
        Value::Null => Expr::Null,
        Value::Int64(value) => Expr::IntegerLiteral(value),
        Value::Bool(value) => Expr::BoolLiteral(value),
        Value::String(value) => Expr::StringLiteral(value),
        value => {
            params.push(value);
            Expr::Param(params.len() - 1)
        }
    }
}
