use std::collections::{BTreeMap, HashMap, HashSet};

use crate::catalog::virtual_views;
use crate::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem, SelectSet, SetOperator};
use crate::types::{DataType, Schema, Value};

use super::{
    batch, filter, row_signature, Batch, BatchRow, CassieSession, FunctionMeta, LogicalPlan,
    QueryError,
};

pub(super) fn schema_text_fields(schema: &Schema) -> Vec<String> {
    schema
        .fields
        .iter()
        .filter(|field| field.data_type == DataType::Text)
        .map(|field| field.name.clone())
        .collect()
}

pub(super) fn qualify_batches(batches: Vec<Batch>, qualifier: &str) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| qualify_row(row, qualifier))
                .collect()
        })
        .collect()
}

pub(super) fn combine_batches_with_outer_row(
    batches: Vec<Batch>,
    outer_row: &BatchRow,
) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| combine_rows(outer_row, &row))
                .collect()
        })
        .collect()
}

pub(super) fn source_row_budget(plan: &LogicalPlan) -> Option<usize> {
    let QuerySource::Join { .. } = &plan.source else {
        return None;
    };
    if plan.filter.is_some()
        || plan.having.is_some()
        || plan.offset.unwrap_or(0) > 0
        || plan.set.is_some()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.ctes.is_empty()
        || !plan.group_by.is_empty()
        || !plan.order.is_empty()
        || plan_uses_aggregate(plan)
        || plan.projection.iter().any(|item| {
            !matches!(item, SelectItem::Wildcard | SelectItem::Column { .. })
                && !matches!(
                    item,
                    SelectItem::Expr {
                        expr: Expr::Column(_),
                        ..
                    }
                )
        })
    {
        return None;
    }

    let limit = plan.limit?;
    usize::try_from(limit.max(0)).ok()
}

