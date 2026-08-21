use super::{
    batch, check_timeout, projected_read, scan, BatchRow, BinaryOp, Cassie, CassieSession, Expr,
    FunctionMeta, HashMap, LogicalPlan, PhysicalPlan, QueryError, QueryExecutionControls,
    QuerySource, SelectItem, Value,
};
use crate::catalog::IndexMeta;
use crate::midge::adapter::{DocumentRef, ScalarIndexBound, ScalarIndexScanRequest};
use crate::planner::physical::{scalar_index_plan_shape, ScalarIndexPlanPath};
use crate::types::semantic::compare_values;
use crate::types::DataType;
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub(super) fn execute_scalar_index_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    physical: Option<&PhysicalPlan>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = scalar_index_read_spec(cassie, session, physical, plan, params)? else {
        return Ok(None);
    };

    if spec.request.limit == Some(0) {
        return Ok(Some(Vec::new()));
    }
    if matches!(
        spec.predicate_resolution,
        ScalarIndexPredicateResolution::Unsatisfiable
    ) {
        return Ok(Some(Vec::new()));
    }

    let hits = cassie
        .midge
        .scan_scalar_index_controlled(&spec.index, &spec.request, controls)
        .map_err(QueryError::from)?;
    let (hits, _hits_memory) = hits.into_parts();
    let schema = cassie.catalog.get_schema(&spec.collection);
    let mut rows = Vec::with_capacity(hits.len());

    for hit in hits {
        check_timeout(controls)?;
        let document = if spec.covered {
            DocumentRef {
                id: hit.id,
                payload: serde_json::Value::Object(hit.fields),
            }
        } else {
            let Some(document) = cassie
                .get_document_for_session(session, &spec.collection, &hit.id)
                .map_err(|error| QueryError::General(error.to_string()))?
            else {
                return Ok(None);
            };
            document
        };
        rows.push(scan::projected_document_to_row(
            document,
            &spec.scan_fields,
            schema.as_ref(),
        ));
    }

    record_scalar_index_read_path(cassie, &spec, rows.len());

    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    let index_usage = if spec.covered {
        projected_read::ProjectedReadIndexUsage::CoveringScalarIndex
    } else {
        projected_read::ProjectedReadIndexUsage::SelectedScalarIndexFallback
    };
    let rows = projected_read::finalize_projected_filtered_read_with_index_usage(
        projected_read::ProjectedReadFinalization {
            cassie,
            session,
            plan,
            user_functions,
            params,
            controls,
            apply_filter: matches!(
                spec.predicate_resolution,
                ScalarIndexPredicateResolution::Residual
            ),
            apply_sort: !spec.sort_applied,
            index_usage: Some(index_usage),
        },
        &mut batches,
    )?;
    Ok(Some(rows))
}

#[derive(Debug, Clone)]
struct ScalarIndexReadSpec {
    collection: String,
    index: IndexMeta,
    scan_fields: Vec<String>,
    request: ScalarIndexScanRequest,
    path: ScalarIndexPlanPath,
    covered: bool,
    sort_applied: bool,
    predicate_resolution: ScalarIndexPredicateResolution,
}

#[derive(Debug, Clone, Copy)]
enum ScalarIndexPredicateResolution {
    Unsatisfiable,
    Exact,
    Residual,
}

