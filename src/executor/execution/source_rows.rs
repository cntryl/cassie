use std::collections::{BTreeMap, HashMap, HashSet};

use crate::catalog::qualifier_variants;
use crate::catalog::virtual_views;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem, SelectSet, SetOperator};
use crate::types::{DataType, Schema, Value};

use super::{
    batch, filter, row_signature, Batch, BatchRow, CassieSession, FunctionMeta, LogicalPlan,
    QueryError,
};

type SetMemory = Vec<crate::runtime::QueryMemoryReservation>;

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

pub(super) fn source_row_budget(plan: &LogicalPlan, max_result_rows: usize) -> Option<usize> {
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

    let result_cap = max_result_rows.saturating_add(1);
    let limit = plan
        .limit
        .and_then(|limit| usize::try_from(limit.max(0)).ok())
        .unwrap_or(result_cap);
    Some(limit.min(result_cap))
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
    let qualifiers = qualifier_variants(qualifier);
    let (values, mut aliases) = row.into_parts();
    for (index, (name, _)) in values.iter().enumerate() {
        for qualifier in &qualifiers {
            aliases.push((format!("{qualifier}.{name}"), index));
        }
    }
    BatchRow::with_aliases(values, aliases)
}

pub(in crate::executor::execution) fn combine_rows(left: &BatchRow, right: &BatchRow) -> BatchRow {
    let mut values = left.entries().to_vec();
    let left_width = values.len();
    values.extend(right.entries().iter().cloned());
    let mut aliases = left.aliases().to_vec();
    aliases.extend(
        right
            .aliases()
            .iter()
            .map(|(name, index)| (name.clone(), left_width + index)),
    );
    BatchRow::with_aliases(values, aliases)
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
    BatchRow::with_aliases(values, left.aliases().to_vec())
}

