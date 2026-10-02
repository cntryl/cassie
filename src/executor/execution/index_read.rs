use super::scalar_index_constraints::{
    canonicalize_field_constraints, concrete_constraints, expression_index_constraints,
    ConcreteConstraint,
};
pub(super) use super::scalar_index_constraints::{
    canonicalize_float_number, index_trailing_keys_not_null,
};
use super::{
    batch, check_timeout, projected_read, scan, BatchRow, Cassie, CassieSession, Expr,
    FunctionMeta, HashMap, LogicalPlan, PhysicalPlan, QueryError, QueryExecutionControls,
    QuerySource, SelectItem, Value,
};
use crate::catalog::IndexMeta;
use crate::midge::adapter::{DocumentRef, ScalarIndexBound, ScalarIndexScanRequest};
use crate::planner::physical::{scalar_index_plan_shape, ScalarIndexNullKeys, ScalarIndexPlanPath};
use crate::types::DataType;
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
    if spec.null_keys == ScalarIndexNullKeys::SortAfterLimit && !hits_fill_limit(plan, hits.len()) {
        // Rows with a NULL sort key are not indexed; the scan and sort path
        // returns them once the indexed rows run out.
        return Ok(None);
    }
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
            &document,
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
    null_keys: ScalarIndexNullKeys,
    predicate_resolution: ScalarIndexPredicateResolution,
}

fn hits_fill_limit(plan: &LogicalPlan, hits: usize) -> bool {
    let Some(limit) = plan.limit else {
        return false;
    };
    let needed = limit.max(0).saturating_add(plan.offset.unwrap_or(0).max(0));
    usize::try_from(needed).is_ok_and(|needed| hits >= needed)
}

#[derive(Debug, Clone, Copy)]
enum ScalarIndexPredicateResolution {
    Unsatisfiable,
    Exact,
    Residual,
}

