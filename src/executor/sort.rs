use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::batch::{chunk_rows, flatten_batches, row_tie_key, Batch, DEFAULT_BATCH_SIZE};
use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, NullsOrder, OrderExpr, SelectItem, SortDirection};
use crate::types::Value;

pub(crate) fn sort_batches_with_controls(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    if eval.order.is_empty() {
        return Ok(batches);
    }
    let rows = sort_rows_with_controls(flatten_batches(batches), eval, controls)?;
    Ok(chunk_rows(rows, DEFAULT_BATCH_SIZE))
}

pub(crate) fn sort_rows_with_controls<R>(
    rows: Vec<R>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<R>, crate::executor::QueryError>
where
    R: RowAccess,
{
    let mut runs = Vec::with_capacity(rows.len());
    let mut key_memory = Vec::with_capacity(rows.len());
    for row in rows {
        check_query_controls(controls)?;
        let key = eval.row_key(&row)?;
        key_memory.push(controls.reserve_query_memory(row_key_bytes(&key))?);
        runs.push(VecDeque::from([(key, row)]));
    }
    while runs.len() > 1 {
        let mut merged = Vec::with_capacity(runs.len().div_ceil(2));
        let mut run_iter = runs.into_iter();
        while let Some(left) = run_iter.next() {
            let Some(right) = run_iter.next() else {
                merged.push(left);
                break;
            };
            merged.push(merge_sorted_runs(left, right, controls)?);
        }
        runs = merged;
    }
    Ok(runs
        .pop()
        .unwrap_or_default()
        .into_iter()
        .map(|(_, row)| row)
        .collect())
}

fn merge_sorted_runs<R>(
    mut left: VecDeque<(RowKey, R)>,
    mut right: VecDeque<(RowKey, R)>,
    controls: &QueryExecutionControls,
) -> Result<VecDeque<(RowKey, R)>, crate::executor::QueryError> {
    let mut merged = VecDeque::with_capacity(left.len() + right.len());
    while !left.is_empty() && !right.is_empty() {
        check_query_controls(controls)?;
        if compare_row_keys(&left[0].0, &right[0].0) == Ordering::Greater {
            merged.push_back(right.pop_front().expect("right run is nonempty"));
        } else {
            merged.push_back(left.pop_front().expect("left run is nonempty"));
        }
    }
    merged.append(&mut left);
    merged.append(&mut right);
    Ok(merged)
}

fn check_query_controls(
    controls: &QueryExecutionControls,
) -> Result<(), crate::executor::QueryError> {
    if controls.is_cancelled() {
        return Err(crate::executor::QueryError::General(
            "query canceled".to_string(),
        ));
    }
    if controls.is_timed_out() {
        return Err(crate::executor::QueryError::General(
            "query timeout exceeded".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn top_k_batches_with_controls(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    top_needed: usize,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    if eval.order.is_empty() || top_needed == 0 {
        return Ok(Vec::new());
    }
    let mut top = BinaryHeap::with_capacity(top_needed.min(DEFAULT_BATCH_SIZE).saturating_add(1));
    let mut top_memory = controls.reserve_query_memory(0)?;
    for row in flatten_batches(batches) {
        check_query_controls(controls)?;
        let candidate = TopCandidate {
            key: eval.row_key(&row)?,
            row,
        };
        push_top_candidate(&mut top, top_needed, candidate);
        drop(top_memory);
        top_memory = controls.reserve_query_memory(
            top.iter()
                .map(|candidate| row_key_bytes(&candidate.key))
                .sum(),
        )?;
    }
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_top_candidates);
    Ok(chunk_rows(
        ranked.into_iter().map(|candidate| candidate.row).collect(),
        DEFAULT_BATCH_SIZE,
    ))
}

/// Maintains the production top-k heap without query-runtime orchestration.
pub(crate) fn maintain_top_k_kernel(
    rows: Vec<crate::executor::batch::BatchRow>,
    eval: &EvalInput<'_>,
    top_needed: usize,
) -> Result<Vec<crate::executor::batch::BatchRow>, crate::executor::QueryError> {
    if eval.order.is_empty() || top_needed == 0 {
        return Ok(Vec::new());
    }
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
    for row in rows {
        let candidate = TopCandidate {
            key: eval.row_key(&row)?,
            row,
        };
        push_top_candidate(&mut top, top_needed, candidate);
    }
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_top_candidates);
    Ok(ranked.into_iter().map(|candidate| candidate.row).collect())
}

fn alias_expr(expr: &Expr, projection: &[SelectItem]) -> Option<Expr> {
    match expr {
        Expr::Column(alias) => projection.iter().find_map(|item| {
            let alias_lower = alias.to_ascii_lowercase();
            match item {
                SelectItem::Column {
                    name,
                    alias: Some(project_alias),
                    ..
                } if project_alias.to_ascii_lowercase() == alias_lower => {
                    Some(Expr::Column(name.clone()))
                }
                SelectItem::Function {
                    function,
                    alias: Some(project_alias),
                    ..
                } if project_alias.to_ascii_lowercase() == alias_lower => {
                    Some(Expr::Function(function.clone()))
                }
                _ => None,
            }
        }),
        _ => None,
    }
}

fn compare_scalar(left: &SemanticValue, right: &SemanticValue) -> Ordering {
    left.cmp(right)
}

fn compare_nulls(
    left: &SemanticValue,
    right: &SemanticValue,
    direction: &SortDirection,
    nulls: Option<NullsOrder>,
) -> Option<Ordering> {
    let left_null = left.is_null();
    let right_null = right.is_null();
    if left_null == right_null {
        return None;
    }

    let nulls = nulls.unwrap_or(match direction {
        SortDirection::Asc => NullsOrder::Last,
        SortDirection::Desc => NullsOrder::First,
    });
    Some(match (left_null, nulls) {
        (true, NullsOrder::First) | (false, NullsOrder::Last) => Ordering::Less,
        (true, NullsOrder::Last) | (false, NullsOrder::First) => Ordering::Greater,
    })
}

pub(crate) struct EvalInput<'a> {
    pub(crate) order: &'a [OrderExpr],
    pub(crate) projection: &'a [SelectItem],
    pub(crate) params: &'a [Value],
    pub(crate) search_context: Option<&'a SearchContext>,
    pub(crate) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(crate) session: Option<&'a CassieSession>,
}

impl EvalInput<'_> {
    fn row_key<R: RowAccess>(&self, row: &R) -> Result<RowKey, crate::executor::QueryError> {
        let parts = self
            .order
            .iter()
            .map(|order| {
                self.value(row, &order.expr).map(|value| KeyPart {
                    value: SemanticValue::from_value(&value),
                    direction: order.direction.clone(),
                    nulls: order.nulls,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RowKey {
            parts,
            tie_key: row_tie_key(row),
        })
    }

    fn value<R: RowAccess>(
        &self,
        row: &R,
        expr: &Expr,
    ) -> Result<Value, crate::executor::QueryError> {
        let base = filter::evaluate_expr_value(
            row,
            expr,
            self.params,
            self.search_context,
            self.user_functions,
            self.session,
            None,
        )?;
        if !matches!(base, Value::Null) {
            return Ok(base);
        }

        let Some(alias_expr) = alias_expr(expr, self.projection) else {
            return Ok(base);
        };
        filter::evaluate_expr_value(
            row,
            &alias_expr,
            self.params,
            self.search_context,
            self.user_functions,
            self.session,
            None,
        )
    }
}

struct KeyPart {
    value: SemanticValue,
    direction: SortDirection,
    nulls: Option<NullsOrder>,
}

struct RowKey {
    parts: Vec<KeyPart>,
    tie_key: String,
}

struct TopCandidate {
    key: RowKey,
    row: crate::executor::batch::BatchRow,
}

impl TopCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_top_candidates(self, other) == Ordering::Less
    }
}

impl PartialEq for TopCandidate {
    fn eq(&self, other: &Self) -> bool {
        compare_top_candidates(self, other) == Ordering::Equal
    }
}

impl Eq for TopCandidate {}

impl PartialOrd for TopCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TopCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_top_candidates(self, other)
    }
}

