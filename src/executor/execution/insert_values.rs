//! Materialize scalar VALUES before starting the insertion loop.
use super::{
    check_timeout, filter, insert_expr_to_value, Cassie, CassieSession, Expr, FunctionMeta,
    HashMap, QueryError, QueryExecutionControls, Value,
};

pub(super) struct ValuesContext<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) params: &'a [Value],
    pub(super) functions: &'a HashMap<String, FunctionMeta>,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) controls: &'a QueryExecutionControls,
}

pub(super) fn evaluate(
    rows: &[Vec<Expr>],
    context: &ValuesContext<'_>,
) -> Result<Vec<Vec<Value>>, QueryError> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|expression| value(expression, context))
                .collect()
        })
        .collect()
}

fn value(expression: &Expr, context: &ValuesContext<'_>) -> Result<Value, QueryError> {
    check_timeout(context.controls)?;
    if matches!(
        expression,
        Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null
            | Expr::Param(_)
    ) {
        return insert_expr_to_value(expression, context.params).map_err(QueryError::General);
    }
    if expression.any_descendant_or_self(&mut |child| {
        matches!(child, Expr::Function(function) if crate::sql::functions::is_aggregate_function(&function.name))
    }) {
        return Err(QueryError::Cassie(crate::app::CassieError::Execution(
            "aggregate functions are not allowed in VALUES".into(),
        )));
    }
    if expression.any_descendant_or_self(&mut |child| matches!(child, Expr::Column(_))) {
        return Err(QueryError::General(
            "INSERT VALUES scalar expressions cannot reference columns".into(),
        ));
    }
    let resolved;
    let expression = if expression.contains_subquery() {
        // A VALUES subquery reads the data as it was when the statement began.
        let controls = crate::executor::execution::entrypoints::statement_read_controls(
            context.cassie,
            context.session,
            context.controls,
        )?;
        let (_read, _overlay) =
            crate::executor::execution::entrypoints::enter_statement_read(&controls);
        resolved = crate::executor::execution::resolve_statement_exists(
            context.cassie,
            context.session,
            expression,
            context.functions,
            context.params,
            &controls,
        )?;
        &resolved
    } else {
        expression
    };
    filter::evaluate_expr_value(
        &Vec::<(String, Value)>::new(),
        expression,
        context.params,
        None,
        context.functions,
        context.session,
        None,
    )
}
