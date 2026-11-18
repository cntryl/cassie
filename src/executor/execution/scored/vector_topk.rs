use super::{
    filter, scan, value_to_vector, vector_prefilter_supported, BatchRow, Cassie, CassieSession,
    Expr, FunctionCall, FunctionMeta, HashMap, LogicalPlan, QueryError, QuerySource, SelectItem,
    SortDirection, Value,
};
use crate::runtime::{
    owned_memory::try_reserve_vec, QueryExecutionControls, QueryMemoryReservation,
};
use crate::types::DataType;
use crate::vector::text::visit_components_with_dimension;

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
    let Some(spec) = vector_distance_top_k_spec(plan, params, controls)? else {
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
    _query_memory: Option<QueryMemoryReservation>,
}

impl VectorDistanceTopKSpec {
    pub(super) fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

fn vector_distance_top_k_spec(
    plan: &LogicalPlan,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<VectorDistanceTopKSpec>, QueryError> {
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
        return Ok(None);
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    let Some(limit) = plan
        .limit_value()
        .and_then(|limit| usize::try_from(limit).ok())
    else {
        return Ok(None);
    };
    let offset = plan
        .offset_value()
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    if let Some(spec) = operator_top_k_spec(plan, collection, (limit, offset), params, controls)? {
        return Ok(Some(spec));
    }
    let Some((id_column, function, score_column)) =
        vector_distance_projection(plan.projection.as_slice())
    else {
        return Ok(None);
    };
    let Some((vector_field, operand)) = vector_distance_args(function) else {
        return Ok(None);
    };
    let Some(order) = vector_order_operand(
        &plan.order[0].expr,
        Some(&score_column),
        vector_field,
        |expr| match expr {
            Expr::Function(function) if function.name.eq_ignore_ascii_case("vector_distance") => {
                vector_distance_args(function)
            }
            _ => None,
        },
    ) else {
        return Ok(None);
    };
    let Some(query) = matching_vector_query(operand, order, params, controls)? else {
        return Ok(None);
    };
    Ok(Some(VectorDistanceTopKSpec {
        collection: collection.to_string(),
        vector_field: vector_field.to_owned(),
        query: query.values,
        id_column,
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
        payload_id_column: None,
        _query_memory: query.memory,
    }))
}

fn operator_top_k_spec(
    plan: &LogicalPlan,
    collection: &str,
    (limit, offset): (usize, usize),
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<VectorDistanceTopKSpec>, QueryError> {
    let SelectItem::Column { name, alias } = &plan.projection[0] else {
        return Ok(None);
    };
    if limit == 0 || crate::types::row_identity::is_row_identity_column(name) {
        return Ok(None);
    }
    let SelectItem::Expr {
        expr,
        alias: score_alias,
    } = &plan.projection[1]
    else {
        return Ok(None);
    };
    let Some((vector_field, operand)) = operator_l2_args(expr) else {
        return Ok(None);
    };
    let Some(order) = vector_order_operand(
        &plan.order[0].expr,
        score_alias.as_deref(),
        vector_field,
        operator_l2_args,
    ) else {
        return Ok(None);
    };
    let Some(query) = matching_vector_query(operand, order, params, controls)? else {
        return Ok(None);
    };
    let score_column = score_alias
        .clone()
        .unwrap_or_else(|| "?column?".to_string());
    Ok(Some(VectorDistanceTopKSpec {
        collection: collection.to_string(),
        vector_field: vector_field.to_owned(),
        query: query.values,
        id_column: alias.clone().unwrap_or_else(|| name.clone()),
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
        payload_id_column: Some(name.clone()),
        _query_memory: query.memory,
    }))
}

fn operator_l2_args(expr: &Expr) -> Option<(&str, &Expr)> {
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
    Some((vector_field, right))
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

enum VectorOrder<'a> {
    ScoreColumn,
    QueryOperand(&'a Expr),
}

fn vector_order_operand<'a>(
    order: &'a Expr,
    score_column: Option<&str>,
    vector_field: &str,
    order_args: impl FnOnce(&'a Expr) -> Option<(&'a str, &'a Expr)>,
) -> Option<VectorOrder<'a>> {
    if let Expr::Column(column) = order {
        return crate::sql::ColumnIdentifierPath::matches_stored_field(column, score_column?)
            .then_some(VectorOrder::ScoreColumn);
    }
    let (order_field, operand) = order_args(order)?;
    (order_field == vector_field).then_some(VectorOrder::QueryOperand(operand))
}

fn matching_vector_query(
    operand: &Expr,
    order: VectorOrder<'_>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<VectorQuery>, QueryError> {
    // Validate both explicit casts before either allocation so a failed cast
    // can keep its scalar diagnostic even when native admission is tight.
    let Some(query) = vector_query_input(operand, params, controls)? else {
        return Ok(None);
    };
    let order_query = match order {
        VectorOrder::ScoreColumn => None,
        VectorOrder::QueryOperand(operand) => {
            let Some(input) = vector_query_input(operand, params, controls)? else {
                return Ok(None);
            };
            Some(input)
        }
    };
    let Some(query) = resolve_vector_query(query, controls)? else {
        return Ok(None);
    };
    if let Some(order_query) = order_query {
        let Some(order_query) = resolve_vector_query(order_query, controls)? else {
            return Ok(None);
        };
        if order_query.values != query.values {
            return Ok(None);
        }
    }
    Ok(Some(query))
}

fn vector_distance_args(function: &FunctionCall) -> Option<(&str, &Expr)> {
    let [Expr::Column(vector_field), operand] = function.args.as_slice() else {
        return None;
    };
    Some((vector_field, operand))
}

struct VectorQuery {
    values: Vec<f32>,
    memory: Option<QueryMemoryReservation>,
}

enum VectorQueryInput<'a> {
    UncastText(&'a str),
    UncastNative(&'a [f32]),
    CastText(&'a str, usize),
    CastNative(&'a [f32]),
}

fn vector_query_input<'a>(
    mut operand: &'a Expr,
    params: &'a [Value],
    controls: &QueryExecutionControls,
) -> Result<Option<VectorQueryInput<'a>>, QueryError> {
    let explicit_cast = matches!(operand, Expr::Cast { .. });
    let mut expected_dimensions = None;
    // Only vector casts preserve the operand. Enforce every fixed dimension
    // in a cast chain and leave failed casts to the generic evaluator.
    while let Expr::Cast {
        expr, data_type, ..
    } = operand
    {
        let DataType::Vector(dimensions) = data_type else {
            return Ok(None);
        };
        if let Some(dimensions) = *dimensions {
            if expected_dimensions.is_some_and(|expected| expected != dimensions) {
                return Ok(None);
            }
            expected_dimensions = Some(dimensions);
        }
        operand = expr;
    }
    let text = match operand {
        Expr::StringLiteral(query) => query,
        Expr::Param(index) => match params.get(*index) {
            Some(Value::String(query)) => query,
            Some(Value::Vector(query)) => {
                if !explicit_cast {
                    return Ok(Some(VectorQueryInput::UncastNative(&query.values)));
                }
                let dimensions = query.dimension();
                if !(1..=16_000).contains(&dimensions)
                    || expected_dimensions.is_some_and(|expected| expected != dimensions)
                    || !query.values.iter().all(|value| value.is_finite())
                {
                    return Ok(None);
                }
                return Ok(Some(VectorQueryInput::CastNative(&query.values)));
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    if !explicit_cast {
        return Ok(Some(VectorQueryInput::UncastText(text)));
    }
    controls.check_interruption()?;
    let Ok(dimensions) = visit_components_with_dimension(text, expected_dimensions, |_| {}) else {
        return Ok(None);
    };
    controls.check_interruption()?;
    Ok(Some(VectorQueryInput::CastText(text, dimensions)))
}

fn resolve_vector_query(
    input: VectorQueryInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<VectorQuery>, QueryError> {
    let admission = || controls.reserve_query_memory(0);
    let values = match input {
        VectorQueryInput::UncastText(text) => parse_vector_literal(text),
        VectorQueryInput::UncastNative(source) => crate::vector::finite_f32_vector(source),
        VectorQueryInput::CastText(text, dimensions) => {
            controls.check_interruption()?;
            let (mut values, memory) = try_reserve_vec(dimensions, &admission()?)?;
            if visit_components_with_dimension(text, Some(dimensions), |value| values.push(value))
                .is_err()
            {
                return Ok(None);
            }
            controls.check_interruption()?;
            return Ok(Some(VectorQuery {
                values,
                memory: Some(memory),
            }));
        }
        VectorQueryInput::CastNative(source) => {
            controls.check_interruption()?;
            let (mut values, memory) = try_reserve_vec(source.len(), &admission()?)?;
            values.extend_from_slice(source);
            return Ok(Some(VectorQuery {
                values,
                memory: Some(memory),
            }));
        }
    };
    Ok(values.map(|values| VectorQuery {
        values,
        memory: None,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::sql::ast::{CastIdentity, OrderExpr};
    use crate::types::Vector;
    use std::time::Instant;

    #[test]
    fn should_release_order_scratch_after_matching_cast_vectors() {
        // Arrange
        let distance = |operand| FunctionCall {
            name: "vector_distance".into(),
            args: vec![
                Expr::Column("embedding".into()),
                Expr::Cast {
                    expr: Box::new(operand),
                    data_type: DataType::Vector(Some(3)),
                    source_type: None,
                    identity: CastIdentity::Scalar,
                },
            ],
            ..FunctionCall::default()
        };
        let plan = LogicalPlan {
            command: None,
            source: QuerySource::Collection("vectors".into()),
            collection: "vectors".into(),
            ctes: vec![],
            distinct: false,
            distinct_on: vec![],
            projection: vec![
                SelectItem::Column {
                    name: "_id".into(),
                    alias: None,
                },
                SelectItem::Function {
                    function: distance(Expr::Param(0)),
                    alias: Some("score".into()),
                },
            ],
            filter: None,
            group_by: vec![],
            having: None,
            order: vec![OrderExpr {
                expr: Expr::Function(distance(Expr::StringLiteral("[1,0,0]".into()))),
                direction: SortDirection::Asc,
                nulls: None,
            }],
            limit: Some(Expr::IntegerLiteral(1)),
            offset: None,
            set: None,
        };
        let params = [Value::Vector(Vector::new(vec![1.0, 0.0, 0.0]))];
        let vector_bytes = 3 * std::mem::size_of::<f32>();
        let controls = |budget| {
            QueryExecutionControls::from_limits(
                &CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..CassieRuntimeLimits::default()
                },
                Instant::now(),
            )
        };
        let just_below = controls(2 * vector_bytes - 1);
        let exact = controls(2 * vector_bytes);
        let invalid = controls(vector_bytes - 1);
        let mut invalid_order = plan.clone();
        invalid_order.order[0].expr = Expr::Function(distance(Expr::StringLiteral("[1,0]".into())));

        // Act
        let rejected = vector_distance_top_k_spec(&plan, &params, &just_below);
        let admitted = vector_distance_top_k_spec(&plan, &params, &exact)
            .expect("admit exactly two query-vector buffers")
            .expect("select native vector top-k");
        let declined = vector_distance_top_k_spec(&invalid_order, &params, &invalid)
            .expect("leave invalid ORDER BY casts to scalar evaluation");

        // Assert
        assert!(matches!(
            rejected.map_err(CassieError::from),
            Err(CassieError::ResourceLimit(_))
        ));
        assert_eq!(just_below.current_query_memory_bytes(), 0);
        assert_eq!(exact.peak_query_memory_bytes(), 2 * vector_bytes);
        assert_eq!(exact.current_query_memory_bytes(), vector_bytes);
        assert_eq!(admitted.query, vec![1.0, 0.0, 0.0]);
        assert!(declined.is_none());
        assert_eq!(invalid.peak_query_memory_bytes(), 0);
        drop(admitted);
        assert_eq!(exact.current_query_memory_bytes(), 0);
    }
}
