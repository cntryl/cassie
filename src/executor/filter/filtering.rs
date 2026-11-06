//! Filtering with explicit statement execution controls.
use super::{
    eval_scalar_with_context, Batch, CassieSession, EvalContext, Expr, FunctionMeta, HashMap,
    QueryError, RowAccess, SearchContext, Value,
};

pub(crate) fn filter_rows<R>(
    rows: Vec<R>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<R>, QueryError>
where
    R: RowAccess,
{
    filter_rows_in_context(
        rows,
        expression,
        EvalContext {
            params,
            search_context,
            user_functions,
            local_args: None,
            session,
            exists: None,
            controls: None,
            memory: None,
        },
    )
}

fn filter_rows_in_context<R: RowAccess>(
    rows: Vec<R>,
    expression: &Expr,
    context: EvalContext<'_>,
) -> Result<Vec<R>, QueryError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(controls) = context.controls {
            controls.check_interruption()?;
        }
        if eval_scalar_with_context(&row, expression, context)?.is_true()? {
            out.push(row);
        }
    }
    Ok(out)
}

pub(crate) fn filter_batches(
    batches: Vec<Batch>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    batches
        .into_iter()
        .map(|batch| {
            filter_rows(
                batch,
                expression,
                params,
                search_context,
                user_functions,
                session,
            )
        })
        .collect()
}

pub(crate) fn filter_batches_with_controls(
    batches: Vec<Batch>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let context = EvalContext {
        params,
        search_context,
        user_functions,
        local_args: None,
        session,
        exists: None,
        controls: Some(controls),
        memory: None,
    };
    batches
        .into_iter()
        .map(|batch| filter_rows_in_context(batch, expression, context))
        .collect()
}
