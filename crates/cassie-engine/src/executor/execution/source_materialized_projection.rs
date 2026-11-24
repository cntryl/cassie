use super::{
    schema_text_fields, source_collection, QueryError, SourceExecution, SourceExecutionEnv,
};

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    projection: &crate::catalog::ProjectionMeta,
    qualify: bool,
    row_budget: Option<usize>,
) -> SourceExecution {
    let visible;
    let projection = if projection
        .materialized
        .as_ref()
        .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
    {
        visible = env.cassie.midge.statement_materialized_view(name)?;
        visible.as_ref().ok_or_else(|| {
            QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "55000",
                    format!("materialized view '{name}' has no visible publication"),
                )
                .into(),
            )
        })?
    } else {
        projection
    };
    if let Some(definition) = &projection.materialized {
        crate::sql::definition_guard::projection(definition).map_err(QueryError::Cassie)?;
    }
    let Some(output_collection) = projection.active_output_collection() else {
        let is_materialized_view = projection
            .materialized
            .as_ref()
            .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view);
        if is_materialized_view {
            return Err(QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "55000",
                    format!("materialized view '{name}' has not been populated"),
                )
                .into(),
            ));
        }
        return Err(QueryError::General(format!(
            "materialized projection '{name}' has no active version"
        )));
    };
    let _read = crate::midge::adapter::StatementDataRead::enter_materialized_output(
        env.controls,
        name,
        output_collection,
    );
    let output_collection = output_collection.to_string();
    let batches = source_collection::scan_collection(
        env,
        &output_collection,
        row_budget,
        qualify,
        name,
        crate::midge::adapter::RowTraversal::CompleteSource,
    )?;
    let text_fields = projection
        .materialized
        .as_ref()
        .map_or_default(|materialized| schema_text_fields(&materialized.output_schema));
    Ok((batches, text_fields))
}
