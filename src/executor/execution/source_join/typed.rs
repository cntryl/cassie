//! Admitted compatibility boundaries around full typed join build/probe/output.
use std::mem::size_of;

use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::executor::typed_batch::{join::HashJoin, TypedBatch};
use crate::types::{DataType, Value};

use super::{
    check_timeout, BatchRow, JoinKind, JoinResult, JoinRetentionContext, JoinRetentionPhase,
    JoinRows, PendingJoinDiagnostic, QueryError, SourceExecutionEnv, VectorizedJoinSpec,
};

pub(super) fn try_execute(
    env: &SourceExecutionEnv<'_>,
    spec: VectorizedJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
    batch_size: usize,
) -> Result<Option<JoinResult>, QueryError> {
    let Some((left_key, right_key)) = eligible(&spec) else {
        return Ok(None);
    };
    let Some(left_types) =
        transport_types(env, spec.left_rows, spec.sources.map(|(left, _)| left))?
    else {
        return Ok(None);
    };
    let Some(right_types) =
        transport_types(env, spec.right_rows, spec.sources.map(|(_, right)| right))?
    else {
        return Ok(None);
    };
    if crate::executor::typed_batch::capability::join_keys(
        &left_types.types[left_key],
        &right_types.types[right_key],
    ) != crate::executor::typed_batch::capability::Capability::NativeTyped
    {
        return Ok(None);
    }
    // Eligibility is decided before conversion; admitted native failures are terminal.
    let left = from_rows(env, spec.left_rows, &left_types.types)?;
    let right = from_rows(env, spec.right_rows, &right_types.types)?;
    let mut kernel = HashJoin::new(
        env.controls,
        &left,
        &right,
        (left_key, right_key),
        matches!(spec.kind, JoinKind::Left),
        batch_size,
        |_| retention.before(JoinRetentionPhase::HashBuild),
    )?;
    let mut joined = JoinRows::try_new(env.controls)?;
    let budget = spec.row_budget.unwrap_or(usize::MAX);
    let mut batches = 0;
    let mut matched_rows = 0;
    let mut probe_rows = 0;
    while let Some(output) = kernel.next_batch(budget.saturating_sub(joined.len()))? {
        check_timeout(env.controls)?;
        batches += 1;
        for lane in 0..output.batch.len() {
            check_timeout(env.controls)?;
            let left_row = &spec.left_rows[output.left[lane]];
            let right_row =
                output.right[lane].map_or(spec.right_template, |index| &spec.right_rows[index]);
            joined.try_push_combined(left_row, right_row, || {
                retention.before(JoinRetentionPhase::HashOutput)?;
                check_timeout(env.controls)?;
                row_from_output(&output.batch, lane, left_row, right_row).map(Some)
            })?;
            matched_rows += usize::from(output.right[lane].is_some());
            probe_rows = probe_rows.max(output.left[lane] + 1);
        }
    }
    // Preserve actual input/probe accounting even when the final lanes have no matches.
    if joined.len() < budget {
        probe_rows = spec.left_rows.len();
    }
    Ok(Some(JoinResult::new(
        joined,
        PendingJoinDiagnostic::Typed {
            input_rows: crate::runtime::VectorizedJoinInputRows {
                left: spec.left_rows.len(),
                right: spec.right_rows.len(),
                build: kernel.build_rows(),
                probe: probe_rows,
            },
            matched_rows,
            batch_size,
            batches,
        },
    )))
}

fn eligible(spec: &VectorizedJoinSpec<'_>) -> Option<(usize, usize)> {
    let left_key = key_index(spec.left_rows.first()?, &spec.keys.left)?;
    let right_key = key_index(spec.right_rows.first()?, &spec.keys.right)?;
    if !homogeneous(spec.left_rows) || !homogeneous(spec.right_rows) {
        return None;
    }
    if matches!(spec.kind, JoinKind::Left)
        && (spec.right_rows[0]
            .entries()
            .iter()
            .map(|(name, _)| name)
            .ne(spec.right_template.entries().iter().map(|(name, _)| name))
            || spec.right_rows[0].aliases() != spec.right_template.aliases())
    {
        return None;
    }
    Some((left_key, right_key))
}

fn key_index(row: &BatchRow, key: &str) -> Option<usize> {
    let candidates = crate::sql::ColumnIdentifierPath::parse(key)
        .ok()?
        .row_lookup_candidates();
    for candidate in candidates {
        if let Some(index) = row
            .entries()
            .iter()
            .position(|(name, _)| name == &candidate)
        {
            return Some(index);
        }
        if let Some((_, index)) = row.aliases().iter().find(|(name, _)| name == &candidate) {
            return Some(*index);
        }
    }
    None
}

fn homogeneous(rows: &[BatchRow]) -> bool {
    let Some(first) = rows.first() else {
        return false;
    };
    rows.iter().all(|row| {
        !row.has_outer_scope()
            && row.entries().len() == first.entries().len()
            && row
                .entries()
                .iter()
                .zip(first.entries())
                .all(|((name, _), (first, _))| name == first)
            && row.aliases() == first.aliases()
    })
}

