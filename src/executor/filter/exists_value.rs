//! Borrow a selected SELECT resolver without changing ordinary scalar entrypoints.
use super::{
    evaluate_expr_value_with_context, CassieSession, EvalContext, ExistsResolver, Expr,
    FunctionMeta, HashMap, QueryError, RowAccess, SearchContext, Value,
};

#[derive(Clone, Copy)]
pub(crate) struct ExistsValueContext<'a> {
    pub(crate) params: &'a [Value],
    pub(crate) search: Option<&'a SearchContext>,
    pub(crate) functions: &'a HashMap<String, FunctionMeta>,
    pub(crate) session: Option<&'a CassieSession>,
    pub(crate) resolver: &'a ExistsResolver<'a>,
}

pub(crate) fn evaluate_resolving_exists<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: ExistsValueContext<'_>,
) -> Result<Value, QueryError> {
    evaluate_expr_value_with_context(
        row,
        expr,
        EvalContext {
            params: context.params,
            search_context: context.search,
            user_functions: context.functions,
            local_args: None,
            session: context.session,
            exists: Some(context.resolver),
        },
    )
}

#[cfg(test)]
#[path = "exists_value_tests.rs"]
mod tests;
