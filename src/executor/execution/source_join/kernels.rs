use super::{
    accounting, check_timeout, combine_rows, filter, row_join_key, vectorized_join_selection,
    BatchRow, JoinKind, JoinResult, JoinRetentionContext, JoinRetentionPhase, JoinRows,
    JoinRowsSpec, PendingJoinDiagnostic, QueryError, SourceExecutionEnv, VectorizedJoinOutcome,
    VectorizedJoinSelection, VectorizedJoinSpec,
};
use crate::executor::semantic::SemanticKey;

pub(super) fn execute_nested_loop_join(
    env: &SourceExecutionEnv<'_>,
    spec: JoinRowsSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let mut joined = JoinRows::try_new(env.controls)?;
    let _matched_memory = env.controls.reserve_query_memory(spec.right_rows.len())?;
    let mut right_matched = vec![false; spec.right_rows.len()];
    let mut matched_rows = 0usize;
    let output_budget = spec.row_budget.unwrap_or(usize::MAX);

    'left: for left_row in spec.left_rows {
        let mut matched = false;
        for (right_index, right_row) in spec.right_rows.iter().enumerate() {
            check_timeout(env.controls)?;
            let accepted = joined.try_push_combined(left_row, right_row, || {
                retention.before(JoinRetentionPhase::NestedOutput)?;
                check_timeout(env.controls)?;
                let combined = combine_rows(left_row, right_row);
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
                right_matched[right_index] = true;
                if joined.len() >= output_budget {
                    break 'left;
                }
            }
        }
        if !matched && matches!(spec.kind, JoinKind::Left | JoinKind::Full) {
            joined.try_push_combined(left_row, spec.right_template, || {
                retention.before(JoinRetentionPhase::NestedOutput)?;
                check_timeout(env.controls)?;
                Ok(Some(combine_rows(left_row, spec.right_template)))
            })?;
            if joined.len() >= output_budget {
                break;
            }
        }
    }
    if joined.len() < output_budget && matches!(spec.kind, JoinKind::Right | JoinKind::Full) {
        for (right_index, right_row) in spec.right_rows.iter().enumerate() {
            check_timeout(env.controls)?;
            if !right_matched[right_index] {
                joined.try_push_combined(spec.left_template, right_row, || {
                    retention.before(JoinRetentionPhase::NestedOutput)?;
                    check_timeout(env.controls)?;
                    Ok(Some(combine_rows(spec.left_template, right_row)))
                })?;
                if joined.len() >= output_budget {
                    break;
                }
            }
        }
    }
    Ok(JoinResult::new(
        joined,
        PendingJoinDiagnostic::Scalar {
            operator: if matches!(spec.kind, JoinKind::Cross) {
                "cross"
            } else {
                "nested_loop"
            },
            left_rows: spec.left_rows.len(),
            right_rows: spec.right_rows.len(),
            matched_rows,
        },
    ))
}

pub(super) fn execute_vectorized_join(
    env: &SourceExecutionEnv<'_>,
    spec: VectorizedJoinSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<VectorizedJoinOutcome, QueryError> {
    let batch_size =
        match vectorized_join_selection(env, spec.kind, spec.left_rows, spec.right_rows)? {
            VectorizedJoinSelection::Execute(batch_size) => batch_size,
            VectorizedJoinSelection::Fallback => return Ok(VectorizedJoinOutcome::Fallback),
            VectorizedJoinSelection::SwitchToMerge(state) => {
                return Ok(VectorizedJoinOutcome::SwitchToMerge(state));
            }
        };
    let output_budget = spec.row_budget.unwrap_or(usize::MAX);
    let mut joined = JoinRows::try_new(env.controls)?;
    if output_budget == 0 {
        return Ok(empty_vectorized_join(joined, batch_size));
    }
    let _build_memory = env
        .controls
        .reserve_query_memory(accounting::hash_build_bytes::<&BatchRow>(
            spec.right_rows,
            &spec.keys.right,
        )?)?;
    let mut build = std::collections::HashMap::<SemanticKey, Vec<&BatchRow>>::new();
    build
        .try_reserve(accounting::hash_build_capacity(
            spec.right_rows,
            &spec.keys.right,
        ))
        .map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to retain hash join build: {error}"
            ))
        })?;
    for right in spec.right_rows {
        check_timeout(env.controls)?;
        retention.before(JoinRetentionPhase::HashBuild)?;
        if let Some(key) = row_join_key(right, &spec.keys.right) {
            build.entry(key).or_default().push(right);
        }
    }

    let mut probe_rows = 0usize;
    let build_rows = build.values().map(Vec::len).sum::<usize>();
    let mut batches = 0usize;
    let mut matched_rows = 0usize;
    'probe: for left_batch in spec.left_rows.chunks(batch_size) {
        check_timeout(env.controls)?;
        batches += 1;
        for left in left_batch {
            check_timeout(env.controls)?;
            probe_rows += 1;
            let _probe_memory = env.controls.reserve_query_memory(
                accounting::join_key_bytes(left, &spec.keys.left)?
                    .checked_add(accounting::key_lookup_scratch_bytes(
                        std::slice::from_ref(left),
                        &spec.keys.left,
                    )?)
                    .ok_or_else(|| {
                        crate::app::CassieError::ResourceLimit(
                            "join probe accounting overflow".to_owned(),
                        )
                    })?,
            )?;
            let key = row_join_key(left, &spec.keys.left);
            let right_group = key.as_ref().and_then(|key| build.get(key));
            if let Some(right_group) = right_group {
                for right in right_group {
                    check_timeout(env.controls)?;
                    joined.try_push_combined(left, right, || {
                        retention.before(JoinRetentionPhase::HashOutput)?;
                        check_timeout(env.controls)?;
                        Ok(Some(combine_rows(left, right)))
                    })?;
                    matched_rows += 1;
                    if joined.len() >= output_budget {
                        break 'probe;
                    }
                }
            } else if matches!(spec.kind, JoinKind::Left) {
                joined.try_push_combined(left, spec.right_template, || {
                    retention.before(JoinRetentionPhase::HashOutput)?;
                    check_timeout(env.controls)?;
                    Ok(Some(combine_rows(left, spec.right_template)))
                })?;
                if joined.len() >= output_budget {
                    break 'probe;
                }
            }
        }
    }
    Ok(VectorizedJoinOutcome::Executed(JoinResult::new(
        joined,
        PendingJoinDiagnostic::Vectorized {
            probe_rows,
            build_rows,
            matched_rows,
            batch_size,
            batches,
        },
    )))
}

fn empty_vectorized_join(joined: JoinRows, batch_size: usize) -> VectorizedJoinOutcome {
    VectorizedJoinOutcome::Executed(JoinResult::new(
        joined,
        PendingJoinDiagnostic::Vectorized {
            probe_rows: 0,
            build_rows: 0,
            matched_rows: 0,
            batch_size,
            batches: 0,
        },
    ))
}
