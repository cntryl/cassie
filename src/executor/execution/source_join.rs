use super::{
    batch, catalog, check_timeout, combine_rows, execute_query_source, filter, qualify_row,
    row_lookup_columns, scan, source_contains_lateral, Batch, BatchRow, BinaryOp, CteContext, Expr,
    JoinKind, QueryError, QuerySource, SourceExecution, SourceExecutionEnv, Value,
};
use crate::executor::semantic::SemanticKey;

const VECTOR_TO_MERGE_SWITCH_PAIR: &str = "vectorized_join_to_merge_join";

#[path = "source_join/bounded.rs"]
mod bounded;

#[path = "source_join/merge.rs"]
mod merge;

#[path = "source_join/accounting.rs"]
mod accounting;

#[path = "source_join/kernels.rs"]
mod kernels;

#[path = "source_join/typed.rs"]
mod typed;

#[path = "source_join/output.rs"]
mod output;

pub(super) use accounting::combined_row_bytes;
use accounting::JoinRows;
use kernels::{execute_nested_loop_join, execute_vectorized_join};
use output::finish_retained_join;

#[path = "source_join/retention.rs"]
mod retention;

use retention::{JoinResult, JoinRetentionContext, JoinRetentionPhase, PendingJoinDiagnostic};

#[cfg(test)]
#[path = "source_join/active_controls_tests.rs"]
mod active_controls_tests;

#[cfg(test)]
#[path = "source_join/retention_tests.rs"]
mod retention_tests;

#[cfg(test)]
#[path = "source_join/handoff_tests.rs"]
mod handoff_tests;

#[cfg(test)]
#[path = "source_join/retained_output_tests.rs"]
mod retained_output_tests;

#[cfg(test)]
#[path = "source_join/spill_fallback_tests.rs"]
mod spill_fallback_tests;

#[derive(Debug, Clone)]
struct EquiJoinKeys {
    left: String,
    right: String,
}

#[derive(Clone, Copy)]
pub(super) struct JoinExecutionSpec<'a> {
    pub(super) left: &'a QuerySource,
    pub(super) right: &'a QuerySource,
    pub(super) kind: JoinKind,
    pub(super) on: &'a Expr,
    pub(super) outer_row: Option<&'a BatchRow>,
    pub(super) row_budget: Option<usize>,
}

pub(super) fn execute_join_source<'a>(
    env: &'a SourceExecutionEnv<'a>,
    spec: JoinExecutionSpec<'a>,
    cte_context: &'a mut CteContext,
) -> SourceExecution {
    if spec.row_budget == Some(0) {
        return finish_retained_join(env, JoinResult::empty(env)?);
    }

    if !source_contains_lateral(spec.right) {
        if let Some(joined) = bounded::try_execute_indexed_bounded_inner_join(env, &spec)? {
            return finish_retained_join(env, joined);
        }
        if let Some(joined) =
            bounded::try_execute_streaming_bounded_inner_join(env, &spec, cte_context)?
        {
            return finish_retained_join(env, joined);
        }
    }

    let left_row_budget = if matches!(spec.kind, JoinKind::Left | JoinKind::Cross) {
        spec.row_budget
    } else {
        None
    };
    let (left_batches, _left_text) = execute_query_source(
        env,
        spec.left,
        cte_context,
        true,
        spec.outer_row,
        left_row_budget,
    )?;
    if source_contains_lateral(spec.right) {
        return execute_lateral_join(env, &spec, cte_context, left_batches);
    }
    let (left_rows, _left_memory) = prepare_join_rows(env, left_batches)?;

    let right_row_budget = matches!(spec.kind, JoinKind::Cross)
        .then_some(spec.row_budget)
        .flatten();
    let (right_batches, _right_text) = execute_query_source(
        env,
        spec.right,
        cte_context,
        true,
        spec.outer_row,
        right_row_budget,
    )?;
    let (right_rows, _right_memory) = prepare_join_rows(env, right_batches)?;
    let left_template = super::source_shape::null_row(env, spec.left, cte_context)?;
    let right_template = super::source_shape::null_row(env, spec.right, cte_context)?;
    let left_lookup_columns = row_lookup_columns(std::slice::from_ref(&left_template));
    let right_lookup_columns = row_lookup_columns(std::slice::from_ref(&right_template));

    let joined = execute_loaded_join(
        env,
        JoinRowsSpec {
            kind: spec.kind,
            sources: Some((spec.left, spec.right)),
            on: spec.on,
            left_rows: &left_rows,
            right_rows: &right_rows,
            left_template: &left_template,
            right_template: &right_template,
            row_budget: spec.row_budget,
        },
        &left_lookup_columns,
        &right_lookup_columns,
    )?;
    finish_retained_join(env, joined)
}

