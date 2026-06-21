use super::*;

impl PlanEstimates {
    const DEFAULT_ROWS: u64 = 1_000;

    pub(super) fn from_plan(
        plan: &LogicalPlan,
        selected_index: Option<&str>,
        cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
    ) -> Self {
        let scan_rows = estimated_source_rows(&plan.source, cardinality_stats);
        let stats = collection_stats(&plan.collection, cardinality_stats);
        let filter_rows = stats.and_then(|stats| {
            plan.filter
                .as_ref()
                .and_then(|filter| estimate_filter_rows(filter, stats))
        });
        let index_rows = selected_index
            .and_then(|name| {
                stats.and_then(|stats| {
                    stats
                        .index_cardinality(&CollectionCardinalityStats::scalar_index_key(name))
                        .or_else(|| {
                            stats.index_cardinality(
                                &CollectionCardinalityStats::fulltext_index_key(name),
                            )
                        })
                        .or_else(|| {
                            stats.index_cardinality(&CollectionCardinalityStats::vector_index_key(
                                name,
                            ))
                        })
                        .or_else(|| {
                            stats.index_cardinality(&CollectionCardinalityStats::hybrid_index_key(
                                name,
                            ))
                        })
                        .or_else(|| {
                            stats.index_cardinality(
                                &CollectionCardinalityStats::time_series_index_key(name),
                            )
                        })
                })
            })
            .into_iter()
            .chain(filter_rows)
            .min()
            .unwrap_or(scan_rows);
        let join_rows = join_rows(&plan.source, cardinality_stats);
        let search_rows = if plan_uses_fulltext(plan) {
            scan_rows.saturating_div(2).max(1)
        } else {
            0
        };
        let vector_rows = if plan_uses_vector(plan) {
            scan_rows.saturating_div(2).max(1)
        } else {
            0
        };
        let aggregate_rows = if plan_uses_aggregate(plan) {
            if plan.group_by.is_empty() {
                1
            } else {
                scan_rows.saturating_div(2).max(1)
            }
        } else {
            0
        };
        let scan_cost = scan_rows;
        let index_cost = selected_index
            .map(|_| index_rows.max(1))
            .unwrap_or(scan_rows);
        let selected_cost = index_cost.min(scan_cost);
        let cost_source = if filter_rows.is_some() {
            "advanced_stats"
        } else if stats.is_some() {
            "cardinality_stats"
        } else {
            "conservative_default"
        }
        .to_string();
        let rejected_alternatives = if selected_index.is_some() && index_cost <= scan_cost {
            vec!["row_scan_cost_higher".to_string()]
        } else if selected_index.is_some() {
            vec!["index_cost_higher".to_string()]
        } else {
            vec!["no_safe_index_alternative".to_string()]
        };

        Self {
            cost_model_version: 1,
            scan_rows,
            index_rows,
            join_rows,
            search_rows,
            vector_rows,
            aggregate_rows,
            scan_cost,
            index_cost,
            selected_cost,
            cost_source,
            rejected_alternatives,
        }
    }
}

fn estimate_filter_rows(expr: &Expr, stats: &CollectionCardinalityStats) -> Option<u64> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            let left_rows = estimate_filter_rows(left, stats);
            let right_rows = estimate_filter_rows(right, stats);
            match (left_rows, right_rows) {
                (Some(left), Some(right)) => Some(left.min(right).max(1)),
                (Some(rows), None) | (None, Some(rows)) => Some(rows.max(1)),
                (None, None) => None,
            }
        }
        Expr::Binary { left, op, right } => estimate_binary_filter_rows(left, op, right, stats),
        Expr::Between {
            expr, low, high, ..
        } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            let low = canonical_literal(low)?;
            let high = canonical_literal(high)?;
            let field_stats = usable_field_stats(stats, field)?;
            histogram_range_estimate(field_stats, Some(low.as_str()), Some(high.as_str()))
        }
        _ => None,
    }
}

