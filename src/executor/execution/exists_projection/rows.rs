//! Resolve deferred SELECT nodes with an admitted actual outer-row view.
use super::super::source::SourceExecutionEnv;
use super::super::{
    Batch, BatchRow, CteContext, ExistsResolutionContext, Expr, QueryError, SelectItem,
};
use super::{admission, check_timeout};

pub(in crate::executor::execution) fn project(
    env: &SourceExecutionEnv<'_>,
    context: &CteContext,
    source: &crate::sql::QuerySource,
    batches: Vec<Batch>,
    projection: &[SelectItem],
    search: Option<&crate::executor::filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(batches.len())
        .map_err(|error| allocation(&error))?;
    for batch in batches {
        let mut projected = Vec::new();
        projected
            .try_reserve_exact(batch.len())
            .map_err(|error| allocation(&error))?;
        for row in batch {
            check_timeout(env.controls)?;
            let outer = outer_row(env, source, &row)?;
            let context = ExistsResolutionContext {
                cassie: env.cassie,
                session: env.session,
                cte_context: context,
                user_functions: env.user_functions,
                params: env.params,
                controls: env.controls,
                outer_row: Some(&outer),
            };
            let resolve = |expr: &Expr| match super::super::resolve_exists_expr(&context, expr)? {
                Expr::BoolLiteral(value) => Ok(value),
                _ => Err(QueryError::General(
                    "EXISTS resolver did not return BOOLEAN".into(),
                )),
            };
            let result = crate::executor::projection::project_batches_resolving_exists(
                vec![vec![row]],
                projection,
                env.params,
                search,
                env.user_functions,
                env.session,
                &resolve,
            )?;
            projected.extend(result.into_iter().flatten());
        }
        output.push(projected);
    }
    Ok(output)
}

fn outer_row(
    env: &SourceExecutionEnv<'_>,
    source: &crate::sql::QuerySource,
    row: &BatchRow,
) -> Result<BatchRow, QueryError> {
    let _qualifier_scratch = admission::reserve_clone(&source, env.controls)?;
    let qualifier = super::super::exists_correlated::outer_qualifier(source);
    let memory =
        admission::reserve_clone(&(row.entries(), row.aliases(), &qualifier), env.controls)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(row.entries().len())
        .map_err(|error| allocation(&error))?;
    for (name, value) in row.entries() {
        check_timeout(env.controls)?;
        let name = super::super::outer_names::outer_field_name(
            &env.cassie.catalog,
            source,
            qualifier.as_deref(),
            name,
        );
        entries.push((name, value.clone()));
    }
    let memory = std::sync::Arc::new(memory);
    let mut outer = BatchRow::new(entries)
        .with_optional_data_types(row.shared_data_types())
        .with_query_memory(row.query_memory())
        .with_operator_memory(row.operator_memory());
    outer.attach_operator_memory(env.controls, memory)?;
    Ok(outer)
}

fn allocation(error: &std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain EXISTS output: {error}"))
        .into()
}