fn execute_loaded_join(
    env: &SourceExecutionEnv<'_>,
    spec: JoinRowsSpec<'_>,
    left_lookup_columns: &[String],
    right_lookup_columns: &[String],
) -> Result<JoinResult, QueryError> {
    execute_loaded_join_with_replacement_probe(
        env,
        spec,
        left_lookup_columns,
        right_lookup_columns,
        || {},
    )
}

fn execute_loaded_join_with_replacement_probe(
    env: &SourceExecutionEnv<'_>,
    spec: JoinRowsSpec<'_>,
    left_lookup_columns: &[String],
    right_lookup_columns: &[String],
    replacement_probe: impl FnOnce(),
) -> Result<JoinResult, QueryError> {
    execute_loaded_join_with_context(
        env,
        spec,
        left_lookup_columns,
        right_lookup_columns,
        replacement_probe,
        &JoinRetentionContext::default(),
    )
}

fn execute_loaded_join_with_context(
    env: &SourceExecutionEnv<'_>,
    spec: JoinRowsSpec<'_>,
    left_lookup_columns: &[String],
    right_lookup_columns: &[String],
    replacement_probe: impl FnOnce(),
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let joined = match merge_join_keys(spec.on, left_lookup_columns, right_lookup_columns)
        .filter(|_| !matches!(spec.kind, JoinKind::Cross))
    {
        Some(keys) => {
            match execute_vectorized_join(
                env,
                VectorizedJoinSpec {
                    kind: spec.kind,
                    sources: spec.sources,
                    keys: &keys,
                    left_rows: spec.left_rows,
                    right_rows: spec.right_rows,
                    right_template: spec.right_template,
                    row_budget: spec.row_budget,
                },
                retention,
            )? {
                VectorizedJoinOutcome::Executed(rows) => rows,
                VectorizedJoinOutcome::Fallback => {
                    merge::execute_merge_join_with_context(env, &keys, spec, || {}, retention)?
                }
                VectorizedJoinOutcome::SwitchToMerge(state) => {
                    match merge::execute_merge_join_with_context(
                        env,
                        &keys,
                        spec,
                        replacement_probe,
                        retention,
                    ) {
                        Ok(rows) => rows.with_switch(state),
                        Err(error) => {
                            env.cassie.runtime.record_runtime_operator_switch_fallback(
                                VECTOR_TO_MERGE_SWITCH_PAIR,
                                "replacement_failed",
                                &state,
                            );
                            return Err(error);
                        }
                    }
                }
            }
        }
        None => execute_nested_loop_join(env, spec, retention)?,
    };
    Ok(joined)
}

