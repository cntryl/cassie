use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::batch::{chunk_rows, flatten_batches, row_tie_key, Batch, DEFAULT_BATCH_SIZE};
use crate::executor::filter;
use crate::executor::filter::ScalarValue;
use crate::executor::filter::SearchContext;
use crate::sql::ast::{Expr, NullsOrder, OrderExpr, SelectItem, SortDirection};
use crate::types::Value;

pub(crate) fn sort_rows<R>(
    mut rows: Vec<R>,
    order: &[OrderExpr],
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Vec<R>
where
    R: RowAccess,
{
    if order.is_empty() {
        return rows;
    }

    let eval = EvalInput {
        order,
        projection,
        params,
        search_context,
        user_functions,
        session,
    };
    rows.sort_by(|left, right| eval.compare_rows(left, right));

    rows
}

pub(crate) fn sort_batches(
    batches: Vec<Batch>,
    order: &[OrderExpr],
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Vec<Batch> {
    if order.is_empty() {
        return batches;
    }

    let rows = flatten_batches(batches);
    let rows = sort_rows(
        rows,
        order,
        projection,
        params,
        search_context,
        user_functions,
        session,
    );
    chunk_rows(rows, DEFAULT_BATCH_SIZE)
}

pub(crate) fn top_k_batches(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    top_needed: usize,
) -> Vec<Batch> {
    if eval.order.is_empty() || top_needed == 0 {
        return Vec::new();
    }

    let mut top = BinaryHeap::with_capacity(top_needed.min(DEFAULT_BATCH_SIZE).saturating_add(1));
    for row in flatten_batches(batches) {
        let candidate = TopCandidate {
            key: eval.row_key(&row),
            row,
        };
        push_top_candidate(&mut top, top_needed, candidate);
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_top_candidates);
    let rows = ranked
        .into_iter()
        .map(|candidate| candidate.row)
        .collect::<Vec<_>>();
    chunk_rows(rows, DEFAULT_BATCH_SIZE)
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

fn compare_scalar(left: &ScalarValue, right: &ScalarValue) -> Ordering {
    if let (Some(left), Some(right)) = (left.to_f64(), right.to_f64()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }

    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left.cmp(right);
    }

    Ordering::Equal
}

fn compare_nulls(
    left: &ScalarValue,
    right: &ScalarValue,
    nulls: Option<NullsOrder>,
) -> Option<Ordering> {
    if let Some(nulls) = nulls {
        let left_null = matches!(left, ScalarValue::Null);
        let right_null = matches!(right, ScalarValue::Null);
        if left_null != right_null {
            return Some(match (left_null, nulls) {
                (true, NullsOrder::First) | (false, NullsOrder::Last) => Ordering::Less,
                (true, NullsOrder::Last) | (false, NullsOrder::First) => Ordering::Greater,
            });
        }
    }

    None
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
    fn compare_rows<R: RowAccess>(&self, left: &R, right: &R) -> Ordering {
        for OrderExpr {
            expr,
            direction,
            nulls,
        } in self.order
        {
            let left_value = self.value(left, expr);
            let right_value = self.value(right, expr);
            let cmp = compare_ordered_values(&left_value, &right_value, direction, *nulls);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }

        let left_key = row_tie_key(left);
        let right_key = row_tie_key(right);
        left_key.cmp(&right_key)
    }

    fn row_key<R: RowAccess>(&self, row: &R) -> RowKey {
        let parts = self
            .order
            .iter()
            .map(|order| KeyPart {
                value: self.value(row, &order.expr),
                direction: order.direction.clone(),
                nulls: order.nulls,
            })
            .collect();
        RowKey {
            parts,
            tie_key: row_tie_key(row),
        }
    }

    fn value<R: RowAccess>(&self, row: &R, expr: &Expr) -> ScalarValue {
        let base = filter::eval_scalar(
            row,
            expr,
            self.params,
            self.search_context,
            self.user_functions,
            None,
            self.session,
        )
        .unwrap_or(ScalarValue::Null);
        if !matches!(base, ScalarValue::Null) {
            return base;
        }

        alias_expr(expr, self.projection).map_or(base, |alias_expr| {
            filter::eval_scalar(
                row,
                &alias_expr,
                self.params,
                self.search_context,
                self.user_functions,
                None,
                self.session,
            )
            .unwrap_or(ScalarValue::Null)
        })
    }
}

struct KeyPart {
    value: ScalarValue,
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

fn compare_ordered_values(
    left: &ScalarValue,
    right: &ScalarValue,
    direction: &SortDirection,
    nulls: Option<NullsOrder>,
) -> Ordering {
    if let Some(cmp) = compare_nulls(left, right, nulls) {
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
