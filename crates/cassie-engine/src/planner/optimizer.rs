use crate::catalog::{local_name, CollectionCardinalityStats};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{BinaryOp, Expr, JoinKind, QuerySource};
use std::collections::{BTreeSet, HashMap};
use std::hash::BuildHasher;

#[path = "optimizer/cost.rs"]
mod cost;
#[path = "optimizer/cross_joins.rs"]
mod cross_joins;
#[path = "optimizer/inputs.rs"]
mod inputs;

#[cfg(test)]
#[path = "optimizer/cost_observation_tests.rs"]
mod cost_observation_tests;

#[must_use]
pub fn optimize(plan: LogicalPlan) -> LogicalPlan {
    optimize_with_stats(plan, &HashMap::new())
}

#[must_use]
pub fn optimize_with_stats<S: BuildHasher>(
    mut plan: LogicalPlan,
    cardinality_stats: &HashMap<String, CollectionCardinalityStats, S>,
) -> LogicalPlan {
    cross_joins::bind_equalities(&mut plan);
    if let Some(terms) = InnerJoinTerms::from_source(&plan.source) {
        if terms.relations.len() >= 3 {
            let inputs = inputs::resolve(&terms.relations, &terms.predicates, cardinality_stats);
            let order = enumerate_join_order(&inputs);
            plan.source = terms.rebuild(&order, &inputs);
        }
    }
    plan
}

#[derive(Debug)]
struct InnerJoinTerms {
    relations: Vec<QuerySource>,
    predicates: Vec<Expr>,
}

impl InnerJoinTerms {
    fn from_source(source: &QuerySource) -> Option<Self> {
        let mut relations = Vec::new();
        let mut predicates = Vec::new();
        if !flatten_inner_joins(source, &mut relations, &mut predicates)
            || relations.iter().any(relation_name_from_source_is_none)
        {
            return None;
        }
        Some(Self {
            relations,
            predicates,
        })
    }

    fn rebuild(self, order: &[usize], inputs: &inputs::JoinInputs) -> QuerySource {
        let mut unused = self.predicates.into_iter().enumerate().collect::<Vec<_>>();
        let first = order[0];
        let mut source = self.relations[first].clone();

        for (position, index) in order.iter().copied().enumerate().skip(1) {
            let last = position + 1 == order.len();
            let mut selected = Vec::new();
            let mut remaining = Vec::new();
            for (predicate_index, predicate) in unused {
                let ready = inputs.predicates[predicate_index]
                    .ready(&order[..position], index, last)
                    // Keep an unknown whole ON intact until all inputs exist.
                    .unwrap_or(last);
                if ready {
                    selected.push(predicate);
                } else {
                    remaining.push((predicate_index, predicate));
                }
            }
            unused = remaining;
            source = QuerySource::Join {
                left: Box::new(source),
                right: Box::new(self.relations[index].clone()),
                kind: JoinKind::Inner,
                on: combine_predicates(selected),
            };
        }
        source
    }
}

fn relation_name_from_source_is_none(source: &QuerySource) -> bool {
    !matches!(source, QuerySource::Collection(_))
}

fn flatten_inner_joins(
    source: &QuerySource,
    relations: &mut Vec<QuerySource>,
    predicates: &mut Vec<Expr>,
) -> bool {
    match source {
        QuerySource::Join {
            left,
            right,
            kind: JoinKind::Inner,
            on,
        } => {
            if !flatten_inner_joins(left, relations, predicates)
                || !flatten_inner_joins(right, relations, predicates)
            {
                return false;
            }
            predicates.push(on.clone());
            true
        }
        QuerySource::Join { .. } => false,
        relation => {
            relations.push(relation.clone());
            true
        }
    }
}

fn enumerate_join_order(inputs: &inputs::JoinInputs) -> Vec<usize> {
    let count = inputs.rows.len();
    if count <= 8 {
        let mut best = None;
        enumerate_permutations(
            &mut Vec::with_capacity(count),
            &mut vec![false; count],
            inputs,
            &mut best,
        );
        best.map_or_else(Vec::new, |(_, order)| order)
    } else {
        greedy_order(inputs)
    }
}

fn enumerate_permutations(
    current: &mut Vec<usize>,
    used: &mut [bool],
    inputs: &inputs::JoinInputs,
    best: &mut Option<(u128, Vec<usize>)>,
) {
    if current.len() == inputs.rows.len() {
        let cost = join_order_cost(current, inputs);
        if best.as_ref().is_none_or(|(best_cost, best_order)| {
            cost < *best_cost
                || (cost == *best_cost
                    && current
                        .iter()
                        .map(|index| &inputs.labels[*index])
                        .cmp(best_order.iter().map(|index| &inputs.labels[*index]))
                        .is_lt())
        }) {
            *best = Some((cost, current.clone()));
        }
        return;
    }
    for index in 0..inputs.rows.len() {
        if used[index] {
            continue;
        }
        used[index] = true;
        current.push(index);
        enumerate_permutations(current, used, inputs, best);
        current.pop();
        used[index] = false;
    }
}