fn execute_lateral_join<'a>(
    env: &'a SourceExecutionEnv<'a>,
    spec: &'a JoinExecutionSpec<'a>,
    cte_context: &'a mut CteContext,
    left_batches: Vec<Batch>,
) -> SourceExecution {
    let (left_rows, _left_memory) = prepare_join_rows(env, left_batches)?;
    let mut joined = JoinRows::try_new(env.controls)?;
    let retention = JoinRetentionContext::default();
    let mut matched_rows = 0usize;
    let output_budget = spec.row_budget.unwrap_or(usize::MAX);
    let right_template = super::source_shape::null_row(env, spec.right, cte_context)?;

    'left: for left_row in &left_rows {
        check_timeout(env.controls)?;
        let right_budget = matches!(spec.kind, JoinKind::Cross)
            .then_some(output_budget.saturating_sub(joined.len()));
        let (right_batches, _right_text) = execute_query_source(
            env,
            spec.right,
            cte_context,
            true,
            Some(left_row),
            right_budget,
        )?;
        let (right_rows, _right_memory) = prepare_join_rows(env, right_batches)?;
        let mut matched = false;
        for right_row in &right_rows {
            check_timeout(env.controls)?;
            let accepted = joined.try_push_combined(left_row, right_row, || {
                retention.before(JoinRetentionPhase::NestedOutput)?;
                check_timeout(env.controls)?;
                let combined = combine_rows(left_row, right_row)?;
                let passes = matches!(spec.kind, JoinKind::Cross)
                    || filter::eval_scalar(
                        &combined,
                        spec.on,
                        env.params,
                        None,
                        env.user_functions,
                        None,
                        env.session,
                    )?
                    .is_true()?;
                Ok(passes.then_some(combined))
            })?;
            if accepted {
                matched = true;
                matched_rows += 1;
                if joined.len() >= output_budget {
                    break 'left;
                }
            }
        }

        if !matched && matches!(spec.kind, JoinKind::Left | JoinKind::Full) {
            joined.try_push_combined(left_row, &right_template, || {
                check_timeout(env.controls)?;
                Ok(Some(combine_rows(left_row, &right_template)?))
            })?;
            if joined.len() >= output_budget {
                break;
            }
        }
    }

    finish_retained_join(
        env,
        JoinResult::new(
            joined,
            PendingJoinDiagnostic::Scalar {
                operator: "nested_loop",
                left_rows: left_rows.len(),
                right_rows: 0,
                matched_rows,
            },
        ),
    )
}

#[derive(Clone, Copy)]
struct JoinRowsSpec<'a> {
    kind: JoinKind,
    sources: Option<(&'a QuerySource, &'a QuerySource)>,
    on: &'a Expr,
    left_rows: &'a [BatchRow],
    right_rows: &'a [BatchRow],
    left_template: &'a BatchRow,
    right_template: &'a BatchRow,
    row_budget: Option<usize>,
}

fn collection_join_columns(env: &SourceExecutionEnv<'_>, collection: &str) -> Option<Vec<String>> {
    let mut columns = vec![crate::types::row_identity::LEGACY_ID_COLUMN.to_string()];
    columns.extend(collection_scan_fields(env, collection)?);
    Some(qualify_column_names(columns, collection))
}

fn collection_scan_fields(env: &SourceExecutionEnv<'_>, collection: &str) -> Option<Vec<String>> {
    Some(
        env.cassie
            .catalog
            .get_schema(collection)?
            .fields
            .into_iter()
            .map(|field| field.name)
            .collect(),
    )
}

fn qualify_column_names(columns: Vec<String>, qualifier: &str) -> Vec<String> {
    let qualifiers = crate::catalog::qualifier_variants(qualifier);
    let mut out = Vec::with_capacity(columns.len() * (qualifiers.len() + 1));
    for column in columns {
        out.push(column.clone());
        for qualifier in &qualifiers {
            out.push(format!("{qualifier}.{column}"));
        }
    }
    out
}

fn join_field_for_collection(key: &str, collection: &str) -> Option<String> {
    let key_lower = key.to_ascii_lowercase();
    for qualifier in crate::catalog::qualifier_variants(collection) {
        let prefix = format!("{qualifier}.");
        if key_lower.starts_with(&prefix) {
            return Some(key[prefix.len()..].to_string());
        }
    }
    (!key.contains('.')).then(|| key.to_string())
}

#[derive(Clone, Copy)]
struct VectorizedJoinSpec<'a> {
    kind: JoinKind,
    sources: Option<(&'a QuerySource, &'a QuerySource)>,
    keys: &'a EquiJoinKeys,
    left_rows: &'a [BatchRow],
    right_rows: &'a [BatchRow],
    right_template: &'a BatchRow,
    row_budget: Option<usize>,
}

enum VectorizedJoinOutcome {
    Executed(JoinResult),
    Fallback,
    SwitchToMerge(String),
}

enum VectorizedJoinSelection {
    Execute(usize),
    Fallback,
    SwitchToMerge(String),
}

