use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ReadOperatorCandidate {
    pub label: String,
    pub selected_index: Option<String>,
    pub operator_family: &'static str,
    pub estimated_rows: u64,
    pub base_cost: u64,
    pub base_selected: bool,
    pub index: Option<IndexMeta>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReadOperatorSelection {
    pub base_selected_index: Option<String>,
    pub candidates: Vec<ReadOperatorCandidate>,
}

pub(crate) fn read_operator_selection(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> ReadOperatorSelection {
    let base_selected_index = base_selected_index(plan, indexes, cardinality_stats);
    let scan_estimates = PlanEstimates::from_plan(plan, None, cardinality_stats);
    let mut candidates = vec![ReadOperatorCandidate {
        label: "row_scan".to_string(),
        selected_index: None,
        operator_family: "row_scan",
        estimated_rows: scan_estimates.scan_rows,
        base_cost: scan_estimates.scan_cost,
        base_selected: base_selected_index.is_none(),
        index: None,
    }];

    for index in scalar_candidates(plan, indexes, cardinality_stats) {
        let estimates =
            PlanEstimates::from_plan(plan, Some(index.name.as_str()), cardinality_stats);
        candidates.push(ReadOperatorCandidate {
            label: format!("index:{}", index.name),
            selected_index: Some(index.name.clone()),
            operator_family: "index_read",
            estimated_rows: estimates.index_rows,
            base_cost: estimates.index_cost,
            base_selected: base_selected_index.as_deref() == Some(index.name.as_str()),
            index: Some(index),
        });
    }

    if let Some(selected_index) = base_selected_index.as_deref() {
        if candidates
            .iter()
            .all(|candidate| candidate.selected_index.as_deref() != Some(selected_index))
        {
            if let Some(index) = indexes.iter().find(|index| index.name == selected_index) {
                let estimates =
                    PlanEstimates::from_plan(plan, Some(index.name.as_str()), cardinality_stats);
                candidates.push(ReadOperatorCandidate {
                    label: format!("index:{}", index.name),
                    selected_index: Some(index.name.clone()),
                    operator_family: "index_read",
                    estimated_rows: estimates.index_rows,
                    base_cost: estimates.index_cost,
                    base_selected: true,
                    index: Some(index.clone()),
                });
            }
        }
    }

    ReadOperatorSelection {
        base_selected_index,
        candidates,
    }
}

pub(crate) fn base_selected_index(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> Option<String> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let equality_fields = plan
        .filter
        .as_ref()
        .map(super::equality_filter_fields)
        .unwrap_or_default();
    let equality_expressions = plan
        .filter
        .as_ref()
        .map(super::equality_filter_expressions)
        .unwrap_or_default();
    let scalar = scalar_candidates(plan, indexes, cardinality_stats)
        .into_iter()
        .find(|index| {
            scalar_index_matches_plan(plan, index, &equality_fields, &equality_expressions)
        })
        .map(|index| index.name.clone());
    scalar.or_else(|| {
        plan.filter
            .as_ref()
            .and_then(|filter| time_series::selected_time_series_index(collection, filter, indexes))
    })
}

fn scalar_candidates(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> Vec<IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return Vec::new();
    };
    let equality_fields = plan
        .filter
        .as_ref()
        .map(super::equality_filter_fields)
        .unwrap_or_default();
    let equality_expressions = plan
        .filter
        .as_ref()
        .map(super::equality_filter_expressions)
        .unwrap_or_default();
    let mut scalar = indexes
        .iter()
        .filter(|index| index.collection == *collection && index.kind == IndexKind::Scalar)
        .filter(|index| partial_index_matches_query(plan.filter.as_ref(), index.predicate.as_ref()))
        .filter(|index| {
            scalar_index_matches_plan(plan, index, &equality_fields, &equality_expressions)
        })
        .cloned()
        .collect::<Vec<_>>();
    scalar.sort_by(|left, right| {
        compare_scalar_index_candidates(plan, collection, left, right, cardinality_stats)
    });
    scalar
}

fn scalar_index_matches_plan(
    plan: &LogicalPlan,
    index: &IndexMeta,
    equality_fields: &BTreeSet<String>,
    equality_expressions: &BTreeSet<String>,
) -> bool {
    if index.expressions.is_empty() {
        let field_match = index
            .normalized_fields()
            .iter()
            .all(|field| equality_fields.contains(&field.to_ascii_lowercase()));
        return scalar_index_plan_shape(plan, index).is_some() || field_match;
    }

    let field_match = index
        .normalized_fields()
        .iter()
        .all(|field| equality_fields.contains(&field.to_ascii_lowercase()));
    let expression_match = index
        .normalized_expressions()
        .iter()
        .all(|expression| equality_expressions.contains(expression));
    field_match && expression_match
}

fn compare_scalar_index_candidates(
    plan: &LogicalPlan,
    collection: &str,
    left: &IndexMeta,
    right: &IndexMeta,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> std::cmp::Ordering {
    match (
        scalar_index_plan_shape(plan, left),
        scalar_index_plan_shape(plan, right),
    ) {
        (Some(left_shape), Some(right_shape)) => scalar_index_path_rank(left_shape.path)
            .cmp(&scalar_index_path_rank(right_shape.path))
            .then_with(|| {
                right_shape
                    .equality_prefix_len
                    .cmp(&left_shape.equality_prefix_len)
            })
            .then_with(|| {
                right_shape
                    .order_columns_used
                    .cmp(&left_shape.order_columns_used)
            })
            .then_with(|| {
                index_estimate(collection, left, cardinality_stats).cmp(&index_estimate(
                    collection,
                    right,
                    cardinality_stats,
                ))
            })
            .then_with(|| {
                let right_specificity =
                    right.normalized_fields().len() + right.normalized_expressions().len();
                let left_specificity =
                    left.normalized_fields().len() + left.normalized_expressions().len();
                right_specificity.cmp(&left_specificity)
            })
            .then_with(|| left.name.cmp(&right.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => index_estimate(collection, left, cardinality_stats)
            .cmp(&index_estimate(collection, right, cardinality_stats))
            .then_with(|| {
                let right_specificity =
                    right.normalized_fields().len() + right.normalized_expressions().len();
                let left_specificity =
                    left.normalized_fields().len() + left.normalized_expressions().len();
                right_specificity.cmp(&left_specificity)
            })
            .then_with(|| left.name.cmp(&right.name)),
    }
}

fn scalar_index_path_rank(path: ScalarIndexPlanPath) -> u8 {
    match path {
        ScalarIndexPlanPath::IndexSeek => 0,
        ScalarIndexPlanPath::PrefixScan => 1,
        ScalarIndexPlanPath::RangeScan => 2,
        ScalarIndexPlanPath::OrderedBoundedScan => 3,
    }
}

fn index_estimate(
    collection: &str,
    index: &IndexMeta,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    cardinality_stats
        .get(collection)
        .filter(|stats| stats.hydrated)
        .and_then(|stats| {
            stats.index_cardinality(&CollectionCardinalityStats::index_key(
                &index.kind,
                &index.name,
            ))
        })
        .unwrap_or(u64::MAX)
}

fn partial_index_matches_query(
    query_filter: Option<&Expr>,
    index_predicate: Option<&String>,
) -> bool {
    match index_predicate {
        None => true,
        Some(predicate) => {
            query_filter
                .and_then(|filter| serde_json::to_string(filter).ok())
                .as_ref()
                == Some(predicate)
        }
    }
}
