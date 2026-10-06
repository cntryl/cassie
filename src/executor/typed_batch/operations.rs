use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;

use super::capability::{self, Capability};
use super::scalar::Environment;
use super::{check_controls, invalid, owner_bytes, Cell, Column, QueryError, TypedBatch};
use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul, value_clone_bytes};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{BinaryOp, Expr, SelectItem};
use crate::types::Value;

impl TypedBatch {
    pub(crate) fn filter(
        &self,
        controls: &QueryExecutionControls,
        expression: &Expr,
        params: &[Value],
        functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let native =
            capability::expression(expression, self.schema(), params) == Capability::NativeTyped;
        let env = Environment {
            controls,
            params,
            functions,
            session,
        };
        let bytes = owner_bytes::<Vec<usize>>(mul(self.len(), size_of::<usize>())?)?;
        let memory = controls.reserve_query_memory(bytes)?;
        let mut positions = Vec::with_capacity(self.len());
        for lane in 0..self.len() {
            check_controls(controls)?;
            let selected = if native {
                truth(native_value(self, lane, expression, params)?)?
            } else {
                let value = env.evaluate(self, lane, expression)?;
                truth(Cell::Scalar(value.get()))?
            };
            if selected == Some(true) {
                positions.push(self.position(lane)?);
            }
        }
        let selection = super::admitted(positions, memory);
        Ok(Self {
            schema: Arc::clone(&self.schema),
            columns: Arc::clone(&self.columns),
            domain: self.domain,
            selection: Some(Arc::new(selection)),
        })
    }

    pub(crate) fn project(
        &self,
        controls: &QueryExecutionControls,
        projection: &[SelectItem],
        params: &[Value],
        functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let env = Environment {
            controls,
            params,
            functions,
            session,
        };
        let count = projection
            .iter()
            .map(|item| {
                if matches!(item, SelectItem::Wildcard) {
                    self.schema().len()
                } else {
                    1
                }
            })
            .sum::<usize>();
        let mut scratch = controls.reserve_query_memory(mul(
            count,
            add(
                size_of::<Column>(),
                size_of::<(String, crate::types::DataType)>(),
            )?,
        )?)?;
        let mut schema = Vec::with_capacity(count);
        let mut columns = Vec::with_capacity(count);
        for item in projection {
            match item {
                SelectItem::Wildcard => {
                    let declares_id = self
                        .schema()
                        .iter()
                        .any(|(name, _)| crate::types::row_identity::is_legacy_id_column(name));
                    for (index, (name, data_type)) in self.schema().iter().enumerate() {
                        if declares_id && crate::types::row_identity::is_row_identity_column(name) {
                            continue;
                        }
                        scratch.try_grow(add(
                            mul(name.len(), 32)?,
                            data_type_clone_bytes(data_type)?,
                        )?)?;
                        let label = crate::sql::ColumnIdentifierPath::parse(name)
                            .map_or_else(|_| name.clone(), |column| column.display_name());
                        schema.push((label, data_type.clone()));
                        columns.push(self.project_column(controls, index)?);
                    }
                }
                SelectItem::Column { name, alias } => {
                    scratch.try_grow(add(
                        mul(name.len(), 32)?,
                        alias.as_ref().map_or(name.len(), String::len),
                    )?)?;
                    let index = self
                        .column_index(name)
                        .ok_or_else(|| invalid("projection column is absent"))?;
                    let key = alias.clone().unwrap_or_else(|| {
                        crate::sql::ColumnIdentifierPath::parse(name)
                            .map_or_else(|_| name.clone(), |column| column.display_name())
                    });
                    scratch.try_grow(data_type_clone_bytes(&self.schema()[index].1)?)?;
                    schema.push((key, self.schema()[index].1.clone()));
                    columns.push(self.project_column(controls, index)?);
                }
                SelectItem::Expr { expr, alias } => {
                    let type_owner = env.result_type(self, expr)?;
                    scratch.try_grow(add(
                        alias.as_deref().unwrap_or("expr").len(),
                        data_type_clone_bytes(type_owner.get())?,
                    )?)?;
                    let mut memory =
                        controls.reserve_query_memory(mul(self.len(), size_of::<Value>())?)?;
                    let mut values = Vec::with_capacity(self.len());
                    let native = capability::expression(expr, self.schema(), params)
                        == Capability::NativeTyped;
                    for lane in 0..self.len() {
                        check_controls(controls)?;
                        if native {
                            let value = native_value(self, lane, expr, params)?;
                            memory.try_grow(super::scalar::cell_heap(value)?)?;
                            values.push(value.to_owned());
                        } else {
                            let value = env.evaluate(self, lane, expr)?;
                            memory.try_grow(value_clone_bytes(value.get())?)?;
                            values.push(value.get().clone());
                        }
                    }
                    columns.push(Column::from_values(controls, type_owner.get(), &values)?);
                    schema.push((
                        alias.as_deref().unwrap_or("expr").to_owned(),
                        type_owner.get().clone(),
                    ));
                }
                _ => {
                    return Err(invalid(
                        "projection uses an existing scalar operator boundary",
                    ))
                }
            }
        }
        Self::from_views(controls, &schema, &columns, self.len(), None)
    }