fn scalar_index_read_spec(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    physical: Option<&PhysicalPlan>,
    plan: &LogicalPlan,
    params: &[Value],
) -> Result<Option<ScalarIndexReadSpec>, QueryError> {
    let Some(projected) = projected_read::projected_filtered_read_spec(plan)
        .or_else(|| expression_index_read_spec(plan))
    else {
        return Ok(None);
    };
    if session.is_some_and(|session| !session.collection_changes(&projected.collection).is_empty())
    {
        return Ok(None);
    }

    let indexes = cassie.catalog.list_indexes(&projected.collection);
    let physical = physical.filter(|physical| physical.collection == projected.collection);
    let (index_name, covered_index) = if let Some(physical) = physical {
        let Some(index_name) = physical.read.selected_index.as_deref() else {
            return Ok(None);
        };
        (index_name.to_string(), physical.read.covered_index)
    } else {
        let cardinality_stats =
            std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
        let physical = crate::planner::physical::build_with_indexes(
            plan.clone(),
            indexes.as_slice(),
            &cardinality_stats,
        );
        let Some(index_name) = physical.read.selected_index else {
            return Ok(None);
        };
        (index_name, physical.read.covered_index)
    };
    let Some(index) = indexes.into_iter().find(|index| index.name == index_name) else {
        return Ok(None);
    };
    let Some(shape) = scalar_index_plan_shape(plan, &index) else {
        return Ok(None);
    };

    let extracted_constraints = if index.expressions.is_empty() {
        concrete_constraints(plan.filter.as_ref(), params)
            .map(|constraints| (constraints, BTreeMap::new()))
    } else {
        expression_index_constraints(plan.filter.as_ref(), params)
            .map(|constraints| (constraints.fields, constraints.expressions))
    };
    let (mut constraints, expression_constraints) = extracted_constraints
        .ok_or_else(|| QueryError::General("unsupported scalar index filter".to_string()))?;
    canonicalize_field_constraints(cassie, &projected.collection, &mut constraints);
    let equality_prefix =
        scalar_index_equality_prefix(&index, &shape, &constraints, &expression_constraints)?;
    let range_constraint =
        range_constraint_for_shape(&index, &shape, &constraints, &expression_constraints);
    let lower_bound = range_constraint
        .and_then(|constraint| constraint.lower.clone())
        .map(|bound| ScalarIndexBound {
            value: bound.value,
            inclusive: bound.inclusive,
        });
    let upper_bound = range_constraint
        .and_then(|constraint| constraint.upper.clone())
        .map(|bound| ScalarIndexBound {
            value: bound.value,
            inclusive: bound.inclusive,
        });
    let bounds_are_exact =
        scalar_index_bounds_are_exact(&index, &shape, &constraints, &expression_constraints);
    let request = ScalarIndexScanRequest {
        equality_prefix,
        lower_bound,
        upper_bound,
        reverse: shape.reverse,
        limit: storage_limit(plan, &shape, bounds_are_exact),
    };
    let unsatisfiable = constraints
        .values()
        .chain(expression_constraints.values())
        .any(|constraint| constraint.unsatisfiable);
    let predicate_resolution = if unsatisfiable {
        ScalarIndexPredicateResolution::Unsatisfiable
    } else if bounds_are_exact {
        ScalarIndexPredicateResolution::Exact
    } else {
        ScalarIndexPredicateResolution::Residual
    };

    Ok(Some(ScalarIndexReadSpec {
        collection: projected.collection,
        index,
        scan_fields: projected.scan_fields,
        request,
        path: shape.path,
        covered: covered_index,
        sort_applied: plan.order.is_empty() || shape.order_satisfied,
        predicate_resolution,
    }))
}

fn range_constraint_for_shape<'a>(
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    field_constraints: &'a BTreeMap<String, ConcreteConstraint>,
    expression_constraints: &'a BTreeMap<String, ConcreteConstraint>,
) -> Option<&'a ConcreteConstraint> {
    let range_index = shape.range_field_index?;
    let fields = index.normalized_fields();
    if range_index < fields.len() {
        return field_constraints.get(&fields[range_index].to_ascii_lowercase());
    }

    let expression_index = range_index.checked_sub(fields.len())?;
    let expressions = index.normalized_expressions();
    let expression = expressions.get(expression_index)?;
    expression_constraints.get(expression)
}

