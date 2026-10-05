use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::batch::{flatten_batches, row_tie_key, Batch, DEFAULT_BATCH_SIZE};
use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, NullsOrder, OrderExpr, SelectItem, SortDirection};
use crate::types::Value;

#[cfg(test)]
mod tests;

mod retention;
use retention::{SortRetentionContext, SortRetentionPhase};
mod accounting;

pub(crate) fn sort_batches_with_controls(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    if eval.order.is_empty() {
        return Ok(batches);
    }
    let rows = sort_rows_with_controls(flatten_batches(batches), eval, controls)?;
    chunk_rows_controlled(rows.into_iter(), controls)
}

pub(crate) fn sort_rows_with_controls<R>(
    rows: Vec<R>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<R>, crate::executor::QueryError>
where
    R: RowAccess,
{
    sort_rows_with_context(rows, eval, controls, &SortRetentionContext::default())
}

fn sort_rows_with_context<R>(
    rows: Vec<R>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
    retention: &SortRetentionContext<'_>,
) -> Result<Vec<R>, crate::executor::QueryError>
where
    R: RowAccess,
{
    use crate::executor::retained_memory::{add, mul};

    let order = eval.resolved_order();
    let slots = mul(rows.len(), 2 * std::mem::size_of::<(RowKey, R)>())?;
    let headers = mul(rows.len(), 2 * std::mem::size_of::<VecDeque<(RowKey, R)>>())?;
    let run_memory = controls.reserve_query_memory(add(slots, headers)?)?;
    retention.before(SortRetentionPhase::RunBacking)?;
    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        check_query_controls(controls)?;
        let key = eval.row_key_with_context(&row, &order, retention, Some(controls))?;
        runs.push(VecDeque::from([(key, row)]));
    }
    while runs.len() > 1 {
        #[cfg(test)]
        let current_header_capacity = runs.capacity();
        let mut merged = Vec::with_capacity(runs.len().div_ceil(2));
        let mut run_iter = runs.into_iter();
        while let Some(left) = run_iter.next() {
            let Some(right) = run_iter.next() else {
                merged.push(left);
                break;
            };
            #[cfg(test)]
            let backing = if retention.has_run_probe() {
                Some(retention::RunBackingSnapshot {
                    other_deque_slots: merged
                        .iter()
                        .chain(run_iter.as_slice())
                        .try_fold(0, |slots, run| add(slots, run.capacity()))?,
                    header_slots: add(current_header_capacity, merged.capacity())?,
                    reservation_bytes: run_memory.bytes(),
                })
            } else {
                None
            };
            merged.push(merge_sorted_runs(
                left,
                right,
                controls,
                #[cfg(test)]
                retention,
                #[cfg(test)]
                backing.as_ref(),
            )?);
        }
        runs = merged;
    }
    check_query_controls(controls)?;
    let sorted = runs
        .pop()
        .unwrap_or_default()
        .into_iter()
        .map(|(_, row)| row)
        .collect();
    drop(run_memory);
    Ok(sorted)
}

fn merge_sorted_runs<R>(
    mut left: VecDeque<(RowKey, R)>,
    mut right: VecDeque<(RowKey, R)>,
    controls: &QueryExecutionControls,
    #[cfg(test)] retention: &SortRetentionContext<'_>,
    #[cfg(test)] backing: Option<&retention::RunBackingSnapshot>,
) -> Result<VecDeque<(RowKey, R)>, crate::executor::QueryError> {
    let mut merged = VecDeque::with_capacity(left.len() + right.len());
    #[cfg(test)]
    if let Some(backing) = backing {
        retention.observe_run_merge(backing, &left, &right, &merged)?;
    }
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
    top_k_batches_with_context(
        batches,
        eval,
        top_needed,
        controls,
        &SortRetentionContext::default(),
    )
}

