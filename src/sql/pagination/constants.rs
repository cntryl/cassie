//! Fold admitted integer bound constants at planning time without applying bounds.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::Expr;

pub(crate) fn check(
    expression: &Expr,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    // Semantic admission owns unsupported forms. It also supplies the finite
    // depth envelope before the integer evaluator or parameter walk recurses.
    if !super::is_admitted(expression) || expression.contains_subquery() {
        return Ok(());
    }
    check_admitted(expression, controls)
}

fn check_admitted(expression: &Expr, controls: &QueryExecutionControls) -> Result<(), CassieError> {
    controls.check_interruption()?;
    if super::has_parameters(expression) {
        expression.try_visit_children(&mut |child| check_admitted(child, controls))?;
    } else {
        // A negative value is an execution-time LIMIT/OFFSET error, whereas
        // overflow while computing that value is a planning-time error.
        super::evaluate(expression, &[], &[], controls)?;
    }
    Ok(())
}
