use super::{
    filter, scan, value_to_vector, vector_prefilter_supported, BatchRow, Cassie, CassieSession,
    Expr, FunctionCall, FunctionMeta, HashMap, LogicalPlan, QueryError, QuerySource, SelectItem,
    SortDirection, Value,
};
use crate::runtime::QueryExecutionControls;

#[path = "vector_topk/candidate.rs"]
mod candidate;
#[path = "vector_topk/diagnostics.rs"]
mod diagnostics;
#[path = "vector_topk/exact.rs"]
mod exact;
#[path = "vector_topk/hnsw.rs"]
mod hnsw;
#[path = "vector_topk/ivfflat.rs"]
mod ivfflat;

pub(crate) use diagnostics::install_ann_rerank_barriers;

pub(crate) fn execute_vector_distance_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    super::super::check_timeout(controls)?;
    let Some(spec) = vector_distance_top_k_spec(plan, params) else {
        return Ok(None);
    };
    let started_at = std::time::Instant::now();
    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    validate_vector_top_k_dimensions(&schema, &spec)?;

    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        diagnostics::record_transaction_overlay_exact_fallback(cassie, &spec)?;
    } else if plan.filter.is_none() {
        if let Some(rows) = hnsw::execute_hnsw_vector_top_k(cassie, session, &spec, controls)? {
            return Ok(Some(rows));
        }
        if let Some(rows) = ivfflat::execute_ivfflat_vector_top_k(cassie, session, &spec, controls)?
        {
            return Ok(Some(rows));
        }
    } else {
        diagnostics::record_filtered_ann_exact_fallback(cassie, &spec)?;
    }

    exact::execute_exact_vector_top_k(
        cassie,
        &spec,
        &exact::ExactVectorRequest {
            session,
            user_functions,
            params,
            filter_expr: plan.filter.as_ref(),
            controls,
        },
        started_at,
    )
}

const ANN_CANDIDATE_OVERSAMPLE: usize = 64;

pub(super) struct AdaptiveCandidateDecision {
    initial_budget: usize,
    feedback_budget: Option<usize>,
}

impl AdaptiveCandidateDecision {
    pub(super) fn ann_candidate_budget(&self, max_budget: usize) -> usize {
        self.initial_budget
            .saturating_mul(ANN_CANDIDATE_OVERSAMPLE)
            .min(max_budget.max(1))
    }
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
    decision: &AdaptiveCandidateDecision,
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

fn validate_vector_top_k_dimensions(
    schema: &crate::catalog::CollectionSchema,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    let Some(field) = schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(&spec.vector_field))
    else {
        return Err(QueryError::General(format!(
            "vector field '{}' does not exist on collection '{}'",
            spec.vector_field, spec.collection
        )));
    };
    let crate::types::DataType::Vector(dimensions) = &field.data_type else {
        return Err(QueryError::General(format!(
            "field '{}' on collection '{}' is not a vector field",
            spec.vector_field, spec.collection
        )));
    };
    if spec.query.len() != *dimensions {
        return Err(QueryError::General(format!(
            "vector_distance query for field '{}' on collection '{}' expects {} dimensions but received {}",
            spec.vector_field,
            spec.collection,
            dimensions,
            spec.query.len()
        )));
    }
    Ok(())
}

pub(super) struct VectorDistanceTopKSpec {
    pub(super) collection: String,
    pub(super) vector_field: String,
    pub(super) query: Vec<f32>,
    pub(super) id_column: String,
    pub(super) score_column: String,
    pub(super) direction: SortDirection,
    pub(super) limit: usize,
    pub(super) offset: usize,
}

impl VectorDistanceTopKSpec {
    pub(super) fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

fn vector_distance_top_k_spec(
    plan: &LogicalPlan,
    params: &[Value],
) -> Option<VectorDistanceTopKSpec> {
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
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column, params) {
        return None;
    }
    let (vector_field, query) = vector_distance_args(function, params)?;
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
    params: &[Value],
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            order_function.name.eq_ignore_ascii_case("vector_distance")
                && vector_distance_args(order_function, params)
                    == vector_distance_args(function, params)
        }
        _ => false,
    }
}

fn vector_distance_args(function: &FunctionCall, params: &[Value]) -> Option<(String, Vec<f32>)> {
    if function.args.len() != 2 {
        return None;
    }
    let Expr::Column(vector_field) = &function.args[0] else {
        return None;
    };
    let query = match &function.args[1] {
        Expr::StringLiteral(query) => parse_vector_literal(query)?,
        Expr::Param(index) => match params.get(*index)? {
            Value::String(query) => parse_vector_literal(query)?,
            Value::Vector(query) => query.values.clone(),
            _ => return None,
        },
        _ => return None,
    };
    Some((vector_field.clone(), query))
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
        out.push(finite_f32(value.as_f64()?, "vector element").ok()?);
    }
    Some(out)
}

fn finite_f32(value: f64, context: &str) -> Result<f32, QueryError> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(QueryError::General(format!(
            "{context} is outside f32 range"
        )));
    }
    value
        .to_string()
        .parse::<f32>()
        .map_err(|_| QueryError::General(format!("failed to parse {context} as f32")))
}