pub(in crate::executor::execution) fn combine_nulls_with_row(
    left_columns: &[String],
    right: &BatchRow,
) -> BatchRow {
    let mut values = left_columns
        .iter()
        .map(|column| (column.clone(), Value::Null))
        .collect::<Vec<_>>();
    let left_width = values.len();
    values.extend(right.entries().iter().cloned());
    let aliases = right
        .aliases()
        .iter()
        .map(|(name, index)| (name.clone(), left_width + index))
        .collect();
    BatchRow::with_aliases(values, aliases)
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

pub(in crate::executor::execution) fn row_lookup_columns(rows: &[BatchRow]) -> Vec<String> {
    let mut columns = row_columns(rows);
    for row in rows {
        for (column, _) in row.aliases() {
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

pub(super) fn distinct_batches(
    batches: Vec<Batch>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let mut rows = BTreeMap::<String, BatchRow>::new();
    let mut memory = Vec::new();
    for row in batch::flatten_batches(batches) {
        super::check_timeout(controls)?;
        let signature = row_signature(&row);
        let bytes = signature.len().saturating_add(
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default(),
        );
        if let std::collections::btree_map::Entry::Vacant(entry) = rows.entry(signature) {
            memory.push(controls.reserve_query_memory(bytes)?);
            entry.insert(row);
        }
    }
    Ok(batch::chunk_rows(
        rows.into_values().collect(),
        batch::DEFAULT_BATCH_SIZE,
    ))
}

pub(super) fn distinct_on_batches(
    batches: Vec<Batch>,
    distinct_on: &[Expr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let mut seen = HashSet::<String>::new();
    let mut rows = Vec::new();
    let mut memory = Vec::new();
    for row in batch::flatten_batches(batches) {
        super::check_timeout(controls)?;
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
        let key_bytes = key.len();
        if seen.insert(key) {
            let row_bytes = serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default();
            memory.push(controls.reserve_query_memory(key_bytes.saturating_add(row_bytes))?);
            rows.push(row);
        }
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

pub(super) fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    super::check_timeout(controls)?;
    validate_set_width(&left, &right)?;
    match set.operator {
        SetOperator::UnionAll => {
            let mut rows = left;
            rows.extend(right);
            rows.sort_by_key(row_signature);
            super::check_timeout(controls)?;
            Ok(rows)
        }
        SetOperator::Union => {
            let mut rows = left;
            rows.extend(right);
            let mut unique = BTreeMap::<String, BatchRow>::new();
            let mut memory = Vec::new();
            for row in rows {
                super::check_timeout(controls)?;
                let signature = row_signature(&row);
                let signature_bytes = signature.len();
                if let std::collections::btree_map::Entry::Vacant(entry) = unique.entry(signature) {
                    memory.push(controls.reserve_query_memory(signature_bytes)?);
                    entry.insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Intersect => {
            let (right_signatures, mut memory) = set_signatures(&right, controls)?;
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                super::check_timeout(controls)?;
                let signature = row_signature(&row);
                if !right_signatures.contains(&signature) {
                    continue;
                }
                let signature_bytes = signature.len();
                if let std::collections::btree_map::Entry::Vacant(entry) = unique.entry(signature) {
                    memory.push(controls.reserve_query_memory(signature_bytes)?);
                    entry.insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Except => {
            let (right_signatures, mut memory) = set_signatures(&right, controls)?;
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                super::check_timeout(controls)?;
                let signature = row_signature(&row);
                if right_signatures.contains(&signature) {
                    continue;
                }
                let signature_bytes = signature.len();
                if let std::collections::btree_map::Entry::Vacant(entry) = unique.entry(signature) {
                    memory.push(controls.reserve_query_memory(signature_bytes)?);
                    entry.insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
    }
}

fn set_signatures(
    rows: &[BatchRow],
    controls: &QueryExecutionControls,
) -> Result<(HashSet<String>, SetMemory), QueryError> {
    let mut signatures = HashSet::new();
    let mut memory = Vec::new();
    for row in rows {
        super::check_timeout(controls)?;
        let signature = row_signature(row);
        let signature_bytes = signature.len();
        if signatures.insert(signature) {
            memory.push(controls.reserve_query_memory(signature_bytes)?);
        }
    }
    Ok((signatures, memory))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_add_qualified_lookup_without_duplicate_entries() {
        // Arrange
        let row = BatchRow::new(vec![
            ("user_key".to_string(), Value::Int64(7)),
            ("name".to_string(), Value::String("alpha".to_string())),
        ]);

        // Act
        let qualified = qualify_row(row, "postgres.public.users");

        // Assert
        assert_eq!(qualified.entries().len(), 2);
        assert_eq!(qualified.get("user_key"), Some(&Value::Int64(7)));
        assert_eq!(qualified.get("users.user_key"), Some(&Value::Int64(7)));
        assert_eq!(
            qualified.get("public.users.user_key"),
            Some(&Value::Int64(7))
        );
        assert_eq!(
            qualified.get("postgres.public.users.user_key"),
            Some(&Value::Int64(7))
        );
    }

    #[test]
    fn should_preserve_qualified_lookup_when_combining_rows() {
        // Arrange
        let left = qualify_row(
            BatchRow::new(vec![("user_key".to_string(), Value::Int64(7))]),
            "postgres.public.users",
        );
        let right = qualify_row(
            BatchRow::new(vec![("total".to_string(), Value::Int64(42))]),
            "postgres.public.orders",
        );

        // Act
        let combined = combine_rows(&left, &right);

        // Assert
        assert_eq!(combined.entries().len(), 2);
        assert_eq!(combined.get("users.user_key"), Some(&Value::Int64(7)));
        assert_eq!(combined.get("orders.total"), Some(&Value::Int64(42)));
        assert_eq!(combined.get("public.orders.total"), Some(&Value::Int64(42)));
    }

    #[test]
    fn should_include_aliases_in_lookup_columns_without_expanding_entries() {
        // Arrange
        let rows = vec![qualify_row(
            BatchRow::new(vec![("user_key".to_string(), Value::Int64(7))]),
            "postgres.public.users",
        )];

        // Act
        let output_columns = row_columns(&rows);
        let lookup_columns = row_lookup_columns(&rows);

        // Assert
        assert_eq!(output_columns, vec!["user_key".to_string()]);
        assert!(lookup_columns.contains(&"user_key".to_string()));
        assert!(lookup_columns.contains(&"users.user_key".to_string()));
        assert!(lookup_columns.contains(&"public.users.user_key".to_string()));
        assert!(lookup_columns.contains(&"postgres.public.users.user_key".to_string()));
    }
}