fn scalar_index_bounds_are_exact(
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    field_constraints: &BTreeMap<String, ConcreteConstraint>,
    expression_constraints: &BTreeMap<String, ConcreteConstraint>,
) -> bool {
    let fields = index.normalized_fields();
    let expressions = index.normalized_expressions();
    let fields_are_represented = field_constraints.keys().all(|constraint| {
        fields
            .iter()
            .position(|field| field.eq_ignore_ascii_case(constraint))
            .is_some_and(|position| constraint_position_is_represented(position, shape))
    });
    let expressions_are_represented = expression_constraints.keys().all(|constraint| {
        expressions
            .iter()
            .position(|expression| expression == constraint)
            .map(|position| fields.len() + position)
            .is_some_and(|position| constraint_position_is_represented(position, shape))
    });
    fields_are_represented && expressions_are_represented
}

fn constraint_position_is_represented(
    position: usize,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
) -> bool {
    position < shape.equality_prefix_len || shape.range_field_index == Some(position)
}

fn expression_index_read_spec(
    plan: &LogicalPlan,
) -> Option<projected_read::ProjectedFilteredReadSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let projection_columns = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection_columns.is_empty() {
        return None;
    }

    let mut scan_fields = projection_columns
        .into_iter()
        .filter(|column| !projected_read::is_row_id_column(column))
        .collect::<Vec<_>>();
    if let Some(filter) = plan.filter.as_ref() {
        collect_expression_columns(filter, &mut scan_fields);
    }
    Some(projected_read::ProjectedFilteredReadSpec {
        collection: collection.clone(),
        scan_fields,
        scan_limit: None,
    })
}

