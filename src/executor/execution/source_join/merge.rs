use super::super::{
    check_timeout, combine_nulls_with_row, combine_row_with_nulls, combine_rows, filter, BatchRow,
    Expr, JoinKind, QueryError, SourceExecutionEnv,
};
use super::{batch_row_bytes, row_join_key, EquiJoinKeys, JoinRowsSpec};
use crate::executor::semantic::SemanticKey;

#[derive(Debug)]
struct KeyedRow {
    key: Option<SemanticKey>,
    row: BatchRow,
}

pub(super) fn execute_merge_join(
    env: &SourceExecutionEnv<'_>,
    keys: &EquiJoinKeys,
    spec: JoinRowsSpec<'_>,
) -> Result<Vec<BatchRow>, QueryError> {
    let left_len = spec.left_rows.len();
    let right_len = spec.right_rows.len();
    let mut left_keyed = keyed_rows(spec.left_rows, &keys.left);
    let mut right_keyed = keyed_rows(spec.right_rows, &keys.right);
    let keyed_bytes = left_keyed
        .iter()
        .chain(&right_keyed)
        .map(|row| {
            std::mem::size_of::<KeyedRow>()
                .saturating_add(row.key.as_ref().map_or(0, SemanticKey::estimated_bytes))
                .saturating_add(batch_row_bytes(&row.row))
        })
        .sum();
    let _keyed_memory = env.controls.reserve_query_memory(keyed_bytes)?;
    left_keyed.sort_by(|left, right| left.key.cmp(&right.key));
    right_keyed.sort_by(|left, right| left.key.cmp(&right.key));

    let mut state = MergeJoinState::new(spec.row_budget);
    state.join_keyed_rows(env, spec, &left_keyed, &right_keyed)?;
    state.append_remaining(spec, &left_keyed, &right_keyed);

    env.cassie.runtime.record_join_execution(
        "merge",
        left_len,
        right_len,
        state.matched_rows,
        state.joined.len(),
        None,
    );
    Ok(state.joined)
}

struct MergeJoinState {
    joined: Vec<BatchRow>,
    matched_rows: usize,
    left_index: usize,
    right_index: usize,
    output_budget: usize,
}

impl MergeJoinState {
    fn new(row_budget: Option<usize>) -> Self {
        Self {
            joined: Vec::new(),
            matched_rows: 0,
            left_index: 0,
            right_index: 0,
            output_budget: row_budget.unwrap_or(usize::MAX),
        }
    }

    fn join_keyed_rows(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        spec: JoinRowsSpec<'_>,
        left_keyed: &[KeyedRow],
        right_keyed: &[KeyedRow],
    ) -> Result<(), QueryError> {
        while self.joined.len() < self.output_budget
            && self.left_index < left_keyed.len()
            && self.right_index < right_keyed.len()
        {
            check_timeout(env.controls)?;
            match left_keyed[self.left_index]
                .key
                .cmp(&right_keyed[self.right_index].key)
            {
                std::cmp::Ordering::Less => self.consume_left_group(spec, left_keyed),
                std::cmp::Ordering::Greater => self.consume_right_group(spec, right_keyed),
                std::cmp::Ordering::Equal => {
                    self.consume_equal_groups(env, spec, left_keyed, right_keyed)?;
                }
            }
        }
        Ok(())
    }

    fn consume_left_group(&mut self, spec: JoinRowsSpec<'_>, left_keyed: &[KeyedRow]) {
        let group_end = keyed_group_end(left_keyed, self.left_index);
        append_left_unmatched(
            &mut self.joined,
            spec.kind,
            &left_keyed[self.left_index..group_end],
            spec.right_columns,
            self.output_budget,
        );
        self.left_index = group_end;
    }

    fn consume_right_group(&mut self, spec: JoinRowsSpec<'_>, right_keyed: &[KeyedRow]) {
        let group_end = keyed_group_end(right_keyed, self.right_index);
        append_right_unmatched(
            &mut self.joined,
            spec.kind,
            spec.left_columns,
            &right_keyed[self.right_index..group_end],
            self.output_budget,
        );
        self.right_index = group_end;
    }

    fn consume_equal_groups(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        spec: JoinRowsSpec<'_>,
        left_keyed: &[KeyedRow],
        right_keyed: &[KeyedRow],
    ) -> Result<(), QueryError> {
        let left_end = keyed_group_end(left_keyed, self.left_index);
        let right_end = keyed_group_end(right_keyed, self.right_index);
        let left_group = &left_keyed[self.left_index..left_end];
        let right_group = &right_keyed[self.right_index..right_end];
        if left_keyed[self.left_index].key.is_some() {
            let result = merge_equal_key_groups(
                env,
                MergeEqualGroupsSpec {
                    kind: spec.kind,
                    on: spec.on,
                    left_columns: spec.left_columns,
                    right_columns: spec.right_columns,
                    output_budget: self.output_budget.saturating_sub(self.joined.len()),
                },
                left_group,
                right_group,
            )?;
            self.matched_rows += result.matched_rows;
            self.joined.extend(result.joined);
        } else {
            append_left_unmatched(
                &mut self.joined,
                spec.kind,
                left_group,
                spec.right_columns,
                self.output_budget,
            );
            append_right_unmatched(
                &mut self.joined,
                spec.kind,
                spec.left_columns,
                right_group,
                self.output_budget,
            );
        }
        self.left_index = left_end;
        self.right_index = right_end;
        Ok(())
    }

