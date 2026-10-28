use super::{
    combine_rows, qualify_row, BatchRow, CteContext, QueryError, QuerySource, SourceExecutionEnv,
    Value,
};

fn source_fields(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    namespace: &std::collections::HashMap<String, Vec<crate::types::FieldSchema>>,
) -> Result<Vec<crate::types::FieldSchema>, crate::app::CassieError> {
    let copy;
    let _memory;
    let source = if has_local_ctes(source) {
        let bytes = crate::sql::parse_analysis::source_clone_bytes(source, env.controls)?;
        _memory = env
            .controls
            .reserve_query_memory(crate::executor::retained_memory::mul(bytes, 2)?)?;
        let mut pruned = source.clone();
        prune_source(&mut pruned, env.controls)?;
        copy = pruned;
        &copy
    } else {
        source
    };
    crate::sql::binder::source_row_fields(
        source,
        namespace,
        &env.cassie.catalog,
        env.user_functions,
    )
}

fn has_local_ctes(source: &QuerySource) -> bool {
    match source {
        QuerySource::Subquery { select, .. } => {
            !select.ctes.is_empty() || has_local_ctes(&select.source)
        }
        QuerySource::Aliased { source, .. } => has_local_ctes(source),
        QuerySource::Join { left, right, .. } => has_local_ctes(left) || has_local_ctes(right),
        _ => false,
    }
}

fn prune_source(
    source: &mut QuerySource,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), crate::app::CassieError> {
    controls.check_interruption()?;
    match source {
        QuerySource::Subquery { select, .. } => prune_select(select, controls)?,
        QuerySource::Aliased { source, .. } => prune_source(source, controls)?,
        QuerySource::Join { left, right, .. } => {
            prune_source(left, controls)?;
            prune_source(right, controls)?;
        }
        _ => {}
    }
    Ok(())
}

fn prune_select(
    select: &mut crate::sql::ast::SelectStatement,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), crate::app::CassieError> {
    let live = super::super::cte_reachability::for_select(select, controls)?;
    let mut index = 0;
    select.ctes.retain(|_| {
        let reached = live.contains(index);
        index += 1;
        reached
    });
    drop(live);
    for cte in &mut select.ctes {
        let (first, second) = match &mut cte.query {
            crate::sql::ast::CteQuery::Simple(statement) => (statement, None),
            crate::sql::ast::CteQuery::Recursive {
                base, recursive, ..
            } => (base, Some(recursive)),
        };
        for statement in std::iter::once(first).chain(second) {
            if let crate::sql::ast::QueryStatement::Select(select) = &mut statement.statement {
                prune_select(select, controls)?;
            }
        }
    }
    prune_source(&mut select.source, controls)?;
    if let Some(set) = &mut select.set {
        prune_select(&mut set.right, controls)?;
    }
    Ok(())
}

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
    let inferred_copy = if let QuerySource::Cte(name) = unaliased(source) {
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
    let fields = source_fields(env, source, &namespace)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if let (QuerySource::Cte(name), Some(memory)) = (unaliased(source), inferred_copy) {
        return aliased_cte_template(env, source, fields, memory, name);
    }
    if super::super::dispatch::source_has_cte_boundary(source) {
        if let QuerySource::Subquery { alias, .. } = unaliased(source) {
            let memory = env
                .controls
                .reserve_query_memory(super::super::cte::source_fields_bytes(&fields)?)?;
            return aliased_cte_template(env, source, fields, memory, alias);
        }
    }
    let data_types = fields
        .iter()
        .any(|field| {
            matches!(
                field.data_type,
                crate::types::DataType::Array(_) | crate::types::DataType::Char { .. }
            )
        })
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
    let qualifier = source_qualifier(source);
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
    if let QuerySource::TableSample { source, .. } = source {
        return attach_types(env, source, context, batches);
    }
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
    let _inferred_copy = if let QuerySource::Cte(name) = unaliased(source) {
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
    let fields = source_fields(env, source, &namespace)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if !fields.iter().any(|field| {
        matches!(
            field.data_type,
            crate::types::DataType::Array(_) | crate::types::DataType::Char { .. }
        )
    }) {
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

fn source_qualifier(source: &QuerySource) -> Option<String> {
    match source {
        QuerySource::Aliased { alias, .. } => Some(crate::sql::binder::alias_row_qualifier(alias)),
        QuerySource::TableSample { source, .. } => source_qualifier(source),
        QuerySource::Collection(name) => Some(name.to_string()),
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => Some(name.clone()),
        QuerySource::Subquery { alias, .. } => Some(alias.clone()),
        QuerySource::Values { .. } | QuerySource::SingleRow | QuerySource::Join { .. } => None,
    }
}

#[path = "source_template.rs"]
mod template;
use template::known_cte_template;

fn unaliased(mut source: &QuerySource) -> &QuerySource {
    while let QuerySource::Aliased { source: inner, .. } = source {
        source = inner;
    }
    source
}

fn aliased_cte_template(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    fields: Vec<crate::types::FieldSchema>,
    memory: crate::runtime::QueryMemoryReservation,
    fallback_qualifier: &str,
) -> Result<BatchRow, QueryError> {
    if let QuerySource::Aliased { alias, .. } = source {
        let (qualifier, _scratch) = super::source_qualification::alias_qualifier(env, alias)?;
        known_cte_template(env, fields, memory, &qualifier)
    } else {
        known_cte_template(env, fields, memory, fallback_qualifier)
    }
}