    fn project_column(
        &self,
        controls: &QueryExecutionControls,
        index: usize,
    ) -> Result<Column, QueryError> {
        let column = &self.columns.get()[index];
        self.selection.as_ref().map_or_else(
            || Ok(column.clone()),
            |positions| column.gather(controls, positions.get()),
        )
    }
}

pub(super) fn native_value<'a>(
    batch: &'a TypedBatch,
    lane: usize,
    expr: &'a Expr,
    params: &'a [Value],
) -> Result<Cell<'a>, QueryError> {
    match expr {
        Expr::Column(name) => batch.cell(
            batch
                .column_index(name)
                .ok_or_else(|| invalid("native column is absent"))?,
            lane,
        ),
        Expr::Param(index) => Ok(params.get(*index).map_or(Cell::Null, Cell::Scalar)),
        Expr::IntegerLiteral(value) => Ok(Cell::Integer(*value)),
        Expr::NumberLiteral(value) => Ok(Cell::Float(*value)),
        Expr::BoolLiteral(value) => Ok(Cell::Boolean(*value)),
        Expr::StringLiteral(value) => Ok(Cell::Text(value)),
        Expr::Null => Ok(Cell::Null),
        Expr::IsNull { expr, negated } => Ok(Cell::Boolean(
            is_null(native_value(batch, lane, expr, params)?) != *negated,
        )),
        Expr::Not { expr } => Ok(truth(native_value(batch, lane, expr, params)?)?
            .map_or(Cell::Null, |value| Cell::Boolean(!value))),
        Expr::Binary { left, op, right } => {
            let left = native_value(batch, lane, left, params)?;
            let right = native_value(batch, lane, right, params)?;
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                let (left, right) = (truth(left)?, truth(right)?);
                let value = match (op, left, right) {
                    (BinaryOp::And, Some(false), _) | (BinaryOp::And, _, Some(false)) => {
                        Some(false)
                    }
                    (BinaryOp::And, Some(true), Some(true))
                    | (BinaryOp::Or, Some(true), _)
                    | (BinaryOp::Or, _, Some(true)) => Some(true),
                    (BinaryOp::Or, Some(false), Some(false)) => Some(false),
                    _ => None,
                };
                return Ok(value.map_or(Cell::Null, Cell::Boolean));
            }
            if is_null(left) || is_null(right) {
                return Ok(Cell::Null);
            }
            let order = crate::executor::semantic::compare_numeric_values(
                &numeric(left)?,
                &numeric(right)?,
            )
            .ok_or_else(|| invalid("native comparison requires numeric values"))?;
            Ok(Cell::Boolean(match op {
                BinaryOp::Eq => order.is_eq(),
                BinaryOp::NotEq => !order.is_eq(),
                BinaryOp::Lt => order.is_lt(),
                BinaryOp::Lte => !order.is_gt(),
                BinaryOp::Gt => order.is_gt(),
                BinaryOp::Gte => !order.is_lt(),
                _ => return Err(invalid("unqualified native binary kernel")),
            }))
        }
        _ => Err(invalid("unqualified native expression kernel")),
    }
}

fn is_null(value: Cell<'_>) -> bool {
    matches!(value, Cell::Null | Cell::Scalar(Value::Null))
}

fn truth(value: Cell<'_>) -> Result<Option<bool>, QueryError> {
    match value {
        Cell::Null | Cell::Scalar(Value::Null) => Ok(None),
        Cell::Boolean(value) => Ok(Some(value)),
        Cell::Scalar(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(QueryError::General(
            "Boolean expression requires BOOLEAN or SQL NULL".to_owned(),
        )),
    }
}

fn numeric(value: Cell<'_>) -> Result<Value, QueryError> {
    match value {
        Cell::Integer(value) => Ok(Value::Int64(value)),
        Cell::Float(value) => Ok(Value::Float64(value)),
        Cell::Scalar(Value::Int64(value)) => Ok(Value::Int64(*value)),
        Cell::Scalar(Value::Float64(value)) => Ok(Value::Float64(*value)),
        _ => Err(invalid("native comparison value is not numeric")),
    }
}
