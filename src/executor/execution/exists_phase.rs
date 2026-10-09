//! Borrow the actual phase row while evaluating reached correlated scalar nodes.
use super::source::SourceExecutionEnv;
use super::{
    check_timeout, ensure_query_memory_budget, filter, Batch, BatchRow, CteContext,
    ExistsResolutionContext, Expr, LogicalPlan, QueryError, QuerySource, Value,
};

pub(super) fn evaluate(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    source: &QuerySource,
    row: &BatchRow,
    expr: &Expr,
    search: Option<&filter::SearchContext>,
) -> Result<Value, QueryError> {
    if !super::exists_correlated::contains_exists(expr) {
        return filter::evaluate_expr_value(
            row,
            expr,
            env.params,
            search,
            env.user_functions,
            env.session,
            None,
        );
    }
    check_timeout(env.controls)?;
    let outer = super::exists_projection::outer_row(env, source, row)?;
    let context = ExistsResolutionContext {
        cassie: env.cassie,
        session: env.session,
        cte_context: ctes,
        user_functions: env.user_functions,
        params: env.params,
        controls: env.controls,
        outer_row: Some(&outer),
    };
    let resolve = |expr: &Expr| match super::resolve_exists_expr(&context, expr)? {
        Expr::BoolLiteral(value) => Ok(value),
        _ => Err(QueryError::General(
            "EXISTS resolver did not return BOOLEAN".into(),
        )),
    };
    filter::evaluate_resolving_exists(
        row,
        expr,
        filter::ExistsValueContext {
            params: env.params,
            search,
            functions: env.user_functions,
            session: env.session,
            resolver: &resolve,
        },
    )
}

pub(super) fn having(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    plan: &LogicalPlan,
    mut batches: Vec<Batch>,
    search: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let Some(having) = plan.having.as_ref() else {
        return Ok(batches);
    };
    let having = super::aggregate_exec::rewrite_aggregate_expr(having, &plan.group_by);
    let _output_memory = ensure_query_memory_budget(env.controls, &batches)?;
    if super::exists_correlated::contains_exists(&having) {
        for batch in &mut batches {
            let mut failure = None;
            batch.retain(|row| {
                if failure.is_some() {
                    return false;
                }
                match evaluate(env, ctes, &plan.source, row, &having, search) {
                    Ok(Value::Bool(value)) => value,
                    Ok(Value::Null) => false,
                    Ok(_) => {
                        failure = Some(QueryError::General(
                            "Boolean expression requires BOOLEAN or SQL NULL".into(),
                        ));
                        false
                    }
                    Err(error) => {
                        failure = Some(error);
                        false
                    }
                }
            });
            if let Some(error) = failure {
                return Err(error);
            }
        }
    } else {
        batches = filter::filter_batches(
            batches,
            &having,
            env.params,
            search,
            env.user_functions,
            env.session,
        )?;
    }
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok(batches)
}