fn from_rows(
    env: &SourceExecutionEnv<'_>,
    rows: &[BatchRow],
    types: &[DataType],
) -> Result<TypedBatch, QueryError> {
    let first = &rows[0];
    let width = first.entries().len();
    let mut bytes = add(
        mul(width, size_of::<Vec<Value>>())?,
        mul(mul(width, rows.len())?, size_of::<Value>())?,
    )?;
    bytes = add(bytes, mul(width, size_of::<(String, DataType)>())?)?;
    for (name, data_type) in first.entries().iter().map(|(name, _)| name).zip(types) {
        bytes = add(
            bytes,
            add(
                name.len(),
                crate::executor::retained_memory::data_type_clone_bytes(data_type)?,
            )?,
        )?;
    }
    for row in rows {
        check_timeout(env.controls)?;
        for (_, value) in row.entries() {
            bytes = add(bytes, value_clone_bytes(value)?)?;
        }
    }
    let _memory = env.controls.reserve_query_memory(bytes)?;
    let schema = first
        .entries()
        .iter()
        .zip(types)
        .map(|((name, _), t)| (name.clone(), t.clone()))
        .collect::<Vec<_>>();
    let mut values = (0..width)
        .map(|_| Vec::with_capacity(rows.len()))
        .collect::<Vec<_>>();
    for row in rows {
        check_timeout(env.controls)?;
        for (column, (_, value)) in values.iter_mut().zip(row.entries()) {
            column.push(value.clone());
        }
    }
    TypedBatch::from_columns(env.controls, &schema, &values, rows.len(), None)
}

fn row_from_output(
    batch: &TypedBatch,
    lane: usize,
    left: &BatchRow,
    right: &BatchRow,
) -> Result<BatchRow, QueryError> {
    let mut values = Vec::with_capacity(batch.schema().len());
    for (column, (name, _)) in batch.schema().iter().enumerate() {
        values.push((name.clone(), batch.cell(column, lane)?.to_owned()));
    }
    let width = left.entries().len();
    let mut aliases = Vec::with_capacity(left.aliases().len() + right.aliases().len());
    aliases.extend(left.aliases().iter().cloned());
    aliases.extend(
        right
            .aliases()
            .iter()
            .map(|(name, index)| (name.clone(), width + index)),
    );
    let data_types = (!left.data_types().is_empty() || !right.data_types().is_empty()).then(|| {
        let mut types = Vec::with_capacity(batch.schema().len());
        for row in [left, right] {
            for index in 0..row.entries().len() {
                types.push(
                    row.data_types()
                        .get(index)
                        .cloned()
                        .unwrap_or(DataType::Null),
                );
            }
        }
        types.into()
    });
    Ok(BatchRow::with_aliases(values, aliases).with_optional_data_types(data_types))
}

struct TransportTypes {
    types: Vec<DataType>,
    _memory: crate::runtime::QueryMemoryReservation,
}

fn transport_types(
    env: &SourceExecutionEnv<'_>,
    rows: &[BatchRow],
    source: Option<&super::QuerySource>,
) -> Result<Option<TransportTypes>, QueryError> {
    let first = &rows[0];
    let source = match source {
        Some(super::QuerySource::Collection(name)) => Some(name),
        None => None,
        _ => return Ok(None),
    };
    let (schema, _schema_memory) = if let Some(name) = source {
        match env
            .cassie
            .catalog
            .clone_schema_with_controls(name, env.controls)
        {
            Ok(schema) => schema,
            Err(crate::app::CassieError::ResourceLimit(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    } else {
        (None, None)
    };
    if schema.is_none() && first.data_types().len() != first.entries().len() {
        return Ok(None);
    }
    let resolve = |index: usize, name: &str| -> Option<&DataType> {
        if let Some(schema) = &schema {
            schema
                .fields
                .iter()
                .find(|field| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(name, &field.name)
                })
                .map(|field| &field.data_type)
                .or_else(|| {
                    crate::types::row_identity::is_identity_reference(name, schema.declares_id())
                        .then_some(&DataType::Text)
                })
        } else {
            first.data_types().get(index)
        }
    };
    let mut bytes = mul(first.entries().len(), size_of::<DataType>())?;
    for (index, (name, _)) in first.entries().iter().enumerate() {
        check_timeout(env.controls)?;
        let Some(data_type) = resolve(index, name) else {
            return Ok(None);
        };
        bytes = add(
            bytes,
            crate::executor::retained_memory::data_type_clone_bytes(data_type)?,
        )?;
    }
    let memory = match env.controls.reserve_query_memory(bytes) {
        Ok(memory) => memory,
        Err(crate::app::CassieError::ResourceLimit(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let types = first
        .entries()
        .iter()
        .enumerate()
        .map(|(index, (name, _))| {
            resolve(index, name)
                .expect("validated transport type")
                .clone()
        })
        .collect();
    Ok(Some(TransportTypes {
        types,
        _memory: memory,
    }))
}
