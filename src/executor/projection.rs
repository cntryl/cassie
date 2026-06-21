use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{Batch, BatchRow, RowAccess};
use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::executor::QueryError;
use crate::sql::ast::{Expr, SelectItem};
use crate::types::Value;
use std::sync::atomic::{AtomicU64, Ordering};

static OWNED_VALUE_MOVES: AtomicU64 = AtomicU64::new(0);

pub(crate) fn project_rows<R>(
    rows: Vec<R>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<BatchRow>, QueryError>
where
    R: RowAccess,
{
    let ops = compile_projection_ops(projection);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut projected = Vec::with_capacity(ops.len());
        for op in &ops {
            match op {
                ProjectionOp::Wildcard => {
                    projected.extend(
                        row.entries()
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone())),
                    );
                }
                ProjectionOp::Column { source, key } => {
                    let value = row.get(source).cloned().unwrap_or(Value::Null);
                    projected.push((key.clone(), value));
                }
                ProjectionOp::AggregateFunction { key, function_name } => {
                    let value = row
                        .get(key)
                        .or_else(|| row.get(function_name))
                        .cloned()
                        .unwrap_or(Value::Null);
                    projected.push((key.clone(), value));
                }
                ProjectionOp::ScalarFunction { key, expr } => {
                    let value = filter::evaluate_expr_value(
                        &row,
                        expr,
                        params,
                        search_context,
                        user_functions,
                        session,
                        None,
                    )?;
                    projected.push((key.clone(), value));
                }
                ProjectionOp::WindowFunction { key } => {
                    projected.push((key.clone(), row.get(key).cloned().unwrap_or(Value::Null)));
                }
            }
        }
        out.push(BatchRow::from_projected_values(projected));
    }

    Ok(out)
}

fn compile_projection_ops(projection: &[SelectItem]) -> Vec<ProjectionOp> {
    projection
        .iter()
        .map(|item| match item {
            SelectItem::Wildcard => ProjectionOp::Wildcard,
            SelectItem::Column { name, alias } => ProjectionOp::Column {
                source: name.clone(),
                key: alias.as_deref().unwrap_or(name).to_string(),
            },
            SelectItem::Function { function, alias } => {
                let key = alias
                    .as_deref()
                    .unwrap_or(function.name.as_str())
                    .to_string();
                if crate::sql::functions::is_aggregate_function(&function.name) {
                    ProjectionOp::AggregateFunction {
                        key,
                        function_name: function.name.clone(),
                    }
                } else {
                    ProjectionOp::ScalarFunction {
                        key,
                        expr: Expr::Function(function.clone()),
                    }
                }
            }
            SelectItem::Expr { expr, alias } => ProjectionOp::ScalarFunction {
                key: alias.as_deref().unwrap_or("expr").to_string(),
                expr: expr.clone(),
            },
            SelectItem::WindowFunction { function, alias } => ProjectionOp::WindowFunction {
                key: alias
                    .as_deref()
                    .unwrap_or(function.name.as_str())
                    .to_string(),
            },
        })
        .collect()
}

pub(crate) fn project_batches(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let ops = compile_projection_ops(projection);
    if ops.iter().all(ProjectionOp::can_move_owned) {
        return batches
            .into_iter()
            .map(|batch| project_owned_batch(batch, &ops))
            .collect();
    }

    batches
        .into_iter()
        .map(|batch| {
            project_rows(
                batch,
                projection,
                params,
                search_context,
                user_functions,
                session,
            )
        })
        .collect()
}

fn project_owned_batch(batch: Batch, ops: &[ProjectionOp]) -> Result<Batch, QueryError> {
    let mut out = Vec::with_capacity(batch.len());
    for row in batch {
        out.push(project_owned_row(row, ops));
    }
    Ok(out)
}