pub(in crate::executor::execution) fn source_contains_lateral(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { left, right, .. } => {
            source_contains_lateral(left) || source_contains_lateral(right)
        }
        QuerySource::Subquery { lateral, .. } | QuerySource::TableFunction { lateral, .. } => {
            *lateral
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

pub(in crate::executor::execution) fn qualify_row(row: BatchRow, qualifier: &str) -> BatchRow {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut values = Vec::new();
    for (name, value) in row.into_entries() {
        values.push((name.clone(), value.clone()));
        values.push((format!("{qualifier}.{name}"), value));
    }
    BatchRow::new(values)
}

pub(in crate::executor::execution) fn combine_rows(left: &BatchRow, right: &BatchRow) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

pub(in crate::executor::execution) fn combine_row_with_nulls(
    left: &BatchRow,
    right_columns: &[String],
) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(
        right_columns
            .iter()
            .map(|column| (column.clone(), Value::Null)),
    );
    BatchRow::new(values)
}

pub(in crate::executor::execution) fn combine_nulls_with_row(
    left_columns: &[String],
    right: &BatchRow,
) -> BatchRow {
    let mut values = left_columns
        .iter()
        .map(|column| (column.clone(), Value::Null))
        .collect::<Vec<_>>();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

pub(in crate::executor::execution) fn row_columns(rows: &[BatchRow]) -> Vec<String> {
    let mut columns = Vec::new();
    for row in rows {
        for (column, _) in row.entries() {
            if !columns.contains(column) {
                columns.push(column.clone());
            }
        }
    }
    columns
}

pub(super) fn materialize_virtual_rows(rows: Vec<virtual_views::VirtualRow>) -> Vec<Batch> {
    batch::chunk_rows(
        rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
        batch::DEFAULT_BATCH_SIZE,
    )
}

pub(super) fn project_rows_to_schema(
    rows: Vec<BatchRow>,
    schema: &Schema,
    relation: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let entries = row.into_entries();
        if entries.len() < schema.fields.len() {
            return Err(QueryError::General(format!(
                "view '{}' produced {} columns but schema expects {}",
                relation,
                entries.len(),
                schema.fields.len()
            )));
        }

        let mut values = Vec::with_capacity(schema.fields.len());
        for (field, (_name, value)) in schema.fields.iter().zip(entries) {
            values.push((field.name.clone(), value));
        }
        projected.push(BatchRow::new(values));
    }
    Ok(projected)
}

pub(super) fn distinct_batches(batches: Vec<Batch>) -> Vec<Batch> {
    let mut rows = BTreeMap::<String, BatchRow>::new();
    for row in batch::flatten_batches(batches) {
        rows.entry(row_signature(&row)).or_insert(row);
    }
    batch::chunk_rows(rows.into_values().collect(), batch::DEFAULT_BATCH_SIZE)
}

pub(super) fn distinct_on_batches(
    batches: Vec<Batch>,
    distinct_on: &[Expr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let mut seen = HashSet::<String>::new();
    let mut rows = Vec::new();
    for row in batch::flatten_batches(batches) {
        let key = distinct_on
            .iter()
            .map(|expr| {
                filter::evaluate_expr_value(
                    &row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )
                .map(|value| value_sort_key(&value))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("|");
        if seen.insert(key) {
            rows.push(row);
        }
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

pub(super) fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
) -> Result<Vec<BatchRow>, QueryError> {
    validate_set_width(&left, &right)?;
    match set.operator {
        SetOperator::UnionAll => {
            let mut rows = left;
            rows.extend(right);
            rows.sort_by_key(row_signature);
            Ok(rows)
        }
        SetOperator::Union => {
            let mut rows = left;
            rows.extend(right);
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in rows {
                unique.entry(row_signature(&row)).or_insert(row);
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Intersect => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Except => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if !right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
    }
}

pub(in crate::executor::execution) fn slice_rows(
    rows: Vec<BatchRow>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Vec<BatchRow> {
    let offset = offset
        .and_then(|value| usize::try_from(value.max(0)).ok())
        .unwrap_or(0);
    let limit = limit.and_then(|value| usize::try_from(value.max(0)).ok());
    let iter = rows.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

pub(super) fn validate_set_width(left: &[BatchRow], right: &[BatchRow]) -> Result<(), QueryError> {
    let left_width = left.first().map(|row| row.entries().len());
    let right_width = right.first().map(|row| row.entries().len());
    if let (Some(left_width), Some(right_width)) = (left_width, right_width) {
        if left_width != right_width {
            return Err(QueryError::General(format!(
                "set operation column count mismatch: {left_width} != {right_width}"
            )));
        }
    }
    Ok(())
}

pub(super) fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}

pub(crate) fn group_expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        _ => expr_key(expr),
    }
}

pub(crate) fn aggregate_signature(function: &FunctionCall) -> String {
    format!(
        "{}({})",
        function.name.to_ascii_lowercase(),
        function
            .args
            .iter()
            .map(expr_key)
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(crate) fn expr_key(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        Expr::Param(index) => format!("${}", index + 1),
        Expr::Null => "null".to_string(),
        Expr::BoolLiteral(value) => value.to_string(),
        Expr::NumberLiteral(value) => value.to_string(),
        Expr::StringLiteral(value) => format!("'{value}'"),
        Expr::Function(function) => aggregate_signature(function),
        Expr::Binary { left, op, right } => {
            format!("{}{:?}{}", expr_key(left), op, expr_key(right))
        }
        Expr::IsNull { expr, negated } => {
            format!(
                "{} is{} null",
                expr_key(expr),
                if *negated { " not" } else { "" }
            )
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => format!(
            "{}{} in ({})",
            expr_key(expr),
            if *negated { " not" } else { "" },
            values.iter().map(expr_key).collect::<Vec<_>>().join(",")
        ),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => format!(
            "{}{} between {} and {}",
            expr_key(expr),
            if *negated { " not" } else { "" },
            expr_key(low),
            expr_key(high)
        ),
        Expr::Not { expr } => format!("not {}", expr_key(expr)),
        Expr::Cast { expr, data_type } => format!("{}::{data_type:?}", expr_key(expr)),
        Expr::Exists(_) => "exists".to_string(),
    }
}

pub(crate) fn value_sort_key(value: &Value) -> String {
    match value {
        Value::Null => "0:null".to_string(),
        Value::Bool(value) => format!("1:{value}"),
        Value::Int64(value) => format!("2:{value:020}"),
        Value::Float64(value) => format!("3:{value:020.12}"),
        Value::String(value) => format!("4:{value}"),
        Value::Vector(value) => format!("5:{:?}", value.values),
        Value::Json(value) => format!("6:{value}"),
    }
}