fn top_k_batches_with_context(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    top_needed: usize,
    controls: &QueryExecutionControls,
    retention: &SortRetentionContext<'_>,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    if eval.order.is_empty() || top_needed == 0 {
        return Ok(Vec::new());
    }
    let order = eval.resolved_order();
    let mut top = BinaryHeap::new();
    let mut backing_memory = controls.reserve_query_memory(0)?;
    for row in batches.into_iter().flatten() {
        check_query_controls(controls)?;
        let candidate = TopCandidate {
            key: eval.row_key_with_context(&row, &order, retention, Some(controls))?,
            row,
        };
        if top.len() < top_needed && top.len() == top.capacity() {
            use crate::executor::retained_memory::mul;

            let next_capacity = top
                .capacity()
                .checked_mul(2)
                .ok_or_else(|| {
                    crate::app::CassieError::ResourceLimit("sort heap capacity overflow".to_owned())
                })?
                .max(1)
                .min(top_needed);
            backing_memory.try_grow(mul(
                next_capacity - top.capacity(),
                std::mem::size_of::<TopCandidate>(),
            )?)?;
            retention.before(SortRetentionPhase::HeapBacking)?;
            top.try_reserve_exact(next_capacity - top.len())
                .map_err(|error| sort_allocation_error(&error))?;
        }
        push_top_candidate(&mut top, top_needed, candidate);
    }
    let _sort_scratch = controls.reserve_query_memory(crate::executor::retained_memory::mul(
        top.len(),
        std::mem::size_of::<TopCandidate>(),
    )?)?;
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_top_candidates);
    check_query_controls(controls)?;
    chunk_rows_controlled(ranked.into_iter().map(|candidate| candidate.row), controls)
}

fn chunk_rows_controlled(
    mut rows: impl ExactSizeIterator<Item = crate::executor::batch::BatchRow>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    use crate::executor::retained_memory::{add, mul};

    let batch_count = rows.len().div_ceil(DEFAULT_BATCH_SIZE);
    let _chunk_memory = controls.reserve_query_memory(add(
        mul(
            rows.len(),
            std::mem::size_of::<crate::executor::batch::BatchRow>(),
        )?,
        mul(batch_count, std::mem::size_of::<Batch>())?,
    )?)?;
    let mut batches = Vec::new();
    batches
        .try_reserve_exact(batch_count)
        .map_err(|error| sort_allocation_error(&error))?;
    while rows.len() > 0 {
        check_query_controls(controls)?;
        let count = rows.len().min(DEFAULT_BATCH_SIZE);
        let mut batch = Vec::new();
        batch
            .try_reserve_exact(count)
            .map_err(|error| sort_allocation_error(&error))?;
        batch.extend(rows.by_ref().take(count));
        batches.push(batch);
    }
    check_query_controls(controls)?;
    Ok(batches)
}

fn sort_allocation_error(error: &std::collections::TryReserveError) -> crate::executor::QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain sort state: {error}")).into()
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
    let order = eval.resolved_order();
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
    for row in rows {
        let candidate = TopCandidate {
            key: eval.row_key(&row, &order)?,
            row,
        };
        push_top_candidate(&mut top, top_needed, candidate);
    }
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_top_candidates);
    Ok(ranked.into_iter().map(|candidate| candidate.row).collect())
}