fn collect_expression_columns(expr: &Expr, fields: &mut Vec<String>) {
    match expr {
        Expr::Column(name) => {
            if !projected_read::is_row_id_column(name)
                && !fields.iter().any(|field| field.eq_ignore_ascii_case(name))
            {
                fields.push(name.clone());
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expression_columns(left, fields);
            collect_expression_columns(right, fields);
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_expression_columns(expr, fields);
        }
        Expr::InList { expr, values, .. } => {
            collect_expression_columns(expr, fields);
            for value in values {
                collect_expression_columns(value, fields);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_expression_columns(expr, fields);
            collect_expression_columns(low, fields);
            collect_expression_columns(high, fields);
        }
        Expr::Function(function) => {
            for argument in &function.args {
                collect_expression_columns(argument, fields);
            }
        }
        Expr::Exists(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => {}
    }
}

fn scalar_index_equality_prefix(
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    constraints: &BTreeMap<String, ConcreteConstraint>,
    expression_constraints: &BTreeMap<String, ConcreteConstraint>,
) -> Result<Vec<serde_json::Value>, QueryError> {
    let fields = index.normalized_fields();
    let expressions = index.normalized_expressions();
    let key_count = fields.len() + expressions.len();
    if shape.equality_prefix_len > key_count {
        return Err(QueryError::General(format!(
            "scalar index '{}' equality prefix exceeds key width",
            index.name
        )));
    }

    let mut equality_prefix = Vec::with_capacity(shape.equality_prefix_len);
    let field_prefix_len = shape.equality_prefix_len.min(fields.len());
    for field in fields.iter().take(field_prefix_len) {
        let value = constraints
            .get(&field.to_ascii_lowercase())
            .and_then(|constraint| constraint.equality.clone())
            .ok_or_else(|| QueryError::General(format!("missing equality bound for '{field}'")))?;
        equality_prefix.push(value);
    }

    let expression_prefix_len = shape.equality_prefix_len.saturating_sub(fields.len());
    for expression in expressions.iter().take(expression_prefix_len) {
        let value = expression_constraints
            .get(expression)
            .and_then(|constraint| constraint.equality.clone())
            .ok_or_else(|| QueryError::General("missing expression equality bound".to_string()))?;
        equality_prefix.push(value);
    }

    Ok(equality_prefix)
}

fn storage_limit(
    plan: &LogicalPlan,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    bounds_are_exact: bool,
) -> Option<usize> {
    if (!plan.order.is_empty() && !shape.order_satisfied)
        || (plan.filter.is_some() && !bounds_are_exact)
    {
        return None;
    }

    let limit = usize::try_from(plan.limit?.max(0)).ok()?;
    let offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

#[derive(Debug, Clone, Default)]
struct ConcreteConstraint {
    equality: Option<serde_json::Value>,
    lower: Option<ConcreteBound>,
    upper: Option<ConcreteBound>,
    unsatisfiable: bool,
}

#[derive(Debug, Clone)]
struct ConcreteBound {
    value: serde_json::Value,
    inclusive: bool,
}

fn canonicalize_field_constraints(
    cassie: &Cassie,
    collection: &str,
    constraints: &mut BTreeMap<String, ConcreteConstraint>,
) {
    for (field, constraint) in constraints {
        if !matches!(
            cassie.catalog.field_type(collection, field),
            Some(DataType::Float)
        ) {
            continue;
        }
        if let Some(value) = constraint.equality.as_mut() {
            canonicalize_float_number(value);
        }
        if let Some(bound) = constraint.lower.as_mut() {
            canonicalize_float_number(&mut bound.value);
        }
        if let Some(bound) = constraint.upper.as_mut() {
            canonicalize_float_number(&mut bound.value);
        }
        refresh_constraint_satisfiability(constraint);
    }
}

fn canonicalize_float_number(value: &mut serde_json::Value) {
    let serde_json::Value::Number(number) = value else {
        return;
    };
    let Some(number) = number.as_f64().and_then(serde_json::Number::from_f64) else {
        return;
    };
    *value = serde_json::Value::Number(number);
}

fn intersect_lower_bound(
    bound: &mut Option<ConcreteBound>,
    value: serde_json::Value,
    inclusive: bool,
) {
    intersect_bound(bound, ConcreteBound { value, inclusive }, Ordering::Greater);
}

fn intersect_upper_bound(
    bound: &mut Option<ConcreteBound>,
    value: serde_json::Value,
    inclusive: bool,
) {
    intersect_bound(bound, ConcreteBound { value, inclusive }, Ordering::Less);
}

fn intersect_constraint(
    constraint: &mut ConcreteConstraint,
    op: &BinaryOp,
    value: serde_json::Value,
) -> Option<()> {
    match op {
        BinaryOp::Eq => {
            if constraint
                .equality
                .as_ref()
                .is_some_and(|current| compare_json_values(current, &value) != Ordering::Equal)
            {
                constraint.unsatisfiable = true;
            } else if constraint.equality.is_none() {
                constraint.equality = Some(value);
            }
        }
        BinaryOp::Gt => intersect_lower_bound(&mut constraint.lower, value, false),
        BinaryOp::Gte => intersect_lower_bound(&mut constraint.lower, value, true),
        BinaryOp::Lt => intersect_upper_bound(&mut constraint.upper, value, false),
        BinaryOp::Lte => intersect_upper_bound(&mut constraint.upper, value, true),
        _ => return None,
    }
    refresh_constraint_satisfiability(constraint);
    Some(())
}

fn refresh_constraint_satisfiability(constraint: &mut ConcreteConstraint) {
    constraint.unsatisfiable |= constraint
        .equality
        .as_ref()
        .is_some_and(|equality| !equality_satisfies_bounds(equality, constraint));
    constraint.unsatisfiable |= match (&constraint.lower, &constraint.upper) {
        (Some(lower), Some(upper)) => match compare_json_values(&lower.value, &upper.value) {
            Ordering::Greater => true,
            Ordering::Equal => !lower.inclusive || !upper.inclusive,
            Ordering::Less => false,
        },
        _ => false,
    };
}

fn equality_satisfies_bounds(
    equality: &serde_json::Value,
    constraint: &ConcreteConstraint,
) -> bool {
    let satisfies_lower = constraint.lower.as_ref().is_none_or(|lower| {
        let ordering = compare_json_values(equality, &lower.value);
        ordering == Ordering::Greater || ordering == Ordering::Equal && lower.inclusive
    });
    let satisfies_upper = constraint.upper.as_ref().is_none_or(|upper| {
        let ordering = compare_json_values(equality, &upper.value);
        ordering == Ordering::Less || ordering == Ordering::Equal && upper.inclusive
    });
    satisfies_lower && satisfies_upper
}

fn intersect_bound(
    bound: &mut Option<ConcreteBound>,
    candidate: ConcreteBound,
    tighter_ordering: Ordering,
) {
    let replace = bound.as_ref().is_none_or(|current| {
        let ordering = compare_json_values(&candidate.value, &current.value);
        ordering == tighter_ordering
            || ordering == Ordering::Equal && !candidate.inclusive && current.inclusive
    });
    if replace {
        *bound = Some(candidate);
    }
}

fn compare_json_values(left: &serde_json::Value, right: &serde_json::Value) -> Ordering {
    compare_values(&concrete_bound_value(left), &concrete_bound_value(right))
}

fn concrete_bound_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::Int64(value)
            } else if let Some(value) = value.as_f64() {
                Value::Float64(value)
            } else {
                Value::Json(serde_json::Value::Number(value.clone()))
            }
        }
        serde_json::Value::String(value) => Value::String(value.clone()),
        value => Value::Json(value.clone()),
    }
}

