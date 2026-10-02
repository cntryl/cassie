use super::{
    combine_rows, qualify_row, BatchRow, CteContext, QueryError, QuerySource, SourceExecutionEnv,
    Value,
};

pub(super) fn null_row(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    context: &CteContext,
) -> Result<BatchRow, QueryError> {
    if let QuerySource::Join { left, right, .. } = source {
        return Ok(combine_rows(
            &null_row(env, left, context)?,
            &null_row(env, right, context)?,
        ));
    }
    let fields = crate::sql::binder::source_row_fields(
        source,
        &super::super::cte::context_fields(context),
        &env.cassie.catalog,
        env.user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    let row = BatchRow::new(
        fields
            .into_iter()
            .map(|field| (field.name, Value::Null))
            .collect(),
    );
    let qualifier = match source {
        QuerySource::Collection(name)
        | QuerySource::Cte(name)
        | QuerySource::TableFunction { name, .. } => Some(name),
        QuerySource::Subquery { alias, .. } => Some(alias),
        QuerySource::SingleRow | QuerySource::Join { .. } => None,
    };
    Ok(match qualifier {
        Some(qualifier) => qualify_row(row, qualifier),
        None => row,
    })
}
