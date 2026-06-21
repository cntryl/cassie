use super::*;

pub(crate) fn execute_vector_distance_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = vector_distance_top_k_spec(plan) else {
        return Ok(None);
    };

    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    let mut candidates = batch::flatten_batches(scan::scan(cassie, session, &spec.collection)?);
    if let Some(filter_expr) = &plan.filter {
        if vector_prefilter_supported(filter_expr, &schema) {
            let before = candidates.len();
            candidates = filter::filter_rows(
                candidates,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            cassie
                .runtime
                .record_vector_prefilter_usage(before, candidates.len(), None);
        } else {
            return Ok(None);
        }
    }
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));

    let final_candidate_count = candidates.len();
    for candidate in candidates {
        let vector = candidate
            .get(&spec.vector_field)
            .and_then(value_to_vector)
            .unwrap_or_default();
        let score = if vector.len() == spec.query.len() && !vector.is_empty() {
            crate::vector::l2_distance(&vector, &spec.query)
        } else {
            f64::INFINITY
        };
        let candidate = SqlVectorCandidate {
            sort_value: match spec.direction {
                SortDirection::Asc => score,
                SortDirection::Desc => -score,
            },
            score,
            id: candidate
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        if top.len() < top_needed {
            top.push(candidate);
        } else if let Some(worst) = top.peek() {
            if candidate.is_better_than(worst) {
                top.pop();
                top.push(candidate);
            }
        }
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    let rows: Vec<BatchRow> = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect();
    record_adaptive_candidate_decision(cassie, adaptive, final_candidate_count, rows.len());
    Ok(Some(rows))
}

pub(super) struct AdaptiveCandidateDecision {
    initial_budget: usize,
    feedback_budget: Option<usize>,
}

pub(super) fn adaptive_candidate_decision(
    cassie: &Cassie,
    collection: &str,
    top_needed: usize,
) -> Result<AdaptiveCandidateDecision, QueryError> {
    let limits = cassie.runtime.limits();
    let max_budget = limits.adaptive_candidate_max.max(1);
    if top_needed > max_budget {
        cassie.runtime.record_adaptive_candidate_limit_error();
        return Err(QueryError::General(format!(
            "top-k candidate requirement {top_needed} exceeds adaptive candidate max {max_budget}"
        )));
    }

    let min_budget = limits.adaptive_candidate_min.max(1).min(max_budget);
    let feedback_budget = cassie
        .runtime
        .feedback_candidate_budget(collection)
        .map(|budget| budget.min(max_budget));
    let initial_budget = top_needed
        .max(min_budget)
        .max(feedback_budget.unwrap_or_default())
        .min(max_budget);

    Ok(AdaptiveCandidateDecision {
        initial_budget,
        feedback_budget,
    })
}

pub(super) fn record_adaptive_candidate_decision(
    cassie: &Cassie,
    decision: AdaptiveCandidateDecision,
    final_candidate_count: usize,
    result_count: usize,
) {
    let expansions = if final_candidate_count > decision.initial_budget {
        final_candidate_count
            .saturating_sub(decision.initial_budget)
            .saturating_add(decision.initial_budget - 1)
            / decision.initial_budget
    } else {
        0
    };
    let exhausted = result_count < decision.initial_budget.min(final_candidate_count);
    cassie.runtime.record_adaptive_candidate_decision(
        decision.initial_budget,
        decision.feedback_budget,
        expansions,
        final_candidate_count,
        exhausted,
    );
}

struct VectorDistanceTopKSpec {
    collection: String,
    vector_field: String,
    query: Vec<f32>,
    id_column: String,
    score_column: String,
    direction: SortDirection,
    limit: usize,
    offset: usize,
}

fn vector_distance_top_k_spec(plan: &LogicalPlan) -> Option<VectorDistanceTopKSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.projection.len() != 2
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);

    let (id_column, function, score_column) =
        vector_distance_projection(plan.projection.as_slice())?;
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column) {
        return None;
    }

    let (vector_field, query) = vector_distance_args(function)?;
    Some(VectorDistanceTopKSpec {
        collection: collection.clone(),
        vector_field,
        query,
        id_column,
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
    })
}

fn vector_distance_projection(
    projection: &[SelectItem],
) -> Option<(String, &FunctionCall, String)> {
    let SelectItem::Column { name, alias: _ } = &projection[0] else {
        return None;
    };
    if !name.eq_ignore_ascii_case("id") && !name.eq_ignore_ascii_case("_id") {
        return None;
    }
    let SelectItem::Function { function, alias } = &projection[1] else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("vector_distance") {
        return None;
    }
    Some((
        alias.clone().unwrap_or_else(|| name.clone()),
        function,
        alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn order_matches_vector_distance_score(
    order: &crate::sql::ast::OrderExpr,
    function: &FunctionCall,
    score_column: &str,
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            order_function.name.eq_ignore_ascii_case("vector_distance")
                && vector_distance_args(order_function) == vector_distance_args(function)
        }
        _ => false,
    }
}

fn vector_distance_args(function: &FunctionCall) -> Option<(String, Vec<f32>)> {
    if function.args.len() != 2 {
        return None;
    }
    let Expr::Column(vector_field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((vector_field.clone(), parse_vector_literal(query)?))
}

pub(crate) fn parse_vector_literal(value: &str) -> Option<Vec<f32>> {
    let values = serde_json::from_str::<Vec<f32>>(value).ok()?;
    if values.is_empty() {
        return None;
    }
    Some(values)
}

pub(super) fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(value.as_f64()? as f32);
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq)]
struct SqlVectorCandidate {
    sort_value: f64,
    score: f64,
    id: String,
}

impl SqlVectorCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_sql_vector_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for SqlVectorCandidate {}

impl PartialOrd for SqlVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for SqlVectorCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_sql_vector_candidates(self, other)
    }
}

fn compare_sql_vector_candidates(
    left: &SqlVectorCandidate,
    right: &SqlVectorCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}