fn row_join_key(row: &BatchRow, key_column: &str) -> Option<SemanticKey> {
    row.get(key_column)
        .filter(|value| !matches!(value, Value::Null))
        .map(SemanticKey::single)
}

fn estimate_vectorized_join_bytes(left_rows: usize, right_rows: usize) -> usize {
    left_rows
        .saturating_add(right_rows)
        .saturating_mul(std::mem::size_of::<BatchRow>().max(512))
}

/// Admit both the complete input bodies and old/new row-slot overlap before flattening.
fn prepare_join_rows(
    env: &SourceExecutionEnv<'_>,
    batches: Vec<Batch>,
) -> Result<(Vec<BatchRow>, crate::runtime::QueryMemoryReservation), QueryError> {
    use crate::executor::retained_memory::{add, mul};
    use std::mem::size_of;

    check_timeout(env.controls)?;
    let mut row_count = 0;
    let mut old_buffers = mul(batches.capacity(), size_of::<Batch>())?;
    for batch in &batches {
        row_count = add(row_count, batch.len())?;
        old_buffers = add(old_buffers, mul(batch.capacity(), size_of::<BatchRow>())?)?;
    }
    // Keep complete-body admission even when a row carries an origin Arc: a derived
    // clone can share that lease while owning additional deep-cloned row buffers.
    let mut memory = reserve_join_rows(env, batches.iter().flatten(), std::iter::empty())?;
    let retained_bytes = memory.bytes();
    memory.try_grow(old_buffers)?;
    check_timeout(env.controls)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(row_count).map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to retain loaded join inputs: {error}"
        ))
    })?;
    for batch in batches {
        for row in batch {
            check_timeout(env.controls)?;
            rows.push(row);
        }
    }
    // The old outer/batch Vecs have been freed; the body estimate already includes
    // one slot for every row in the exact-capacity flattened buffer that survives.
    memory.shrink_to(retained_bytes);
    check_timeout(env.controls)?;
    Ok((rows, memory))
}

fn reserve_join_rows<'a>(
    env: &SourceExecutionEnv<'_>,
    left: impl IntoIterator<Item = &'a BatchRow>,
    right: impl IntoIterator<Item = &'a BatchRow>,
) -> Result<crate::runtime::QueryMemoryReservation, QueryError> {
    let bytes =
        left.into_iter()
            .chain(right)
            .try_fold(0, |bytes, row| -> Result<usize, QueryError> {
                check_timeout(env.controls)?;
                Ok(crate::executor::retained_memory::add(
                    bytes,
                    accounting::moved_row_bytes(row)?,
                )?)
            })?;
    env.controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

fn merge_join_keys(
    on: &Expr,
    left_columns: &[String],
    right_columns: &[String],
) -> Option<EquiJoinKeys> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = on
    else {
        return None;
    };
    let (Expr::Column(left_name), Expr::Column(right_name)) = (left.as_ref(), right.as_ref())
    else {
        return None;
    };

    if column_belongs_to(left_name, left_columns, right_columns)
        && column_belongs_to(right_name, right_columns, left_columns)
    {
        return Some(EquiJoinKeys {
            left: left_name.clone(),
            right: right_name.clone(),
        });
    }

    if column_belongs_to(right_name, left_columns, right_columns)
        && column_belongs_to(left_name, right_columns, left_columns)
    {
        return Some(EquiJoinKeys {
            left: right_name.clone(),
            right: left_name.clone(),
        });
    }

    None
}

fn column_belongs_to(name: &str, own_columns: &[String], other_columns: &[String]) -> bool {
    let Ok(reference) = crate::sql::ColumnIdentifierPath::parse(name) else {
        return false;
    };
    let candidates = reference.row_lookup_candidates();
    let count = candidates
        .len()
        .saturating_sub(usize::from(reference.is_qualified()));
    let candidates = if reference.is_qualified() {
        &candidates[..count]
    } else {
        &candidates[..]
    };
    for candidate in candidates {
        let own = own_columns.iter().any(|column| column == candidate);
        let other = other_columns.iter().any(|column| column == candidate);
        if own || other {
            return own && (!other || reference.is_qualified());
        }
    }
    false
}

