use super::{
    accounting, check_timeout, combine_rows, filter, row_join_key, BatchRow, EquiJoinKeys,
    JoinKind, JoinResult, JoinRetentionContext, JoinRetentionPhase, JoinRows, JoinRowsSpec,
    PendingJoinDiagnostic, QueryError, SourceExecutionEnv,
};
use crate::executor::semantic::SemanticKey;

struct KeyedRow {
    key: Option<SemanticKey>,
    row: BatchRow,
}

pub(super) fn execute_merge_join_with_context(
    env: &SourceExecutionEnv<'_>,
    keys: &EquiJoinKeys,
    spec: JoinRowsSpec<'_>,
    keyed_probe: impl FnOnce(),
    retention: &JoinRetentionContext<'_>,
) -> Result<JoinResult, QueryError> {
    let _keyed_memory = env
        .controls
        .reserve_query_memory(accounting::keyed_rows_bytes::<KeyedRow>(
            spec.left_rows,
            &keys.left,
            spec.right_rows,
            &keys.right,
        )?)?;
    let mut left_keyed = keyed_rows(spec.left_rows, &keys.left, retention)?;
    let mut right_keyed = keyed_rows(spec.right_rows, &keys.right, retention)?;
    keyed_probe();
    check_timeout(env.controls)?;
    // Preserve stable equal-key order; admission includes the native sort scratch.
    left_keyed.sort_by(|left, right| left.key.cmp(&right.key));
    right_keyed.sort_by(|left, right| left.key.cmp(&right.key));
    check_timeout(env.controls)?;
    let mut state = MergeJoinState {
        joined: JoinRows::try_new(env.controls)?,
        matched_rows: 0,
        left_index: 0,
        right_index: 0,
        output_budget: spec.row_budget.unwrap_or(usize::MAX),
    };
    state.join_keyed_rows(env, spec, &left_keyed, &right_keyed, retention)?;
    append_unmatched(
        env,
        &mut state.joined,
        spec,
        &UnmatchedRows::Left(&left_keyed[state.left_index..]),
        state.output_budget,
        retention,
    )?;
    append_unmatched(
        env,
        &mut state.joined,
        spec,
        &UnmatchedRows::Right(&right_keyed[state.right_index..]),
        state.output_budget,
        retention,
    )?;
    Ok(JoinResult::new(
        state.joined,
        PendingJoinDiagnostic::Scalar {
            operator: "merge",
            left_rows: spec.left_rows.len(),
            right_rows: spec.right_rows.len(),
            matched_rows: state.matched_rows,
        },
    ))
}

struct MergeJoinState {
    joined: JoinRows,
    matched_rows: usize,
    left_index: usize,
    right_index: usize,
    output_budget: usize,
}

impl MergeJoinState {
    fn join_keyed_rows(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        spec: JoinRowsSpec<'_>,
        left: &[KeyedRow],
        right: &[KeyedRow],
        retention: &JoinRetentionContext<'_>,
    ) -> Result<(), QueryError> {
        while self.joined.len() < self.output_budget
            && self.left_index < left.len()
            && self.right_index < right.len()
        {
            check_timeout(env.controls)?;
            let left_end = keyed_group_end(left, self.left_index);
            let right_end = keyed_group_end(right, self.right_index);
            match left[self.left_index].key.cmp(&right[self.right_index].key) {
                std::cmp::Ordering::Less => {
                    append_unmatched(
                        env,
                        &mut self.joined,
                        spec,
                        &UnmatchedRows::Left(&left[self.left_index..left_end]),
                        self.output_budget,
                        retention,
                    )?;
                    self.left_index = left_end;
                }
                std::cmp::Ordering::Greater => {
                    append_unmatched(
                        env,
                        &mut self.joined,
                        spec,
                        &UnmatchedRows::Right(&right[self.right_index..right_end]),
                        self.output_budget,
                        retention,
                    )?;
                    self.right_index = right_end;
                }
                std::cmp::Ordering::Equal => {
                    let left_group = &left[self.left_index..left_end];
                    let right_group = &right[self.right_index..right_end];
                    if left[self.left_index].key.is_some() {
                        self.matched_rows += merge_equal_key_groups(
                            env,
                            &mut self.joined,
                            &MergeGroupsSpec {
                                rows: spec,
                                left: left_group,
                                right: right_group,
                                output_budget: self.output_budget,
                            },
                            retention,
                        )?;
                    } else {
                        append_unmatched(
                            env,
                            &mut self.joined,
                            spec,
                            &UnmatchedRows::Left(left_group),
                            self.output_budget,
                            retention,
                        )?;
                        append_unmatched(
                            env,
                            &mut self.joined,
                            spec,
                            &UnmatchedRows::Right(right_group),
                            self.output_budget,
                            retention,
                        )?;
                    }
                    self.left_index = left_end;
                    self.right_index = right_end;
                }
            }
        }
        Ok(())
    }
}