fn compare_top_candidates(left: &TopCandidate, right: &TopCandidate) -> Ordering {
    compare_row_keys(&left.key, &right.key)
}

fn compare_row_keys(left: &RowKey, right: &RowKey) -> Ordering {
    for (left_part, right_part) in left.parts.iter().zip(&right.parts) {
        let cmp = compare_ordered_values(
            &left_part.value,
            &right_part.value,
            &left_part.direction,
            left_part.nulls,
        );
        if cmp != Ordering::Equal {
            return cmp;
        }
    }

    left.parts
        .len()
        .cmp(&right.parts.len())
        .then_with(|| left.tie_key.cmp(&right.tie_key))
}

fn row_key_bytes(key: &RowKey) -> usize {
    key.tie_key
        .len()
        .saturating_add(
            key.parts
                .len()
                .saturating_mul(std::mem::size_of::<KeyPart>()),
        )
        .saturating_add(
            key.parts
                .iter()
                .map(|part| part.value.estimated_bytes())
                .sum(),
        )
}

fn compare_ordered_values(
    left: &SemanticValue,
    right: &SemanticValue,
    direction: &SortDirection,
    nulls: Option<NullsOrder>,
) -> Ordering {
    if let Some(cmp) = compare_nulls(left, right, direction, nulls) {
        return cmp;
    }

    let cmp = compare_scalar(left, right);
    if cmp == Ordering::Equal {
        return cmp;
    }
    match direction {
        SortDirection::Asc => cmp,
        SortDirection::Desc => cmp.reverse(),
    }
}

fn push_top_candidate(
    top: &mut BinaryHeap<TopCandidate>,
    top_needed: usize,
    candidate: TopCandidate,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if top
        .peek()
        .is_some_and(|worst| candidate.is_better_than(worst))
    {
        top.pop();
        top.push(candidate);
    }
}
