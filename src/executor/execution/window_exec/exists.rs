//! Window arguments resolve against the frame-selected input row.
use super::{typed, Batch, SelectItem, WindowExecutionContext};
use crate::executor::execution::{source::SourceExecutionEnv, CteContext, QueryError, QuerySource};
use crate::executor::filter::SearchContext;

pub(in crate::executor::execution) fn apply_resolving(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    source: &QuerySource,
    batches: Vec<Batch>,
    projection: &[SelectItem],
    search: Option<&SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let evaluate = |row: &super::BatchRow, expr: &crate::sql::ast::Expr| {
        super::super::exists_phase::evaluate(env, ctes, source, row, expr, search)
    };
    typed::apply(
        batches,
        projection,
        &WindowExecutionContext {
            params: env.params,
            search_context: search,
            user_functions: env.user_functions,
            session: env.session,
            controls: env.controls,
            evaluate: Some(&evaluate),
        },
    )
}
