//! Materialize scalar VALUES before starting the insertion loop.
use super::{
    check_timeout, filter, insert_expr_to_value, CassieSession, Expr, FunctionMeta, HashMap,
    QueryError, QueryExecutionControls, Value,
};

pub(super) fn evaluate(
    rows: &[Vec<Expr>],
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    rows.iter().map(|row| {
        row.iter().map(|expression| {
            check_timeout(controls)?;
            if matches!(expression, Expr::StringLiteral(_) | Expr::NumberLiteral(_) | Expr::IntegerLiteral(_) | Expr::BoolLiteral(_) | Expr::Null | Expr::Param(_)) {
                return insert_expr_to_value(expression, params).map_err(QueryError::General);
            }
            if expression.any_descendant_or_self(&mut |child| matches!(child, Expr::Function(function) if crate::sql::functions::is_aggregate_function(&function.name))) {
                return Err(QueryError::Cassie(crate::app::CassieError::Execution("aggregate functions are not allowed in VALUES".into())));
            }
            if expression.any_descendant_or_self(&mut |child| matches!(child, Expr::Column(_) | Expr::Exists(_))) {
                return Err(QueryError::General("INSERT VALUES scalar expressions cannot reference columns or subqueries".into()));
            }
            filter::evaluate_expr_value(&Vec::<(String, Value)>::new(), expression, params, None, functions, session, None)
        }).collect()
    }).collect()
}
