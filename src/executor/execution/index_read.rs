use super::*;
use crate::catalog::IndexMeta;
use crate::midge::adapter::{DocumentRef, ScalarIndexBound, ScalarIndexScanRequest};
use crate::planner::physical::{scalar_index_plan_shape, ScalarIndexPlanPath};
use std::collections::BTreeMap;

pub(super) fn execute_scalar_index_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = scalar_index_read_spec(cassie, session, plan, params)? else {
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
    let rows = projected_read::finalize_projected_filtered_read(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
        &mut batches,
        false,
        !spec.sort_applied,
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
    plan: &LogicalPlan,
    params: &[Value],
) -> Result<Option<ScalarIndexReadSpec>, QueryError> {
    let Some(projected) = projected_read::projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    if session
        .map(|session| !session.collection_changes(&projected.collection).is_empty())
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let indexes = cassie.catalog.list_indexes(&projected.collection);
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.clone(),
        &Default::default(),
    );
    let Some(index_name) = physical.selected_index.as_deref() else {
        return Ok(None);
    };
    let Some(index) = indexes.into_iter().find(|index| index.name == index_name) else {
        return Ok(None);
    };
    let Some(shape) = scalar_index_plan_shape(plan, &index) else {
        return Ok(None);
    };

    let constraints = concrete_constraints(plan.filter.as_ref(), params)
        .ok_or_else(|| QueryError::General("unsupported scalar index filter".to_string()))?;
    let fields = index.normalized_fields();
    let equality_prefix = fields
        .iter()
        .take(shape.equality_prefix_len)
        .map(|field| {
            constraints
                .get(&field.to_ascii_lowercase())
                .and_then(|constraint| constraint.equality.clone())
                .ok_or_else(|| QueryError::General(format!("missing equality bound for '{field}'")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let lower_bound = shape
        .range_field_index
        .and_then(|field_index| constraints.get(&fields[field_index].to_ascii_lowercase()))
        .and_then(|constraint| constraint.lower.clone())
        .map(|bound| ScalarIndexBound {
            value: bound.value,
            inclusive: bound.inclusive,
        });
    let upper_bound = shape
        .range_field_index
        .and_then(|field_index| constraints.get(&fields[field_index].to_ascii_lowercase()))
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
        covered: physical.covered_index,
        sort_applied: !plan.order.is_empty(),
    }))
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
                    })
                }
                BinaryOp::Gte => {
                    entry.lower = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    })
                }
                BinaryOp::Lt => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: false,
                    })
                }
                BinaryOp::Lte => {
                    entry.upper = Some(ConcreteBound {
                        value,
                        inclusive: true,
                    })
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
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64(*value as f64))
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
                .record_read_path_index_seek(&spec.collection, rows, &spec.index.name)
        }
        ScalarIndexPlanPath::PrefixScan => {
            cassie
                .runtime
                .record_read_path_prefix_scan(&spec.collection, rows, &spec.index.name)
        }
        ScalarIndexPlanPath::RangeScan => {
            cassie
                .runtime
                .record_read_path_range_scan(&spec.collection, rows, &spec.index.name)
        }
        ScalarIndexPlanPath::OrderedBoundedScan => cassie
            .runtime
            .record_read_path_ordered_bounded_scan(&spec.collection, rows, &spec.index.name),
    }
}