    fn append_remaining(
        &mut self,
        spec: JoinRowsSpec<'_>,
        left_keyed: &[KeyedRow],
        right_keyed: &[KeyedRow],
    ) {
        append_left_unmatched(
            &mut self.joined,
            spec.kind,
            &left_keyed[self.left_index..],
            spec.right_columns,
            self.output_budget,
        );
        append_right_unmatched(
            &mut self.joined,
            spec.kind,
            spec.left_columns,
            &right_keyed[self.right_index..],
            self.output_budget,
        );
    }
}

fn keyed_rows(rows: &[BatchRow], key_column: &str) -> Vec<KeyedRow> {
    rows.iter()
        .cloned()
        .map(|row| {
            let key = row_join_key(&row, key_column);
            KeyedRow { key, row }
        })
        .collect()
}

fn keyed_group_end(rows: &[KeyedRow], start: usize) -> usize {
    let key = &rows[start].key;
    let mut end = start + 1;
    while end < rows.len() && &rows[end].key == key {
        end += 1;
    }
    end
}

struct MergeEqualGroupsResult {
    joined: Vec<BatchRow>,
    matched_rows: usize,
}

#[derive(Clone, Copy)]
struct MergeEqualGroupsSpec<'a> {
    kind: JoinKind,
    on: &'a Expr,
    left_columns: &'a [String],
    right_columns: &'a [String],
    output_budget: usize,
}

fn merge_equal_key_groups(
    env: &SourceExecutionEnv<'_>,
    spec: MergeEqualGroupsSpec<'_>,
    left_group: &[KeyedRow],
    right_group: &[KeyedRow],
) -> Result<MergeEqualGroupsResult, QueryError> {
    let mut left_matched = vec![false; left_group.len()];
    let mut right_matched = vec![false; right_group.len()];
    let mut joined = Vec::new();
    let mut matched_rows = 0usize;

    'left: for (left_offset, left) in left_group.iter().enumerate() {
        for (right_offset, right) in right_group.iter().enumerate() {
            check_timeout(env.controls)?;
            let combined = combine_rows(&left.row, &right.row);
            if filter::eval_scalar(
                &combined,
                spec.on,
                env.params,
                None,
                env.user_functions,
                None,
                env.session,
            )?
            .is_true()
            {
                left_matched[left_offset] = true;
                right_matched[right_offset] = true;
                matched_rows += 1;
                joined.push(combined);
                if joined.len() >= spec.output_budget {
                    break 'left;
                }
            }
        }
    }

    if joined.len() < spec.output_budget && matches!(spec.kind, JoinKind::Left | JoinKind::Full) {
        for (offset, matched) in left_matched.iter().enumerate() {
            if !matched {
                joined.push(combine_row_with_nulls(
                    &left_group[offset].row,
                    spec.right_columns,
                ));
                if joined.len() >= spec.output_budget {
                    break;
                }
            }
        }
    }
    if joined.len() < spec.output_budget && matches!(spec.kind, JoinKind::Right | JoinKind::Full) {
        for (offset, matched) in right_matched.iter().enumerate() {
            if !matched {
                joined.push(combine_nulls_with_row(
                    spec.left_columns,
                    &right_group[offset].row,
                ));
                if joined.len() >= spec.output_budget {
                    break;
                }
            }
        }
    }

    Ok(MergeEqualGroupsResult {
        joined,
        matched_rows,
    })
}

fn append_left_unmatched(
    joined: &mut Vec<BatchRow>,
    kind: JoinKind,
    rows: &[KeyedRow],
    right_columns: &[String],
    output_budget: usize,
) {
    if joined.len() >= output_budget {
        return;
    }
    if matches!(kind, JoinKind::Left | JoinKind::Full) {
        for left in rows {
            joined.push(combine_row_with_nulls(&left.row, right_columns));
            if joined.len() >= output_budget {
                break;
            }
        }
    }
}

fn append_right_unmatched(
    joined: &mut Vec<BatchRow>,
    kind: JoinKind,
    left_columns: &[String],
    rows: &[KeyedRow],
    output_budget: usize,
) {
    if joined.len() >= output_budget {
        return;
    }
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for right in rows {
            joined.push(combine_nulls_with_row(left_columns, &right.row));
            if joined.len() >= output_budget {
                break;
            }
        }
    }
}
