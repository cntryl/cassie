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
    let data_types = fields
        .iter()
        .any(|field| matches!(field.data_type, crate::types::DataType::Array(_)))
        .then(|| {
            fields
                .iter()
                .map(|field| field.data_type.clone())
                .collect::<Vec<_>>()
                .into()
        });
    let row = BatchRow::new(
        fields
            .into_iter()
            .map(|field| (field.name, Value::Null))
            .collect(),
    )
    .with_optional_data_types(data_types);
    let qualifier = match source {
        QuerySource::Aliased { alias, .. } => Some(crate::sql::binder::alias_row_qualifier(alias)),
        QuerySource::Collection(name) => Some(name.to_string()),
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => Some(name.clone()),
        QuerySource::Subquery { alias, .. } => Some(alias.clone()),
        QuerySource::SingleRow | QuerySource::Join { .. } => None,
    };
    Ok(match qualifier {
        Some(qualifier) => qualify_row(row, &qualifier),
        None => row,
    })
}

pub(super) fn attach_types(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    context: &CteContext,
    batches: &mut [super::Batch],
) -> Result<(), QueryError> {
    match source {
        QuerySource::Collection(name)
            if env.cassie.catalog.get_schema(name).is_some()
                && env.cassie.catalog.get_view(name).is_none()
                && env
                    .cassie
                    .catalog
                    .get_materialized_projection(name)
                    .is_none() =>
        {
            // Base scans carry their physical identity even when `id` is a
            // declared column. Share the scan's types in actual entry order;
            // the public relation shape can omit that hidden identity.
            if let Some(types) = batches
                .iter()
                .flatten()
                .next()
                .and_then(BatchRow::shared_data_types)
            {
                for row in batches.iter_mut().flatten() {
                    row.set_data_types(types.clone());
                }
            }
            return Ok(());
        }
        _ => {}
    }
    let fields = crate::sql::binder::source_row_fields(
        source,
        &super::super::cte::context_fields(context),
        &env.cassie.catalog,
        env.user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    if !fields
        .iter()
        .any(|field| matches!(field.data_type, crate::types::DataType::Array(_)))
    {
        return Ok(());
    }
    let types: std::sync::Arc<Vec<crate::types::DataType>> = fields
        .into_iter()
        .map(|field| field.data_type)
        .collect::<Vec<_>>()
        .into();
    for row in batches.iter_mut().flatten() {
        row.set_data_types(types.clone());
    }
    Ok(())
}