fn project_owned_row(row: BatchRow, ops: &[ProjectionOp]) -> BatchRow {
    let mut entries = row
        .into_entries()
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect::<Vec<_>>();
    let mut projected = Vec::with_capacity(ops.len());

    for op in ops {
        match op {
            ProjectionOp::Wildcard => {
                for (name, value) in &mut entries {
                    let value = value.take().unwrap_or(Value::Null);
                    OWNED_VALUE_MOVES.fetch_add(1, Ordering::Relaxed);
                    projected.push((name.clone(), value));
                }
            }
            ProjectionOp::Column { source, key } => {
                let value = entries
                    .iter_mut()
                    .find(|(name, _)| name == source)
                    .and_then(|(_, value)| value.take())
                    .unwrap_or(Value::Null);
                if !matches!(value, Value::Null) {
                    OWNED_VALUE_MOVES.fetch_add(1, Ordering::Relaxed);
                }
                projected.push((key.clone(), value));
            }
            ProjectionOp::WindowFunction { key } => {
                let value = entries
                    .iter_mut()
                    .find(|(name, _)| name == key)
                    .and_then(|(_, value)| value.take())
                    .unwrap_or(Value::Null);
                if !matches!(value, Value::Null) {
                    OWNED_VALUE_MOVES.fetch_add(1, Ordering::Relaxed);
                }
                projected.push((key.clone(), value));
            }
            ProjectionOp::AggregateFunction { .. } | ProjectionOp::ScalarFunction { .. } => {
                unreachable!()
            }
        }
    }

    BatchRow::from_projected_values(projected)
}

enum ProjectionOp {
    Wildcard,
    Column { source: String, key: String },
    AggregateFunction { key: String, function_name: String },
    ScalarFunction { key: String, expr: Expr },
    WindowFunction { key: String },
}

impl ProjectionOp {
    fn can_move_owned(&self) -> bool {
        matches!(
            self,
            ProjectionOp::Wildcard
                | ProjectionOp::Column { .. }
                | ProjectionOp::WindowFunction { .. }
        )
    }
}

#[cfg(test)]
pub(crate) fn owned_value_moves_for_tests() -> u64 {
    OWNED_VALUE_MOVES.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn should_project_column_aliases() {
        // Arrange
        let rows = vec![vec![
            ("id".to_string(), Value::String("doc-1".to_string())),
            ("title".to_string(), Value::String("alpha".to_string())),
        ]];
        let projection = vec![SelectItem::Column {
            name: "title".to_string(),
            alias: Some("headline".to_string()),
        }];

        // Act
        let projected = project_rows::<Vec<(String, Value)>>(
            rows,
            projection.as_slice(),
            &[],
            None,
            &HashMap::new(),
            None,
        )
        .expect("project rows");

        // Assert
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].entries()[0].0, "headline");
        assert_eq!(
            projected[0].get("headline"),
            Some(&Value::String("alpha".to_string()))
        );
    }

    #[test]
    fn should_move_owned_values_for_simple_projection() {
        // Arrange
        let before = owned_value_moves_for_tests();
        let batches = vec![vec![BatchRow::new(vec![
            ("id".to_string(), Value::String("doc-1".to_string())),
            ("title".to_string(), Value::String("alpha".to_string())),
        ])]];
        let projection = vec![SelectItem::Column {
            name: "title".to_string(),
            alias: Some("headline".to_string()),
        }];

        // Act
        let projected = project_batches(
            batches,
            projection.as_slice(),
            &[],
            None,
            &HashMap::new(),
            None,
        )
        .expect("project batches");
        let after = owned_value_moves_for_tests();

        // Assert
        assert_eq!(
            projected[0][0].get("headline"),
            Some(&Value::String("alpha".to_string()))
        );
        assert!(after > before);
    }

    #[test]
    fn should_keep_moved_complex_values_owned() {
        // Arrange
        let payload = serde_json::json!({"nested": ["alpha", "beta"]});
        let vector = crate::types::Vector::new(vec![1.0, 2.0]);
        let batches = vec![vec![BatchRow::new(vec![
            ("payload".to_string(), Value::Json(payload.clone())),
            ("embedding".to_string(), Value::Vector(vector.clone())),
        ])]];
        let projection = vec![
            SelectItem::Column {
                name: "payload".to_string(),
                alias: None,
            },
            SelectItem::Column {
                name: "embedding".to_string(),
                alias: None,
            },
        ];

        // Act
        let mut projected = project_batches(
            batches,
            projection.as_slice(),
            &[],
            None,
            &HashMap::new(),
            None,
        )
        .expect("project batches");
        let row = projected.remove(0).remove(0);

        // Assert
        assert_eq!(row.get("payload"), Some(&Value::Json(payload)));
        assert_eq!(row.get("embedding"), Some(&Value::Vector(vector)));
    }
}
