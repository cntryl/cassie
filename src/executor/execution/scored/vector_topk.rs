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
    if spec.payload_id_column.is_some() {
        return execute_operator_vector_top_k(cassie, session, &schema, &spec, plan, controls);
    }
    let stored_vector_field = validate_vector_top_k_dimensions(&schema, &spec)?;
    if spec.limit == 0 {
        return Ok(Some(Vec::new()));
    }

    if session.is_some_and(|session| session.has_collection_changes(&spec.collection)) {
        diagnostics::record_transaction_overlay_exact_fallback(
            cassie,
            &spec,
            &stored_vector_field,
        )?;
    } else if plan.filter.is_none() {
        if let Some(rows) =
            hnsw::execute_hnsw_vector_top_k(cassie, session, &spec, &stored_vector_field, controls)?
        {
            return Ok(Some(rows));
        }
        if let Some(rows) = ivfflat::execute_ivfflat_vector_top_k(
            cassie,
            session,
            &spec,
            &stored_vector_field,
            controls,
        )? {
            return Ok(Some(rows));
        }
    } else {
        diagnostics::record_filtered_ann_exact_fallback(cassie, &spec, &stored_vector_field)?;
    }

    exact::execute_exact_vector_top_k(
        cassie,
        &spec,
        &exact::ExactVectorRequest {
            session,
            stored_vector_field: &stored_vector_field,
            user_functions,
            params,
            filter_expr: plan.filter.as_ref(),
            controls,
        },
        started_at,
    )
}

/// Serves `SELECT <column>, <vector> <-> <literal> ... ORDER BY <same> LIMIT n`
/// from an ANN index only. Without a usable index the generic scan path runs.
fn execute_operator_vector_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    schema: &crate::catalog::CollectionSchema,
    spec: &VectorDistanceTopKSpec,
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(column) = spec.payload_id_column.as_ref() else {
        return Ok(None);
    };
    let column_key = crate::sql::ColumnIdentifierPath::reference_field_key(column);
    let Some(column_type) = schema
        .fields
        .iter()
        .find(|field| crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == column_key)
        .map(|field| field.data_type.clone())
    else {
        return Ok(None);
    };
    let vector_key = crate::sql::ColumnIdentifierPath::reference_field_key(&spec.vector_field);
    let Some(field) = schema.fields.iter().find(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == vector_key
    }) else {
        return Ok(None);
    };
    let crate::types::DataType::Vector(Some(dimensions)) = &field.data_type else {
        return Ok(None);
    };
    if spec.query.len() != *dimensions
        || spec.limit == 0
        || plan.filter.is_some()
        || session.is_some_and(|session| session.has_collection_changes(&spec.collection))
    {
        return Ok(None);
    }
    let stored_vector_field = field.name.clone();
    let mut rows =
        hnsw::execute_hnsw_vector_top_k(cassie, session, spec, &stored_vector_field, controls)?;
    if rows.is_none() {
        rows = ivfflat::execute_ivfflat_vector_top_k(
            cassie,
            session,
            spec,
            &stored_vector_field,
            controls,
        )?;
    }
    let Some(rows) = rows else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (mut entries, aliases) = row.into_parts();
        let Some(id) = entries.first().and_then(|(_, value)| value.as_str()) else {
            return Ok(None);
        };
        let Some(document) = cassie.get_document_for_session(session, &spec.collection, id)? else {
            return Ok(None);
        };
        entries[0].1 = document.payload.get(column).map_or(Value::Null, |json| {
            crate::executor::scan::json_to_typed_value(json, &column_type)
        });
        out.push(BatchRow::with_aliases(entries, aliases));
    }
    Ok(Some(out))
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
) -> Result<String, QueryError> {
    let Some(field) = schema.fields.iter().find(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
            == crate::sql::ColumnIdentifierPath::reference_field_key(&spec.vector_field)
    }) else {
        return Err(QueryError::General(format!(
            "vector field '{}' does not exist on collection '{}'",
            spec.vector_field, spec.collection
        )));
    };
    let crate::types::DataType::Vector(Some(dimensions)) = &field.data_type else {
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
    Ok(field.name.clone())
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
    /// Set for the pgvector operator shape: the user column projected first,
    /// whose value is read from the document instead of the row identity.
    pub(super) payload_id_column: Option<String>,
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
    let limit = usize::try_from(plan.limit_value()?).ok()?;
    let offset = plan
        .offset_value()
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    if let Some(spec) = operator_top_k_spec(plan, collection, (limit, offset), params) {
        return Some(spec);
    }
    let (id_column, function, score_column) =
        vector_distance_projection(plan.projection.as_slice())?;
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column, params) {
        return None;
    }
    let (vector_field, query) = vector_distance_args(function, params)?;
    Some(VectorDistanceTopKSpec {
        collection: collection.to_string(),
        vector_field,
        query,
        id_column,
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
        payload_id_column: None,
    })
}