fn vectorized_join_selection(
    env: &SourceExecutionEnv<'_>,
    kind: JoinKind,
    left_rows: &[BatchRow],
    right_rows: &[BatchRow],
) -> Result<VectorizedJoinSelection, QueryError> {
    let limits = env.cassie.runtime.limits();
    let batch_size = limits.vectorized_join_batch_size.max(1);
    if !limits.vectorized_joins_enabled {
        return Ok(VectorizedJoinSelection::Fallback);
    }
    if !matches!(kind, JoinKind::Inner | JoinKind::Left) {
        if limits.operator_switching_enabled.is_enabled() {
            env.cassie.runtime.record_runtime_operator_switch_skip(
                VECTOR_TO_MERGE_SWITCH_PAIR,
                "unsupported_join_type",
                "rows_emitted=0",
            );
        }
        env.cassie.runtime.record_vectorized_join_fallback(
            "unsupported_join_type",
            batch_size,
            false,
        );
        return Ok(VectorizedJoinSelection::Fallback);
    }

    let observed_rows = left_rows.len().saturating_add(right_rows.len());
    if limits.operator_switching_enabled.is_enabled()
        && observed_rows > limits.operator_switch_join_row_threshold
    {
        check_timeout(env.controls)?;
        let state = format!(
            "replay_left_rows={};replay_right_rows={};rows_emitted=0",
            left_rows.len(),
            right_rows.len()
        );
        return Ok(VectorizedJoinSelection::SwitchToMerge(state));
    }

    let estimated_bytes = estimate_vectorized_join_bytes(left_rows.len(), right_rows.len());
    if estimated_bytes > env.controls.query_memory_budget_bytes {
        if limits.operator_switching_enabled.is_enabled() {
            env.cassie.runtime.record_runtime_operator_switch_fallback(
                VECTOR_TO_MERGE_SWITCH_PAIR,
                "spill_budget_exceeded",
                "rows_emitted=0",
            );
        }
        env.cassie.runtime.record_vectorized_join_fallback(
            "spill_budget_exceeded",
            batch_size,
            true,
        );
        return Ok(VectorizedJoinSelection::Fallback);
    }

    Ok(VectorizedJoinSelection::Execute(batch_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_preserve_schema_qualified_key_orientation_with_shared_suffix_aliases() {
        // Arrange
        let on = Expr::Binary {
            left: Box::new(Expr::Column("b.records.x".to_string())),
            op: BinaryOp::Eq,
            right: Box::new(Expr::Column("a.records.y".to_string())),
        };
        let left = ["a.records.x", "a.records.y", "records.x", "records.y"].map(str::to_string);
        let right = ["b.records.x", "b.records.y", "records.x", "records.y"].map(str::to_string);

        // Act
        let keys = merge_join_keys(&on, &left, &right).expect("qualified join keys");

        // Assert
        assert_eq!(keys.left, "a.records.y");
        assert_eq!(keys.right, "b.records.x");
    }

    #[test]
    fn should_qualify_join_columns_with_suffix_variants() {
        // Arrange
        let columns = vec!["user_key".to_string()];

        // Act
        let qualified = qualify_column_names(columns, "postgres.public.users");

        // Assert
        assert!(qualified.contains(&"user_key".to_string()));
        assert!(qualified.contains(&"users.user_key".to_string()));
        assert!(qualified.contains(&"public.users.user_key".to_string()));
        assert!(qualified.contains(&"postgres.public.users.user_key".to_string()));
    }

    #[test]
    fn should_match_join_fields_for_local_qualified_names() {
        // Arrange
        let collection = "postgres.public.users";

        // Act
        let local = join_field_for_collection("users.user_key", collection);
        let schema_qualified = join_field_for_collection("public.users.user_key", collection);
        let canonical = join_field_for_collection("postgres.public.users.user_key", collection);

        // Assert
        assert_eq!(local.as_deref(), Some("user_key"));
        assert_eq!(schema_qualified.as_deref(), Some("user_key"));
        assert_eq!(canonical.as_deref(), Some("user_key"));
    }
}