/// Resolves an ORDER BY reference to a projection alias into the aliased
/// expression.
pub(crate) fn alias_expr(expr: &Expr, projection: &[SelectItem]) -> Option<Expr> {
    match expr {
        Expr::Column(alias) => projection.iter().find_map(|item| {
            let reference_key = crate::sql::ColumnIdentifierPath::reference_field_key(alias);
            match item {
                SelectItem::Column {
                    name,
                    alias: Some(project_alias),
                    ..
                } if crate::sql::ColumnIdentifierPath::stored_field_key(project_alias)
                    == reference_key =>
                {
                    Some(Expr::Column(name.clone()))
                }
                SelectItem::Function {
                    function,
                    alias: Some(project_alias),
                    ..
                } if crate::sql::ColumnIdentifierPath::stored_field_key(project_alias)
                    == reference_key =>
                {
                    Some(Expr::Function(function.clone()))
                }
                SelectItem::Expr {
                    expr,
                    alias: Some(project_alias),
                } if crate::sql::ColumnIdentifierPath::stored_field_key(project_alias)
                    == reference_key =>
                {
                    Some(expr.clone())
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
    fn resolved_order(&self) -> Vec<OrderExpr> {
        self.order
            .iter()
            .map(|order| OrderExpr {
                expr: alias_expr(&order.expr, self.projection)
                    .unwrap_or_else(|| order.expr.clone()),
                direction: order.direction.clone(),
                nulls: order.nulls,
            })
            .collect()
    }

    fn row_key<R: RowAccess>(
        &self,
        row: &R,
        order: &[OrderExpr],
    ) -> Result<RowKey, crate::executor::QueryError> {
        self.row_key_with_context(row, order, &SortRetentionContext::default(), None)
    }

    fn row_key_with_context<R: RowAccess>(
        &self,
        row: &R,
        order: &[OrderExpr],
        retention: &SortRetentionContext<'_>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<RowKey, crate::executor::QueryError> {
        use crate::executor::retained_memory::{add, mul};

        let mut memory = controls
            .map(|controls| {
                controls.reserve_query_memory(add(
                    std::mem::size_of::<RowKey>(),
                    mul(order.len(), std::mem::size_of::<KeyPart>())?,
                )?)
            })
            .transpose()?;
        let mut parts = Vec::with_capacity(order.len());
        for order in order {
            let value = self.value(row, &order.expr)?;
            if let Some(controls) = controls {
                check_query_controls(controls)?;
            }
            let previous = memory
                .as_ref()
                .map_or(0, crate::runtime::QueryMemoryReservation::bytes);
            let mut admitted = 0;
            if let Some(memory) = memory.as_mut() {
                memory.try_grow(accounting::type_scratch(
                    row,
                    &order.expr,
                    self.user_functions,
                )?)?;
                let data_type = row.has_array_types().then(|| {
                    super::array_order::expression_type(row, &order.expr, self.user_functions)
                });
                admitted = accounting::conversion_bytes(&value, data_type.flatten().as_ref())?;
                memory.try_grow(admitted)?;
            }
            retention.before(SortRetentionPhase::SemanticPart)?;
            let semantic = super::array_order::key(row, &order.expr, &value, self.user_functions);
            if let Some(memory) = memory.as_mut() {
                let retained = accounting::semantic_heap(&semantic)?;
                if retained > admitted {
                    return Err(crate::app::CassieError::ResourceLimit(
                        "sort key exceeded its admitted allocation shape".to_owned(),
                    )
                    .into());
                }
                drop(value);
                memory.shrink_to(add(previous, retained)?);
            }
            parts.push(KeyPart {
                value: semantic,
                direction: order.direction.clone(),
                nulls: order.nulls,
            });
        }
        let previous = memory
            .as_ref()
            .map_or(0, crate::runtime::QueryMemoryReservation::bytes);
        if let Some(memory) = memory.as_mut() {
            memory.try_grow(accounting::tie_bytes(row)?)?;
        }
        retention.before(SortRetentionPhase::TieKey)?;
        let tie_key = row_tie_key(row);
        if let Some(memory) = memory.as_mut() {
            memory.shrink_to(add(previous, tie_key.capacity())?);
        }
        if let Some(controls) = controls {
            check_query_controls(controls)?;
        }
        Ok(RowKey {
            parts,
            tie_key,
            _memory: memory,
        })
    }

    fn value<R: RowAccess>(
        &self,
        row: &R,
        expr: &Expr,
    ) -> Result<Value, crate::executor::QueryError> {
        filter::evaluate_expr_value(
            row,
            expr,
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
    _memory: Option<crate::runtime::QueryMemoryReservation>,
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