fn concrete_constraints(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<BTreeMap<String, ConcreteConstraint>> {
    let mut constraints = BTreeMap::new();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_concrete_constraints(filter, params, &mut constraints)?;
    Some(constraints)
}

#[derive(Default)]
struct ExpressionIndexConstraints {
    fields: BTreeMap<String, ConcreteConstraint>,
    expressions: BTreeMap<String, ConcreteConstraint>,
}

fn expression_index_constraints(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<ExpressionIndexConstraints> {
    let mut constraints = ExpressionIndexConstraints::default();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_expression_index_constraints(filter, params, &mut constraints)?;
    Some(constraints)
}

fn collect_expression_index_constraints(
    expr: &Expr,
    params: &[Value],
    constraints: &mut ExpressionIndexConstraints,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_expression_index_constraints(left, params, constraints)?;
            collect_expression_index_constraints(right, params, constraints)
        }
        Expr::Binary { left, op, right } => {
            if let Some((field, op, value)) = concrete_constraint(left, op, right, params) {
                return intersect_constraint(
                    constraints.fields.entry(field).or_default(),
                    &op,
                    value,
                );
            }
            let (expression, op, value) = concrete_expression_constraint(left, op, right, params)?;
            intersect_constraint(
                constraints.expressions.entry(expression).or_default(),
                &op,
                value,
            )
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if expr_has_column(expr) && !matches!(expr.as_ref(), Expr::Column(_)) => {
            let constraint = constraints
                .expressions
                .entry(serde_json::to_string(expr.as_ref()).ok()?)
                .or_default();
            intersect_constraint(constraint, &BinaryOp::Gte, expr_to_json(low, params)?)?;
            intersect_constraint(constraint, &BinaryOp::Lte, expr_to_json(high, params)?)
        }
        _ => None,
    }
}

fn concrete_expression_constraint(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    params: &[Value],
) -> Option<(String, BinaryOp, serde_json::Value)> {
    match (left, right) {
        (expr, value) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => Some((
            serde_json::to_string(expr).ok()?,
            op.clone(),
            expr_to_json(value, params)?,
        )),
        (value, expr) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => Some((
            serde_json::to_string(expr).ok()?,
            reverse_binary_op(op)?,
            expr_to_json(value, params)?,
        )),
        _ => None,
    }
}

fn expr_has_column(expr: &Expr) -> bool {
    match expr {
        Expr::Column(_) => true,
        Expr::Binary { left, right, .. } => expr_has_column(left) || expr_has_column(right),
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_has_column(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_has_column(expr) || values.iter().any(expr_has_column)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_has_column(expr) || expr_has_column(low) || expr_has_column(high),
        Expr::Function(function) => function.args.iter().any(expr_has_column),
        Expr::Exists(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => false,
    }
}

fn collect_concrete_constraints(
    expr: &Expr,
    params: &[Value],
    constraints: &mut BTreeMap<String, ConcreteConstraint>,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_concrete_constraints(left, params, constraints)?;
            collect_concrete_constraints(right, params, constraints)
        }
        Expr::Binary { left, op, right } => {
            let (field, op, value) = concrete_constraint(left, op, right, params)?;
            let entry = constraints.entry(field).or_default();
            intersect_constraint(entry, &op, value)
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            let entry = constraints.entry(field.to_ascii_lowercase()).or_default();
            intersect_constraint(entry, &BinaryOp::Gte, expr_to_json(low, params)?)?;
            intersect_constraint(entry, &BinaryOp::Lte, expr_to_json(high, params)?)
        }
        _ => None,
    }
}

fn concrete_constraint(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    params: &[Value],
) -> Option<(String, BinaryOp, serde_json::Value)> {
    match (left, right) {
        (Expr::Column(field), other) => Some((
            field.to_ascii_lowercase(),
            op.clone(),
            expr_to_json(other, params)?,
        )),
        (other, Expr::Column(field)) => Some((
            field.to_ascii_lowercase(),
            reverse_binary_op(op)?,
            expr_to_json(other, params)?,
        )),
        _ => None,
    }
}

fn reverse_binary_op(op: &BinaryOp) -> Option<BinaryOp> {
    match op {
        BinaryOp::Eq => Some(BinaryOp::Eq),
        BinaryOp::Gt => Some(BinaryOp::Lt),
        BinaryOp::Gte => Some(BinaryOp::Lte),
        BinaryOp::Lt => Some(BinaryOp::Gt),
        BinaryOp::Lte => Some(BinaryOp::Gte),
        _ => None,
    }
}

fn expr_to_json(expr: &Expr, params: &[Value]) -> Option<serde_json::Value> {
    match expr {
        Expr::StringLiteral(value) => Some(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => {
            if !value.is_finite() {
                return None;
            }
            if value.fract() == 0.0 {
                if let Ok(integer) = value.to_string().parse::<i64>() {
                    return Some(serde_json::Value::Number(integer.into()));
                }
            }
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
        Expr::IntegerLiteral(value) => Some(serde_json::Value::Number((*value).into())),
        Expr::BoolLiteral(value) => Some(serde_json::Value::Bool(*value)),
        Expr::Null => Some(serde_json::Value::Null),
        Expr::Param(index) => params.get(*index).map(value_to_json),
        _ => None,
    }
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64(f64::from(*value)))
                .map(serde_json::Value::Number)
                .collect(),
        ),
        Value::Json(value) => value.clone(),
    }
}

fn record_scalar_index_read_path(cassie: &Cassie, spec: &ScalarIndexReadSpec, rows: usize) {
    match spec.path {
        ScalarIndexPlanPath::IndexSeek => {
            cassie
                .runtime
                .record_read_path_index_seek(&spec.collection, rows, &spec.index.name);
        }
        ScalarIndexPlanPath::PrefixScan => {
            cassie
                .runtime
                .record_read_path_prefix_scan(&spec.collection, rows, &spec.index.name);
        }
        ScalarIndexPlanPath::RangeScan => {
            cassie
                .runtime
                .record_read_path_range_scan(&spec.collection, rows, &spec.index.name);
        }
        ScalarIndexPlanPath::OrderedBoundedScan => cassie
            .runtime
            .record_read_path_ordered_bounded_scan(&spec.collection, rows, &spec.index.name),
    }
}