fn join_order_cost(order: &[usize], inputs: &inputs::JoinInputs) -> u128 {
    cost::order_cost(order, inputs)
}

fn greedy_order(inputs: &inputs::JoinInputs) -> Vec<usize> {
    let mut order = Vec::with_capacity(inputs.rows.len());
    let mut used = vec![false; inputs.rows.len()];
    while order.len() < inputs.rows.len() {
        let mut best: Option<(u128, usize)> = None;
        for (index, is_used) in used.iter().enumerate() {
            if *is_used {
                continue;
            }
            order.push(index);
            let cost = join_order_cost(&order, inputs);
            order.pop();
            if best.is_none_or(|(best_cost, best_index)| {
                (cost, &inputs.labels[index]) < (best_cost, &inputs.labels[best_index])
            }) {
                best = Some((cost, index));
            }
        }
        let index = best.expect("one unused relation remains").1;
        used[index] = true;
        order.push(index);
    }
    order
}

fn combine_predicates(mut predicates: Vec<Expr>) -> Expr {
    let Some(first) = predicates.pop() else {
        return Expr::BoolLiteral(true);
    };
    predicates
        .into_iter()
        .fold(first, |left, right| Expr::Binary {
            left: Box::new(left),
            op: BinaryOp::And,
            right: Box::new(right),
        })
}

#[must_use]
pub fn join_order(source: &QuerySource) -> Vec<String> {
    let mut order = Vec::new();
    collect_join_order(source, &mut order);
    order
}

fn collect_join_order(source: &QuerySource, order: &mut Vec<String>) {
    match source {
        QuerySource::Join { left, right, .. } => {
            collect_join_order(left, order);
            collect_join_order(right, order);
        }
        QuerySource::Collection(name) => order.push(local_name(name)),
        _ => order.push("derived".to_string()),
    }
}

#[must_use]
pub fn join_enumeration(source: &QuerySource) -> &'static str {
    let count = join_order(source).len();
    if count >= 3 && all_inner_joins(source) {
        if count <= 8 {
            "exhaustive"
        } else {
            "greedy"
        }
    } else {
        "none"
    }
}

fn all_inner_joins(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join {
            left,
            right,
            kind: JoinKind::Inner,
            ..
        } => all_inner_joins(left) && all_inner_joins(right),
        QuerySource::Join { .. } => false,
        _ => true,
    }
}

#[must_use]
pub fn join_legality_barriers(source: &QuerySource) -> Vec<String> {
    let mut barriers = BTreeSet::new();
    collect_join_legality_barriers(source, &mut barriers);
    barriers.into_iter().collect()
}

#[must_use]
pub fn join_required_columns(source: &QuerySource) -> Vec<String> {
    let mut columns = BTreeSet::new();
    collect_join_required_columns(source, &mut columns);
    columns.into_iter().collect()
}

fn collect_join_required_columns(source: &QuerySource, columns: &mut BTreeSet<String>) {
    if let QuerySource::Join {
        left, right, on, ..
    } = source
    {
        collect_expression_columns(on, columns);
        collect_join_required_columns(left, columns);
        collect_join_required_columns(right, columns);
    }
}

fn collect_expression_columns(expr: &Expr, columns: &mut BTreeSet<String>) {
    if let Expr::Column(column) = expr {
        columns.insert(column.clone());
    }
    expr.for_each_child(|child| collect_expression_columns(child, columns));
}

#[must_use]
pub fn join_is_parameterized(source: &QuerySource) -> bool {
    match source {
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            join_is_parameterized(source)
        }
        QuerySource::Join { left, right, .. } => {
            join_is_parameterized(left) || join_is_parameterized(right)
        }
        QuerySource::Subquery { lateral, .. } | QuerySource::TableFunction { lateral, .. } => {
            *lateral
        }
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::Values { .. }
        | QuerySource::SingleRow => false,
    }
}

fn collect_join_legality_barriers(source: &QuerySource, barriers: &mut BTreeSet<String>) {
    if let QuerySource::Join {
        left, right, kind, ..
    } = source
    {
        match kind {
            JoinKind::Inner => {}
            JoinKind::Left => {
                barriers.insert("left_outer".to_string());
            }
            JoinKind::Right => {
                barriers.insert("right_outer".to_string());
            }
            JoinKind::Full => {
                barriers.insert("full_outer".to_string());
            }
            JoinKind::Cross => {
                barriers.insert("cross".to_string());
            }
        }
        collect_join_legality_barriers(left, barriers);
        collect_join_legality_barriers(right, barriers);
    }
}