fn operator_top_k_spec(
    plan: &LogicalPlan,
    collection: &str,
    (limit, offset): (usize, usize),
    params: &[Value],
) -> Option<VectorDistanceTopKSpec> {
    let SelectItem::Column { name, alias } = &plan.projection[0] else {
        return None;
    };
    if crate::types::row_identity::is_row_identity_column(name) {
        return None;
    }
    let SelectItem::Expr {
        expr,
        alias: score_alias,
    } = &plan.projection[1]
    else {
        return None;
    };
    let (vector_field, query) = operator_l2_args(expr, params)?;
    let score_column = score_alias
        .clone()
        .unwrap_or_else(|| "?column?".to_string());
    let order_matches = match &plan.order[0].expr {
        Expr::Column(column) => {
            score_alias.is_some()
                && crate::sql::ColumnIdentifierPath::matches_stored_field(column, &score_column)
        }
        other => {
            operator_l2_args(other, params).as_ref() == Some(&(vector_field.clone(), query.clone()))
        }
    };
    if !order_matches {
        return None;
    }
    Some(VectorDistanceTopKSpec {
        collection: collection.to_string(),
        vector_field,
        query,
        id_column: alias.clone().unwrap_or_else(|| name.clone()),
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
        payload_id_column: Some(name.clone()),
    })
}

fn operator_l2_args(expr: &Expr, params: &[Value]) -> Option<(String, Vec<f32>)> {
    let Expr::Binary {
        left,
        op: crate::sql::ast::BinaryOp::PgvectorL2,
        right,
    } = expr
    else {
        return None;
    };
    let Expr::Column(vector_field) = left.as_ref() else {
        return None;
    };
    let mut literal = right.as_ref();
    while let Expr::Cast { expr, .. } = literal {
        literal = expr.as_ref();
    }
    let query = match literal {
        Expr::StringLiteral(query) => parse_vector_literal(query)?,
        Expr::Param(index) => match params.get(*index)? {
            Value::String(query) => parse_vector_literal(query)?,
            Value::Vector(query) => crate::vector::finite_f32_vector(&query.values)?,
            _ => return None,
        },
        _ => return None,
    };
    Some((vector_field.clone(), query))
}

fn vector_distance_projection(
    projection: &[SelectItem],
) -> Option<(String, &FunctionCall, String)> {
    let SelectItem::Column {
        name,
        alias: id_alias,
    } = &projection[0]
    else {
        return None;
    };
    if !crate::types::row_identity::is_row_identity_column(name) {
        return None;
    }
    let SelectItem::Function { function, alias } = &projection[1] else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("vector_distance") {
        return None;
    }
    Some((
        id_alias.clone().unwrap_or_else(|| name.clone()),
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
        Expr::Column(column) => {
            crate::sql::ColumnIdentifierPath::matches_stored_field(column, score_column)
        }
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
            Value::Vector(query) => crate::vector::finite_f32_vector(&query.values)?,
            _ => return None,
        },
        _ => return None,
    };
    Some((vector_field.clone(), query))
}

pub(crate) fn parse_vector_literal(value: &str) -> Option<Vec<f32>> {
    let values = serde_json::from_str::<Vec<f64>>(value).ok()?;
    if values.is_empty() {
        return None;
    }
    values
        .into_iter()
        .map(crate::vector::f64_to_finite_f32)
        .collect()
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
    crate::vector::f64_to_finite_f32(value)
        .ok_or_else(|| QueryError::General(format!("{context} is outside f32 range")))
}