pub(super) fn signed_zero_probe_counterpart(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> Option<serde_json::Value> {
    if cassie.catalog.field_type(collection, field) != Some(DataType::Float) {
        return None;
    }
    let number = value.as_f64()?;
    if !number.is_finite() || number != 0.0 {
        return None;
    }
    serde_json::Number::from_f64(-number).map(serde_json::Value::Number)
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
    let not_null_fields = cassie.catalog.not_null_fields(&projected.collection);
    let physical = physical.filter(|physical| physical.collection == projected.collection);
    let (index_name, covered_index) = if let Some(physical) = physical {
        let Some(index_name) = physical.read.selected_index.as_deref() else {
            return Ok(None);
        };
        (index_name.to_string(), physical.read.covered_index)
    } else {
        let cardinality_stats =
            std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
        let physical = crate::planner::physical::build_with_indexes_and_not_null_fields(
            plan.clone(),
            indexes.as_slice(),
            &not_null_fields,
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
    let Some(mut shape) = scalar_index_plan_shape(plan, &index, &not_null_fields) else {
        return Ok(None);
    };
    if scalar_index_order_needs_sql_sort(cassie, &projected.collection, &index, &shape, plan) {
        if shape.path == ScalarIndexPlanPath::OrderedBoundedScan {
            return Ok(None);
        }
        // Keep exact predicate candidate selection, but sort all candidates
        // using SQL semantics before applying the requested result window.
        shape.order_satisfied = false;
    }
    if scalar_index_requires_json_scan(cassie, &projected.collection, &index, &shape, plan)
        || scalar_index_requires_semantic_scan(cassie, &projected.collection, &index, plan, params)
    {
        // Decline byte-key reads when executor comparison can equate different
        // encodings or when probe conversion would lose numeric precision.
        return Ok(None);
    }
    // Scalar-index keys cannot represent NaN or infinities, while the filter
    // operator orders them above every finite value. Let the scan answer, but
    // only when such a parameter is actually part of this read's predicate: a
    // non-finite parameter elsewhere in the statement cannot change the keys the
    // index has to answer for.
    if filter_binds_non_finite_param(plan.filter.as_ref(), params) {
        return Ok(None);
    }
    if scalar_index_reads_unsafe_numeric_bounds(cassie, &projected.collection, &index, plan, params)
    {
        // LexKey keeps -0.0 and +0.0 as distinct ordered keys, while SQL
        // comparisons treat them as equal. Expression-index metadata also
        // omits result types, so use SQL filtering for numeric or timestamp-shaped
        // expression bounds whose probe encoding may differ from SQL semantics.
        return Ok(None);
    }

    let Some((request, predicate_resolution)) =
        scalar_index_scan_request(cassie, &projected.collection, &index, &shape, plan, params)?
    else {
        return Ok(None);
    };

    Ok(Some(ScalarIndexReadSpec {
        collection: projected.collection,
        index,
        scan_fields: projected.scan_fields,
        request,
        path: shape.path,
        covered: covered_index,
        sort_applied: plan.order.is_empty() || shape.order_satisfied,
        null_keys: shape.null_keys,
        predicate_resolution,
    }))
}

fn scalar_index_reads_unsafe_numeric_bounds(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    plan: &LogicalPlan,
    params: &[Value],
) -> bool {
    let Some(filter) = plan.filter.as_ref() else {
        return false;
    };

    let field_constraint_reads_zero =
        concrete_constraints(Some(filter), params).is_some_and(|constraints| {
            index.normalized_fields().iter().any(|field| {
                cassie.catalog.field_type(collection, field) == Some(DataType::Float)
                    && constraints
                        .get(&field.to_ascii_lowercase())
                        .is_some_and(constraint_contains_zero)
            })
        });
    let expression_constraint_reads_numeric = !index.expressions.is_empty()
        && expression_index_constraints(Some(filter), params).is_some_and(|constraints| {
            index.normalized_expressions().iter().any(|expression| {
                constraints
                    .expressions
                    .get(expression)
                    .is_some_and(constraint_contains_unsafe_expression_bound)
            })
        });

    field_constraint_reads_zero || expression_constraint_reads_numeric
}

fn constraint_contains_zero(constraint: &ConcreteConstraint) -> bool {
    constraint
        .equality
        .iter()
        .chain(constraint.lower.iter().map(|bound| &bound.value))
        .chain(constraint.upper.iter().map(|bound| &bound.value))
        .any(is_zero_number)
}

fn constraint_contains_unsafe_expression_bound(constraint: &ConcreteConstraint) -> bool {
    constraint
        .equality
        .iter()
        .chain(constraint.lower.iter().map(|bound| &bound.value))
        .chain(constraint.upper.iter().map(|bound| &bound.value))
        .any(|value| {
            value.is_number()
                || value
                    .as_str()
                    .is_some_and(crate::types::temporal::is_canonical_timestamp_text)
        })
}

fn is_zero_number(value: &serde_json::Value) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.is_finite() && number == 0.0)
}

fn scalar_index_scan_request(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    plan: &LogicalPlan,
    params: &[Value],
) -> Result<Option<(ScalarIndexScanRequest, ScalarIndexPredicateResolution)>, QueryError> {
    let extracted_constraints = if index.expressions.is_empty() {
        concrete_constraints(plan.filter.as_ref(), params)
            .map(|constraints| (constraints, BTreeMap::new()))
    } else {
        expression_index_constraints(plan.filter.as_ref(), params)
            .map(|constraints| (constraints.fields, constraints.expressions))
    };
    let Some((mut constraints, expression_constraints)) = extracted_constraints else {
        return Ok(None);
    };
    canonicalize_field_constraints(cassie, collection, &mut constraints);
    let equality_prefix =
        scalar_index_equality_prefix(index, shape, &constraints, &expression_constraints)?;
    let range_constraint =
        range_constraint_for_shape(index, shape, &constraints, &expression_constraints);
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
        scalar_index_bounds_are_exact(index, shape, &constraints, &expression_constraints);
    let request = ScalarIndexScanRequest {
        equality_prefix,
        lower_bound,
        upper_bound,
        reverse: shape.reverse,
        limit: storage_limit(plan, shape, bounds_are_exact),
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

    Ok(Some((request, predicate_resolution)))
}

fn scalar_index_requires_json_scan(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    plan: &LogicalPlan,
) -> bool {
    let fields = index.normalized_fields();
    let is_json_field =
        |field: &str| cassie.catalog.field_type(collection, field) == Some(DataType::Json);
    let uses_json_range_key = shape
        .range_field_index
        .and_then(|field_index| fields.get(field_index))
        .is_some_and(|field| is_json_field(field));
    let uses_json_order_key = !plan.order.is_empty()
        && shape.order_satisfied
        && fields
            .iter()
            .take(shape.order_columns_used)
            .any(|field| is_json_field(field));

    uses_json_range_key || uses_json_order_key
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
    if let Expr::Column(name) = expr {
        if !projected_read::is_row_id_column(name)
            && !fields.iter().any(|field| field.eq_ignore_ascii_case(name))
        {
            fields.push(name.clone());
        }
    }
    expr.for_each_child(|child| collect_expression_columns(child, fields));
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

fn is_non_finite_float(value: &Value) -> bool {
    matches!(value, Value::Float64(value) if !value.is_finite())
}

/// Reports whether the read's own predicate binds a NaN or infinite parameter.
///
/// A subquery is treated as binding one, because its predicates are not
/// inspected here.
fn filter_binds_non_finite_param(filter: Option<&Expr>, params: &[Value]) -> bool {
    let Some(filter) = filter else {
        return false;
    };
    match filter {
        Expr::Param(index) => params.get(*index).is_some_and(is_non_finite_float),
        Expr::Exists(_) => params.iter().any(is_non_finite_float),
        Expr::Binary { left, right, .. } => {
            filter_binds_non_finite_param(Some(left), params)
                || filter_binds_non_finite_param(Some(right), params)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            filter_binds_non_finite_param(Some(expr), params)
        }
        Expr::InList { expr, values, .. } => {
            filter_binds_non_finite_param(Some(expr), params)
                || values
                    .iter()
                    .any(|value| filter_binds_non_finite_param(Some(value), params))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            filter_binds_non_finite_param(Some(expr), params)
                || filter_binds_non_finite_param(Some(low), params)
                || filter_binds_non_finite_param(Some(high), params)
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            operand
                .as_deref()
                .is_some_and(|operand| filter_binds_non_finite_param(Some(operand), params))
                || branches.iter().any(|(when, then)| {
                    filter_binds_non_finite_param(Some(when), params)
                        || filter_binds_non_finite_param(Some(then), params)
                })
                || else_expr
                    .as_deref()
                    .is_some_and(|expr| filter_binds_non_finite_param(Some(expr), params))
        }
        Expr::Function(call) => call
            .args
            .iter()
            .any(|arg| filter_binds_non_finite_param(Some(arg), params)),
        Expr::Column(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => false,
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

fn scalar_index_requires_semantic_scan(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    plan: &LogicalPlan,
    params: &[Value],
) -> bool {
    let fields = index.normalized_fields();
    concrete_constraints(plan.filter.as_ref(), params).is_some_and(|constraints| {
        fields.iter().any(|field| {
            constraints
                .get(&field.to_ascii_lowercase())
                .is_some_and(|constraint| {
                    constraint
                        .equality
                        .iter()
                        .chain(constraint.lower.iter().map(|bound| &bound.value))
                        .chain(constraint.upper.iter().map(|bound| &bound.value))
                        .any(|value| {
                            !super::index_probe_canonicalization::probe_comparison_is_exact(
                                cassie, collection, field, value,
                            )
                        })
                })
        })
    })
}

fn scalar_index_order_needs_sql_sort(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    plan: &LogicalPlan,
) -> bool {
    if plan.order.is_empty() || !shape.order_satisfied {
        return false;
    }
    if index
        .normalized_fields()
        .iter()
        .skip(shape.equality_prefix_len)
        .take(shape.order_columns_used)
        .any(|field| {
            matches!(
                cassie.catalog.field_type(collection, field),
                Some(DataType::Text | DataType::Char { .. } | DataType::Varchar { .. })
            )
        })
    {
        return true;
    }
    if index.expressions.is_empty() {
        return false;
    }
    let Some(source) = cassie.catalog.get_schema(collection) else {
        return true;
    };
    let schema = crate::types::Schema {
        fields: source
            .fields
            .into_iter()
            .map(|field| crate::types::FieldSchema {
                name: field.name,
                data_type: field.data_type,
                nullable: true,
            })
            .collect(),
    };
    plan.order.iter().any(|order| {
        // Lowercase output cannot have the uppercase T/Z timestamp shape.
        if matches!(&order.expr, Expr::Function(function) if function.name.eq_ignore_ascii_case("lower")) {
            return false;
        }
        matches!(crate::sql::binder::infer_expr_type(&order.expr, &schema, &HashMap::new(), &[]),
            None | Some(DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } | DataType::Json | DataType::Array(_)))
    })
}
