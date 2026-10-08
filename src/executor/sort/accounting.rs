use std::collections::HashMap;
use std::mem::size_of;
use std::ops::ControlFlow;

use crate::app::CassieError;
use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::retained_memory::{
    add, data_type_clone_bytes, mul, serialized_json_bytes, value_clone_bytes,
};
use crate::executor::semantic::SemanticValue;
use crate::runtime::accounted::json;
use crate::sql::ast::Expr;
use crate::types::{DataType, Value};

pub(crate) fn type_scratch<R: RowAccess>(
    row: &R,
    expr: &Expr,
    functions: &HashMap<String, FunctionMeta>,
) -> Result<usize, CassieError> {
    if !row.has_array_types() {
        return Ok(0);
    }
    let mut maximum_type = row.maximum_type_heap_bytes()?;
    for function in functions.values() {
        maximum_type = maximum_type.max(data_type_clone_bytes(&function.return_type)?);
    }
    let mut shape = TypeShape {
        nodes: 0,
        longest_name: 0,
        maximum_type,
    };
    shape.visit(expr)?;
    // Schema fields, inferred argument vectors/types and identifier lookup scratch coexist.
    mul(
        shape.nodes,
        add(
            512,
            add(mul(shape.longest_name, 32)?, mul(shape.maximum_type, 4)?)?,
        )?,
    )
}

struct TypeShape {
    nodes: usize,
    longest_name: usize,
    maximum_type: usize,
}

impl TypeShape {
    fn visit(&mut self, expr: &Expr) -> Result<(), CassieError> {
        self.nodes = add(self.nodes, 1)?;
        match expr {
            Expr::Column(name) => self.longest_name = self.longest_name.max(name.len()),
            Expr::Function(function) => {
                self.longest_name = self.longest_name.max(function.name.len());
            }
            Expr::Cast { data_type, .. } => {
                self.maximum_type = self.maximum_type.max(data_type_clone_bytes(data_type)?);
            }
            _ => {}
        }
        match expr.try_for_each_child(|child| match self.visit(child) {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(error),
        }) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(error) => Err(error),
        }
    }
}

pub(crate) fn conversion_bytes(
    value: &Value,
    data_type: Option<&DataType>,
) -> Result<usize, CassieError> {
    let scalar = add(size_of::<Value>(), value_clone_bytes(value)?)?;
    let retained = match data_type {
        Some(DataType::Array(element)) => match value {
            Value::Json(value) if value.is_array() => array_bytes(value, element)?,
            Value::String(text) => {
                // A string-backed ARRAY may parse a JSON tree while its semantic key is built.
                // Two independent decoder-sized bounds cover that tree, cloned elements and
                // recursive semantic vectors; malformed text retains the ordinary string key.
                add(mul(json::serialized_decode_bytes(text.len())?, 2)?, 256)?
            }
            _ => ordinary_bytes(value)?,
        },
        _ => ordinary_bytes(value)?,
    };
    add(scalar, retained)
}

fn ordinary_bytes(value: &Value) -> Result<usize, CassieError> {
    match value {
        Value::String(text) => Ok(text.len().max(27)),
        Value::Vector(vector) => mul(vector.values.len(), size_of::<u32>()),
        Value::Json(value) => json_string_bytes(value),
        Value::Float64(_) => Ok(64),
        Value::Null | Value::Bool(_) | Value::Int64(_) => Ok(0),
    }
}

fn json_string_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    // The locked serializer starts with128bytes and grows geometrically.
    Ok(mul(serialized_json_bytes(value)?, 2)?.max(128))
}

fn array_bytes(value: &serde_json::Value, element_type: &DataType) -> Result<usize, CassieError> {
    let Some(values) = value.as_array() else {
        return json_string_bytes(value);
    };
    values.iter().try_fold(
        mul(values.len(), size_of::<SemanticValue>())?,
        |bytes, value| add(bytes, element_bytes(value, element_type)?),
    )
}

fn element_bytes(value: &serde_json::Value, data_type: &DataType) -> Result<usize, CassieError> {
    if matches!(data_type, DataType::Json) && !value.is_null() {
        return add(json::retained_bytes(value)?, json_string_bytes(value)?);
    }
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) => Ok(0),
        serde_json::Value::Number(_) => Ok(64),
        serde_json::Value::String(text) => {
            // Canonical BYTEA may double text; temporal/UUID normalization stays below64.
            // Canonical and final semantic strings can coexist with formatting scratch.
            mul(add(mul(text.len(), 2)?, 64)?, 3)
        }
        serde_json::Value::Array(_) if matches!(data_type, DataType::Vector(_)) => {
            array_bytes(value, &DataType::Float)
        }
        _ => add(json::retained_bytes(value)?, json_string_bytes(value)?),
    }
}

pub(crate) fn semantic_heap(value: &SemanticValue) -> Result<usize, CassieError> {
    match value {
        SemanticValue::String(value) | SemanticValue::Json(value) => Ok(value.capacity()),
        SemanticValue::Vector(values) => mul(values.capacity(), size_of::<u32>()),
        SemanticValue::Array(values) => values.iter().try_fold(
            mul(values.capacity(), size_of::<SemanticValue>())?,
            |bytes, value| add(bytes, semantic_heap(value)?),
        ),
        SemanticValue::Null | SemanticValue::Bool(_) | SemanticValue::Number(_) => Ok(0),
    }
}

pub(crate) fn tie_bytes(row: &impl RowAccess) -> Result<usize, CassieError> {
    let mut text_bytes = row.entries().len().saturating_sub(1);
    let mut scratch = 0;
    for (_, value) in row.entries() {
        let length = match value {
            Value::Null => 6,
            Value::Bool(_) => 5,
            Value::Int64(_) => {
                scratch = scratch.max(32);
                20
            }
            Value::Float64(_) => {
                scratch = scratch.max(1_024);
                350
            }
            Value::String(text) => text.len(),
            Value::Vector(vector) => {
                scratch = scratch.max(1_024);
                mul(vector.values.len(), 351)?
            }
            Value::Json(value) => {
                scratch = scratch.max(json_string_bytes(value)?);
                serialized_json_bytes(value)?
            }
        };
        text_bytes = add(text_bytes, length)?;
    }
    let backing = if text_bytes == 0 {
        0
    } else {
        mul(text_bytes, 2)?.max(8)
    };
    add(backing, scratch)
}