fn estimate_binary_filter_rows(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    stats: &CollectionCardinalityStats,
) -> Option<u64> {
    let (field, literal, reversed) = match (left, right) {
        (Expr::Column(field), value) => (field, canonical_literal(value)?, false),
        (value, Expr::Column(field)) => (field, canonical_literal(value)?, true),
        _ => return None,
    };
    let field_stats = usable_field_stats(stats, field)?;

    match (op, reversed) {
        (BinaryOp::Eq, _) => equality_estimate(field_stats, &literal),
        (BinaryOp::Lt | BinaryOp::Lte, false) | (BinaryOp::Gt | BinaryOp::Gte, true) => {
            histogram_range_estimate(field_stats, None, Some(literal.as_str()))
        }
        (BinaryOp::Gt | BinaryOp::Gte, false) | (BinaryOp::Lt | BinaryOp::Lte, true) => {
            histogram_range_estimate(field_stats, Some(literal.as_str()), None)
        }
        _ => None,
    }
}

fn usable_field_stats<'a>(
    stats: &'a CollectionCardinalityStats,
    field: &str,
) -> Option<&'a crate::catalog::FieldCardinalityStats> {
    stats
        .field_stats(field)
        .filter(|field_stats| field_stats.stale_reason.is_none())
        .filter(|field_stats| field_stats.confidence > 0 || field_stats.sample_count > 0)
}

fn equality_estimate(stats: &crate::catalog::FieldCardinalityStats, literal: &str) -> Option<u64> {
    if let Some(hit) = stats
        .heavy_hitters
        .iter()
        .find(|entry| entry.value == literal)
    {
        return Some(hit.count.max(1));
    }
    if stats.distinct_count > 0 {
        return Some(
            stats
                .non_null_count
                .saturating_div(stats.distinct_count)
                .max(1),
        );
    }
    None
}

fn histogram_range_estimate(
    stats: &crate::catalog::FieldCardinalityStats,
    lower: Option<&str>,
    upper: Option<&str>,
) -> Option<u64> {
    if stats.histogram_buckets.is_empty() {
        return Some(stats.non_null_count.saturating_div(2).max(1));
    }
    let count = stats
        .histogram_buckets
        .iter()
        .filter(|bucket| {
            lower.is_none_or(|lower| bucket.upper.as_str() >= lower)
                && upper.is_none_or(|upper| bucket.lower.as_str() <= upper)
        })
        .map(|bucket| bucket.count)
        .sum::<u64>();
    Some(count.max(1))
}

fn canonical_literal(expr: &Expr) -> Option<String> {
    let value = match expr {
        Expr::StringLiteral(value) => serde_json::Value::String(value.clone()),
        Expr::NumberLiteral(value) => {
            let number = serde_json::Number::from_f64(*value)?;
            serde_json::Value::Number(number)
        }
        Expr::BoolLiteral(value) => serde_json::Value::Bool(*value),
        Expr::Null => serde_json::Value::Null,
        _ => return None,
    };
    serde_json::to_string(&value).ok()
}

fn collection_stats<'a>(
    collection: &str,
    cardinality_stats: &'a std::collections::HashMap<String, CollectionCardinalityStats>,
) -> Option<&'a CollectionCardinalityStats> {
    cardinality_stats
        .get(collection)
        .filter(|stats| stats.hydrated)
}

fn estimated_source_rows(
    source: &QuerySource,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    match source {
        QuerySource::Collection(collection) => collection_stats(collection, cardinality_stats)
            .map(|stats| stats.row_count)
            .unwrap_or(PlanEstimates::DEFAULT_ROWS),
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left_rows = estimated_source_rows(left, cardinality_stats);
            let right_rows = estimated_source_rows(right, cardinality_stats);
            match kind {
                JoinKind::Inner if is_equi_join_predicate(on) => left_rows.min(right_rows),
                JoinKind::Cross => left_rows.saturating_mul(right_rows),
                _ => left_rows.saturating_add(right_rows),
            }
        }
        QuerySource::Subquery { select, .. } => select
            .limit
            .and_then(|limit| usize::try_from(limit.max(0)).ok())
            .map(|limit| limit as u64)
            .unwrap_or(PlanEstimates::DEFAULT_ROWS),
        QuerySource::Cte(_) => PlanEstimates::DEFAULT_ROWS,
        QuerySource::SingleRow => 1,
    }
}

fn join_rows(
    source: &QuerySource,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    match source {
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left_rows = estimated_source_rows(left, cardinality_stats);
            let right_rows = estimated_source_rows(right, cardinality_stats);
            match kind {
                JoinKind::Inner if is_equi_join_predicate(on) => left_rows.min(right_rows),
                JoinKind::Cross => left_rows.saturating_mul(right_rows),
                _ => left_rows.saturating_add(right_rows),
            }
        }
        _ => 0,
    }
}