fn keyed_rows(
    rows: &[BatchRow],
    key_column: &str,
    retention: &JoinRetentionContext<'_>,
) -> Result<Vec<KeyedRow>, QueryError> {
    let mut keyed = Vec::new();
    keyed
        .try_reserve_exact(rows.len())
        .map_err(|error| allocation_error(&error))?;
    for row in rows {
        retention.before(JoinRetentionPhase::MergeKeyed)?;
        let row = row.clone();
        let key = row_join_key(&row, key_column);
        keyed.push(KeyedRow { key, row });
    }
    Ok(keyed)
}

fn keyed_group_end(rows: &[KeyedRow], start: usize) -> usize {
    let key = &rows[start].key;
    let mut end = start + 1;
    while end < rows.len() && &rows[end].key == key {
        end += 1;
    }
    end
}

struct MergeGroupsSpec<'a> {
    rows: JoinRowsSpec<'a>,
    left: &'a [KeyedRow],
    right: &'a [KeyedRow],
    output_budget: usize,
}

fn merge_equal_key_groups(
    env: &SourceExecutionEnv<'_>,
    joined: &mut JoinRows,
    spec: &MergeGroupsSpec<'_>,
    retention: &JoinRetentionContext<'_>,
) -> Result<usize, QueryError> {
    let bitmap_bytes = crate::executor::retained_memory::add(spec.left.len(), spec.right.len())?;
    let _bitmap_memory = env.controls.reserve_query_memory(bitmap_bytes)?;
    let mut left_matched = vec![false; spec.left.len()];
    let mut right_matched = vec![false; spec.right.len()];
    let mut matched_rows = 0;
    'left: for (left_index, left_row) in spec.left.iter().enumerate() {
        for (right_index, right_row) in spec.right.iter().enumerate() {
            check_timeout(env.controls)?;
            let accepted = joined.try_push_combined(&left_row.row, &right_row.row, || {
                retention.before(JoinRetentionPhase::MergeOutput)?;
                check_timeout(env.controls)?;
                let combined = combine_rows(&left_row.row, &right_row.row);
                let passes = filter::eval_scalar(
                    &combined,
                    spec.rows.on,
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
                left_matched[left_index] = true;
                right_matched[right_index] = true;
                matched_rows += 1;
                if joined.len() >= spec.output_budget {
                    break 'left;
                }
            }
        }
    }
    if matches!(spec.rows.kind, JoinKind::Left | JoinKind::Full) {
        for (index, row) in spec.left.iter().enumerate() {
            if !left_matched[index] && joined.len() < spec.output_budget {
                append_unmatched(
                    env,
                    joined,
                    spec.rows,
                    &UnmatchedRows::Left(std::slice::from_ref(row)),
                    spec.output_budget,
                    retention,
                )?;
            }
        }
    }
    if matches!(spec.rows.kind, JoinKind::Right | JoinKind::Full) {
        for (index, row) in spec.right.iter().enumerate() {
            if !right_matched[index] && joined.len() < spec.output_budget {
                append_unmatched(
                    env,
                    joined,
                    spec.rows,
                    &UnmatchedRows::Right(std::slice::from_ref(row)),
                    spec.output_budget,
                    retention,
                )?;
            }
        }
    }
    Ok(matched_rows)
}

enum UnmatchedRows<'a> {
    Left(&'a [KeyedRow]),
    Right(&'a [KeyedRow]),
}

fn append_unmatched(
    env: &SourceExecutionEnv<'_>,
    joined: &mut JoinRows,
    spec: JoinRowsSpec<'_>,
    rows: &UnmatchedRows<'_>,
    output_budget: usize,
    retention: &JoinRetentionContext<'_>,
) -> Result<(), QueryError> {
    let (rows, is_left) = match rows {
        UnmatchedRows::Left(rows) if matches!(spec.kind, JoinKind::Left | JoinKind::Full) => {
            (*rows, true)
        }
        UnmatchedRows::Right(rows) if matches!(spec.kind, JoinKind::Right | JoinKind::Full) => {
            (*rows, false)
        }
        _ => return Ok(()),
    };
    for row in rows {
        if joined.len() >= output_budget {
            break;
        }
        let (left, right) = if is_left {
            (&row.row, spec.right_template)
        } else {
            (spec.left_template, &row.row)
        };
        joined.try_push_combined(left, right, || {
            retention.before(JoinRetentionPhase::MergeOutput)?;
            check_timeout(env.controls)?;
            Ok(Some(combine_rows(left, right)))
        })?;
    }
    Ok(())
}

fn allocation_error(error: &std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain merge join state: {error}"))
        .into()
}
