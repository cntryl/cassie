//! Explicit one-lane scalar-expression boundary. Native kernels never construct this row.
use std::collections::HashMap;
use std::mem::size_of;

use super::{check_controls, Cell, QueryError, TypedBatch};
use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::filter;
use crate::executor::retained_memory::{add, mul, serialized_json_bytes, value_clone_bytes};
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::Expr;
use crate::types::{DataType, Value};

struct ScalarRow<'a> {
    entries: Vec<(String, Value)>,
    schema: &'a [(String, DataType)],
}

impl RowAccess for ScalarRow<'_> {
    fn get(&self, name: &str) -> Option<&Value> {
        <[(String, Value)] as RowAccess>::get(&self.entries, name)
    }
    fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }
    fn column_type(&self, name: &str) -> Option<&DataType> {
        let index = self
            .entries
            .iter()
            .position(|(column, _)| column == name)
            .or_else(|| {
                let reference = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
                let candidates = reference.row_lookup_candidates();
                self.entries
                    .iter()
                    .position(|(column, _)| candidates.iter().any(|candidate| candidate == column))
            })?;
        Some(&self.schema[index].1)
    }
    fn entry_type(&self, index: usize) -> Option<&DataType> {
        self.schema.get(index).map(|(_, data_type)| data_type)
    }
    fn has_array_types(&self) -> bool {
        self.schema
            .iter()
            .any(|(_, data_type)| matches!(data_type, DataType::Array(_)))
    }
}

pub(super) struct Environment<'a> {
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) params: &'a [Value],
    pub(super) functions: &'a HashMap<String, FunctionMeta>,
    pub(super) session: Option<&'a CassieSession>,
}

impl Environment<'_> {
    pub(super) fn evaluate(
        &self,
        batch: &TypedBatch,
        lane: usize,
        expr: &Expr,
    ) -> Result<Accounted<Value>, QueryError> {
        check_controls(self.controls)?;
        let mut bytes = mul(batch.schema().len(), size_of::<(String, Value)>())?;
        for (index, (name, _)) in batch.schema().iter().enumerate() {
            bytes = add(
                bytes,
                add(name.len(), cell_heap(batch.cell(index, lane)?)?)?,
            )?;
        }
        // Parser/name resolution, scalar clones and serialized JSON temporaries share
        // this explicit lane admission. Only the selected expression enters the adapter.
        bytes = add(
            mul(
                add(bytes.max(64), literal_bytes(expr, self.params)?)?,
                expression_weight(expr),
            )?,
            1024,
        )?;
        let memory = self.controls.reserve_query_memory(bytes)?;
        let mut entries = Vec::with_capacity(batch.schema().len());
        for (index, (name, _)) in batch.schema().iter().enumerate() {
            entries.push((name.clone(), batch.cell(index, lane)?.to_owned()));
        }
        let row = ScalarRow {
            entries,
            schema: batch.schema(),
        };
        let value = filter::evaluate_expr_value(
            &row,
            expr,
            self.params,
            None,
            self.functions,
            self.session,
            None,
        )?;
        check_controls(self.controls)?;
        Ok(super::admitted(value, memory))
    }

    pub(super) fn result_type(
        &self,
        batch: &TypedBatch,
        expr: &Expr,
    ) -> Result<Accounted<DataType>, QueryError> {
        let bytes = batch.schema().iter().try_fold(
            add(
                1024,
                add(
                    mul(self.params.len(), size_of::<i32>())?,
                    mul(batch.schema().len(), size_of::<crate::types::FieldSchema>())?,
                )?,
            )?,
            |bytes, (name, data_type)| {
                add(
                    bytes,
                    add(
                        mul(name.len(), 64)?,
                        crate::executor::retained_memory::data_type_clone_bytes(data_type)?,
                    )?,
                )
            },
        )?;
        let memory = self.controls.reserve_query_memory(mul(
            add(bytes, literal_bytes(expr, self.params)?)?,
            expression_weight(expr),
        )?)?;
        let schema = crate::types::Schema::from_iter(batch.schema().iter().cloned());
        let parameter_types = self.params.iter().map(parameter_oid).collect::<Vec<_>>();
        let data_type =
            crate::sql::binder::infer_expr_type(expr, &schema, self.functions, &parameter_types)
                .unwrap_or(DataType::Null);
        Ok(super::admitted(data_type, memory))
    }
}

fn parameter_oid(value: &Value) -> i32 {
    let data_type = match value {
        Value::Null => DataType::Null,
        Value::Bool(_) => DataType::Boolean,
        Value::Int64(_) => DataType::BigInt,
        Value::Float64(_) => DataType::Float,
        Value::String(_) => DataType::Text,
        Value::Vector(value) => DataType::Vector(value.values.len()),
        Value::Json(_) => DataType::Json,
    };
    i32::try_from(data_type.type_oid()).unwrap_or(0)
}

pub(crate) fn cell_heap(cell: Cell<'_>) -> Result<usize, crate::app::CassieError> {
    match cell {
        Cell::Text(text) => Ok(text.len()),
        Cell::Scalar(Value::Json(value)) => add(
            crate::runtime::accounted::json::retained_bytes(value)?
                - size_of::<serde_json::Value>(),
            serialized_json_bytes(value)?,
        ),
        Cell::Scalar(value) => value_clone_bytes(value),
        _ => Ok(0),
    }
}

fn expression_weight(expr: &Expr) -> usize {
    let mut weight = 4_usize;
    expr.for_each_child(|child| {
        weight = weight.saturating_add(expression_weight(child));
    });
    weight
}

fn literal_bytes(expr: &Expr, params: &[Value]) -> Result<usize, crate::app::CassieError> {
    let mut bytes = match expr {
        Expr::StringLiteral(text) => Ok(text.len()),
        Expr::Cast { data_type, .. } => {
            crate::executor::retained_memory::data_type_clone_bytes(data_type)
        }
        Expr::Param(index) => params.get(*index).map_or(Ok(0), value_clone_bytes),
        _ => Ok(0),
    };
    expr.for_each_child(|child| {
        let previous = std::mem::replace(&mut bytes, Ok(0));
        bytes = previous.and_then(|bytes| add(bytes, literal_bytes(child, params)?));
    });
    bytes
}
