use super::{
    batch, projected_read, scan, BatchRow, BinaryOp, Cassie, CassieSession, Expr, FunctionMeta,
    HashMap, LogicalPlan, PhysicalPlan, QueryError, QueryExecutionControls, QuerySource,
    SelectItem, Value,
};
use crate::catalog::IndexMeta;
use crate::midge::adapter::{DocumentRef, ScalarIndexBound, ScalarIndexScanRequest};
use crate::planner::physical::{scalar_index_plan_shape, ScalarIndexPlanPath};
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

    let hits = cassie
        .midge
        .scan_scalar_index(&spec.index, spec.request.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    let schema = cassie.catalog.get_schema(&spec.collection);
    let mut rows = Vec::with_capacity(hits.len());

    for hit in hits {
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
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
        &mut batches,
        false,
        !spec.sort_applied,
        Some(index_usage),
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
        let Some(index_name) = physical.selected_index.as_deref() else {
            return Ok(None);
        };
        (index_name.to_string(), physical.covered_index)
    } else {
        let cardinality_stats =
            std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
        let physical = crate::planner::physical::build_with_indexes(
            plan.clone(),
            indexes.as_slice(),
            &cardinality_stats,
        );
        let Some(index_name) = physical.selected_index else {
            return Ok(None);
        };
        (index_name, physical.covered_index)
    };
    let Some(index) = indexes.into_iter().find(|index| index.name == index_name) else {
        return Ok(None);
    };
    let Some(shape) = scalar_index_plan_shape(plan, &index) else {
        return Ok(None);
    };

    let index_fields = index.normalized_fields();
    let constraints = if index.expressions.is_empty() {
        concrete_constraints(plan.filter.as_ref(), params)
    } else if index_fields.is_empty() {
        Some(BTreeMap::new())
    } else {
        concrete_constraints_for_expression_index(plan.filter.as_ref(), params)
    }
    .ok_or_else(|| QueryError::General("unsupported scalar index filter".to_string()))?;
    let fields = index_fields;
    let expression_equalities =
        if index.expressions.is_empty() || shape.equality_prefix_len <= fields.len() {
            BTreeMap::new()
        } else {
            concrete_expression_equalities(plan.filter.as_ref(), params).ok_or_else(|| {
                QueryError::General("unsupported scalar expression index filter".to_string())
            })?
        };
    let expression_constraints = if index.expressions.is_empty() {
        BTreeMap::new()
    } else {
        concrete_expression_constraints(plan.filter.as_ref(), params).ok_or_else(|| {
            QueryError::General("unsupported scalar expression index filter".to_string())
        })?
    };
    let equality_prefix =
        scalar_index_equality_prefix(&index, &shape, &constraints, &expression_equalities)?;
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
    let request = ScalarIndexScanRequest {
        equality_prefix,
        lower_bound,
        upper_bound,
        reverse: shape.reverse,
        limit: storage_limit(plan),
    };

    Ok(Some(ScalarIndexReadSpec {
        collection: projected.collection,
        index,
        scan_fields: projected.scan_fields,
        request,
        path: shape.path,
        covered: covered_index,
        sort_applied: !plan.order.is_empty(),
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

    let scan_fields = projection_columns
        .into_iter()
        .filter(|column| !projected_read::is_row_id_column(column))
        .collect::<Vec<_>>();
    Some(projected_read::ProjectedFilteredReadSpec {
        collection: collection.clone(),
        scan_fields,
        scan_limit: None,
    })
}

fn scalar_index_equality_prefix(
    index: &IndexMeta,
    shape: &crate::planner::physical::ScalarIndexPlanShape,
    constraints: &BTreeMap<String, ConcreteConstraint>,
    expression_equalities: &BTreeMap<String, serde_json::Value>,
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
        let value = expression_equalities
            .get(expression)
            .cloned()
            .ok_or_else(|| QueryError::General("missing expression equality bound".to_string()))?;
        equality_prefix.push(value);
    }

    Ok(equality_prefix)
}

fn storage_limit(plan: &LogicalPlan) -> Option<usize> {
    let limit = usize::try_from(plan.limit?.max(0)).ok()?;
    let offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

#[derive(Debug, Clone)]
struct ConcreteConstraint {
    equality: Option<serde_json::Value>,
    lower: Option<ConcreteBound>,
    upper: Option<ConcreteBound>,
}

#[derive(Debug, Clone)]
struct ConcreteBound {
    value: serde_json::Value,
    inclusive: bool,
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

fn concrete_constraints_for_expression_index(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<BTreeMap<String, ConcreteConstraint>> {
    let mut constraints = BTreeMap::new();
    collect_expression_index_field_constraints(filter?, params, &mut constraints)?;
    Some(constraints)
}

fn collect_expression_index_field_constraints(
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
            collect_expression_index_field_constraints(left, params, constraints)?;
            collect_expression_index_field_constraints(right, params, constraints)
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } if concrete_expression_equality(left, right, params).is_some() => Some(()),
        Expr::Binary { left, op, right } => {
            let (field, op, value) = concrete_constraint(left, op, right, params)?;
            let entry = constraints
                .entry(field)
                .or_insert_with(|| ConcreteConstraint {
                    equality: None,
                    lower: None,
                    upper: None,
                });
            match op {
                BinaryOp::Eq => entry.equality = Some(value),
                _ => return None,
            }
            Some(())
        }
        _ => None,
    }
}

fn concrete_expression_equalities(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<BTreeMap<String, serde_json::Value>> {
    let mut equalities = BTreeMap::new();
    collect_concrete_expression_equalities(filter?, params, &mut equalities)?;
    Some(equalities)
}

fn collect_concrete_expression_equalities(
    expr: &Expr,
    params: &[Value],
    equalities: &mut BTreeMap<String, serde_json::Value>,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_concrete_expression_equalities(left, params, equalities)?;
            collect_concrete_expression_equalities(right, params, equalities)
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => collect_concrete_expression_equality(left, right, params, equalities),
        _ => None,
    }
}

fn concrete_expression_constraints(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<BTreeMap<String, ConcreteConstraint>> {
    let mut constraints = BTreeMap::new();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_concrete_expression_constraints(filter, params, &mut constraints)?;
    Some(constraints)
}

fn collect_concrete_expression_constraints(
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
            collect_concrete_expression_constraints(left, params, constraints)?;
            collect_concrete_expression_constraints(right, params, constraints)
        }
        Expr::Binary { left, op, right } => {
            let (expression, op, value) = concrete_expression_constraint(left, op, right, params)?;
            let entry = constraints
                .entry(expression)
                .or_insert_with(|| ConcreteConstraint {
                    equality: None,
                    lower: None,
                    upper: None,
                });
            match op {
                BinaryOp::Eq => entry.equality = Some(value),
                BinaryOp::Gt => {
                    entry.lower = Some(ConcreteBound {
                        value,
                        inclusive: false,
                    });
                }
                BinaryOp::Gte => {
                    entry.lower = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    });
                }
                BinaryOp::Lt => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: false,
                    });
                }
                BinaryOp::Lte => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    });
                }
                _ => return None,
            }
            Some(())
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if expr_has_column(expr) && !matches!(expr.as_ref(), Expr::Column(_)) => {
            let entry = constraints
                .entry(serde_json::to_string(expr.as_ref()).ok()?)
                .or_insert_with(|| ConcreteConstraint {
                    equality: None,
                    lower: None,
                    upper: None,
                });
            entry.lower = Some(ConcreteBound {
                value: expr_to_json(low, params)?,
                inclusive: true,
            });
            entry.upper = Some(ConcreteBound {
                value: expr_to_json(high, params)?,
                inclusive: true,
            });
            Some(())
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

fn collect_concrete_expression_equality(
    left: &Expr,
    right: &Expr,
    params: &[Value],
    equalities: &mut BTreeMap<String, serde_json::Value>,
) -> Option<()> {
    if let Some((expression, value)) = concrete_expression_equality(left, right, params) {
        equalities.insert(expression, value);
    }
    Some(())
}

fn concrete_expression_equality(
    left: &Expr,
    right: &Expr,
    params: &[Value],
) -> Option<(String, serde_json::Value)> {
    match (left, right) {
        (expr, value) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => {
            let value = expr_to_json(value, params)?;
            Some((serde_json::to_string(expr).ok()?, value))
        }
        (value, expr) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => {
            let value = expr_to_json(value, params)?;
            Some((serde_json::to_string(expr).ok()?, value))
        }
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
            let entry = constraints
                .entry(field)
                .or_insert_with(|| ConcreteConstraint {
                    equality: None,
                    lower: None,
                    upper: None,
                });
            match op {
                BinaryOp::Eq => entry.equality = Some(value),
                BinaryOp::Gt => {
                    entry.lower = Some(ConcreteBound {
                        value,
                        inclusive: false,
                    });
                }
                BinaryOp::Gte => {
                    entry.lower = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    });
                }
                BinaryOp::Lt => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: false,
                    });
                }
                BinaryOp::Lte => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    });
                }
                _ => return None,
            }
            Some(())
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
            let entry = constraints
                .entry(field.to_ascii_lowercase())
                .or_insert_with(|| ConcreteConstraint {
                    equality: None,
                    lower: None,
                    upper: None,
                });
            entry.lower = Some(ConcreteBound {
                value: expr_to_json(low, params)?,
                inclusive: true,
            });
            entry.upper = Some(ConcreteBound {
                value: expr_to_json(high, params)?,
                inclusive: true,
            });
            Some(())
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
                let integer = *value as i64;
                if (integer as f64) == *value {
                    return Some(serde_json::Value::Number(integer.into()));
                }
            }
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
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
