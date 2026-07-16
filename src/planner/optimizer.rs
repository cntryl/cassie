use crate::catalog::{local_name, name_matches, CollectionCardinalityStats};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{BinaryOp, Expr, JoinKind, QuerySource};
use std::collections::{BTreeSet, HashMap};
use std::hash::BuildHasher;

#[must_use]
pub fn optimize(plan: LogicalPlan) -> LogicalPlan {
    optimize_with_stats(plan, &HashMap::new())
}

#[must_use]
pub fn optimize_with_stats<S: BuildHasher>(
    mut plan: LogicalPlan,
    cardinality_stats: &HashMap<String, CollectionCardinalityStats, S>,
) -> LogicalPlan {
    if plan.offset.is_none() {
        plan.offset = Some(0);
    }
    if let Some(terms) = InnerJoinTerms::from_source(&plan.source) {
        if terms.relations.len() >= 3 {
            let order = enumerate_join_order(&terms.relations, cardinality_stats);
            plan.source = terms.rebuild(&order);
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

    fn rebuild(self, order: &[usize]) -> QuerySource {
        let mut unused = self.predicates;
        let first = order[0];
        let mut joined = BTreeSet::from([relation_name(&self.relations[first])]);
        let mut source = self.relations[first].clone();

        for (position, index) in order.iter().copied().enumerate().skip(1) {
            let next_name = relation_name(&self.relations[index]);
            let mut available = joined.clone();
            available.insert(next_name.clone());
            let last = position + 1 == order.len();
            let mut selected = Vec::new();
            let mut remaining = Vec::new();
            for predicate in unused {
                let references = predicate_relations(&predicate);
                let ready = references
                    .iter()
                    .all(|name| relation_set_contains(&available, name))
                    && (last
                        || (relation_set_contains(&references, &next_name)
                            && references
                                .iter()
                                .any(|name| relation_set_contains(&joined, name))));
                if ready {
                    selected.push(predicate);
                } else {
                    remaining.push(predicate);
                }
            }
            unused = remaining;
            source = QuerySource::Join {
                left: Box::new(source),
                right: Box::new(self.relations[index].clone()),
                kind: JoinKind::Inner,
                on: combine_predicates(selected),
            };
            joined.insert(next_name);
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

fn enumerate_join_order<S: BuildHasher>(
    relations: &[QuerySource],
    cardinality_stats: &HashMap<String, CollectionCardinalityStats, S>,
) -> Vec<usize> {
    let rows = relations
        .iter()
        .map(|source| relation_rows(source, cardinality_stats))
        .collect::<Vec<_>>();
    let labels = relations.iter().map(relation_name).collect::<Vec<_>>();
    if relations.len() <= 8 {
        let mut best = None;
        enumerate_permutations(
            &mut Vec::with_capacity(relations.len()),
            &mut vec![false; relations.len()],
            &rows,
            &labels,
            &mut best,
        );
        best.map_or_else(Vec::new, |(_, _, order)| order)
    } else {
        let mut order = (0..relations.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| (rows[*index], labels[*index].clone()));
        order
    }
}

fn enumerate_permutations(
    current: &mut Vec<usize>,
    used: &mut [bool],
    rows: &[u64],
    labels: &[String],
    best: &mut Option<(u128, Vec<String>, Vec<usize>)>,
) {
    if current.len() == rows.len() {
        let cost = join_order_cost(current, rows);
        let label_order = current
            .iter()
            .map(|index| labels[*index].clone())
            .collect::<Vec<_>>();
        if best.as_ref().is_none_or(|(best_cost, best_labels, _)| {
            (cost, &label_order) < (*best_cost, best_labels)
        }) {
            *best = Some((cost, label_order, current.clone()));
        }
        return;
    }
    for index in 0..rows.len() {
        if used[index] {
            continue;
        }
        used[index] = true;
        current.push(index);
        enumerate_permutations(current, used, rows, labels, best);
        current.pop();
        used[index] = false;
    }
}

fn join_order_cost(order: &[usize], rows: &[u64]) -> u128 {
    let mut intermediate = 1u128;
    let mut cost = 0u128;
    for index in order {
        intermediate = intermediate.saturating_mul(u128::from(rows[*index].max(1)));
        cost = cost.saturating_add(intermediate);
    }
    cost
}

fn relation_rows<S: BuildHasher>(
    source: &QuerySource,
    cardinality_stats: &HashMap<String, CollectionCardinalityStats, S>,
) -> u64 {
    let name = relation_name(source);
    cardinality_stats
        .iter()
        .find(|(stored, _)| name_matches(stored, &name) || name_matches(&name, stored))
        .map_or(1, |(_, stats)| stats.row_count.max(1))
}

fn relation_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) => local_name(name),
        _ => "derived".to_string(),
    }
}

fn predicate_relations(expr: &Expr) -> BTreeSet<String> {
    let mut relations = BTreeSet::new();
    collect_predicate_relations(expr, &mut relations);
    relations
}

fn collect_predicate_relations(expr: &Expr, relations: &mut BTreeSet<String>) {
    match expr {
        Expr::Column(column) => {
            if let Some((qualifier, _)) = column.rsplit_once('.') {
                relations.insert(local_name(qualifier));
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_predicate_relations(left, relations);
            collect_predicate_relations(right, relations);
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_predicate_relations(expr, relations);
        }
        Expr::InList { expr, values, .. } => {
            collect_predicate_relations(expr, relations);
            for value in values {
                collect_predicate_relations(value, relations);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_predicate_relations(expr, relations);
            collect_predicate_relations(low, relations);
            collect_predicate_relations(high, relations);
        }
        Expr::Function(function) => {
            for argument in &function.args {
                collect_predicate_relations(argument, relations);
            }
        }
        Expr::Exists(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
}

fn relation_set_contains(relations: &BTreeSet<String>, requested: &str) -> bool {
    relations
        .iter()
        .any(|stored| name_matches(stored, requested) || name_matches(requested, stored))
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
    match expr {
        Expr::Column(column) => {
            columns.insert(column.clone());
        }
        Expr::Binary { left, right, .. } => {
            collect_expression_columns(left, columns);
            collect_expression_columns(right, columns);
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_expression_columns(expr, columns);
        }
        Expr::InList { expr, values, .. } => {
            collect_expression_columns(expr, columns);
            for value in values {
                collect_expression_columns(value, columns);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_expression_columns(expr, columns);
            collect_expression_columns(low, columns);
            collect_expression_columns(high, columns);
        }
        Expr::Function(function) => {
            for argument in &function.args {
                collect_expression_columns(argument, columns);
            }
        }
        Expr::Exists(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
}

#[must_use]
pub fn join_is_parameterized(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { left, right, .. } => {
            join_is_parameterized(left) || join_is_parameterized(right)
        }
        QuerySource::Subquery { lateral, .. } | QuerySource::TableFunction { lateral, .. } => {
            *lateral
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
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
