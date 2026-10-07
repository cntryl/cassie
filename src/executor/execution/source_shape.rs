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
        let left = null_row(env, left, context)?;
        let right = null_row(env, right, context)?;
        if left.operator_memory().is_some() || right.operator_memory().is_some() {
            let bytes = super::source_join::combined_row_bytes(&left, &right)?;
            let memory = std::sync::Arc::new(env.controls.reserve_query_memory(
                crate::executor::retained_memory::add(
                    bytes,
                    std::mem::size_of::<crate::runtime::QueryMemoryReservation>()
                        + 2 * std::mem::size_of::<usize>(),
                )?,
            )?);
            return Ok(combine_rows(&left, &right)?
                .with_query_memory(Some(std::sync::Arc::clone(&memory)))
                .retain_operator_memory(env.controls, memory)?);
        }
        return combine_rows(&left, &right);
    }
    let namespace = super::super::cte::context_fields(context, env.controls)?;
    let inferred_copy = if let QuerySource::Cte(name) = source {
        let _name_memory = env.controls.reserve_query_memory(name.len())?;
        let key = name.to_ascii_lowercase();
        context
            .get(&key)
            .map(|relation| {
                super::super::cte::source_fields_bytes(&relation.fields)
                    .and_then(|bytes| env.controls.reserve_query_memory(bytes).map_err(Into::into))
            })
            .transpose()?
    } else {
        None
    };
    let fields = crate::sql::binder::source_row_fields(
        source,
        &namespace,
        &env.cassie.catalog,
        env.user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    if let (QuerySource::Cte(name), Some(memory)) = (source, inferred_copy) {
        return known_cte_template(env, fields, memory, name);
    }
    if super::super::dispatch::source_has_cte_boundary(source) {
        if let QuerySource::Subquery { alias, .. } = source {
            let memory = env
                .controls
                .reserve_query_memory(super::super::cte::source_fields_bytes(&fields)?)?;
            return known_cte_template(env, fields, memory, alias);
        }
    }
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
    let namespace = super::super::cte::context_fields(context, env.controls)?;
    let _inferred_copy = if let QuerySource::Cte(name) = source {
        let _name_memory = env.controls.reserve_query_memory(name.len())?;
        let key = name.to_ascii_lowercase();
        context
            .get(&key)
            .map(|relation| {
                super::super::cte::source_fields_bytes(&relation.fields)
                    .and_then(|bytes| env.controls.reserve_query_memory(bytes).map_err(Into::into))
            })
            .transpose()?
    } else {
        None
    };
    let fields = crate::sql::binder::source_row_fields(
        source,
        &namespace,
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
    let retained = batches
        .iter()
        .flatten()
        .any(|row| row.query_memory().is_some() || row.operator_memory().is_some());
    let memory = if retained {
        use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
        use std::mem::size_of;
        let bytes = fields.iter().try_fold(
            add(
                size_of::<Vec<crate::types::DataType>>() + 2 * size_of::<usize>(),
                mul(fields.len(), size_of::<crate::types::DataType>())?,
            )?,
            |bytes, field| add(bytes, data_type_clone_bytes(&field.data_type)?),
        )?;
        Some(std::sync::Arc::new(env.controls.reserve_query_memory(
            add(
                bytes,
                size_of::<crate::runtime::QueryMemoryReservation>() + 2 * size_of::<usize>(),
            )?,
        )?))
    } else {
        None
    };
    // Explicit backing avoids reusing the larger FieldSchema allocation through
    // in-place collect, which can retain more type slots than the admitted length.
    let mut types = Vec::new();
    types.try_reserve_exact(fields.len()).map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to retain source descriptors: {error}"
        ))
    })?;
    for field in fields {
        types.push(field.data_type);
    }
    let types = std::sync::Arc::new(types);
    for row in batches.iter_mut().flatten() {
        if row.query_memory().is_some() || row.operator_memory().is_some() {
            if let Some(memory) = &memory {
                row.attach_operator_memory(env.controls, std::sync::Arc::clone(memory))?;
            }
        }
        row.set_data_types(types.clone());
    }
    Ok(())
}

#[path = "source_template.rs"]
mod template;
use template::known_cte_template;
