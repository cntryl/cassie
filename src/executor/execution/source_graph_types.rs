//! `cassie_graph.key_parts`, `path_steps` and `weighted_path_steps` as FROM
//! sources (#1268). The rows are computed from the supplied value alone: the
//! functions never consult current graph rows.
use super::{
    batch, finalize_source_batches, BatchRow, QueryError, SourceExecution, SourceExecutionEnv,
    Value,
};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::retained_memory::{add, grown_capacity, lookup_bytes, mul, value_clone_bytes};
use crate::runtime::QueryMemoryReservation;
use crate::types::extended::numeric::math;
use crate::types::extended::numeric::Numeric;
use crate::types::graph::{ElementRef, GraphDescriptors, KeyPart, Path};
use crate::types::typed::TypedValue;
use std::mem::size_of;
use std::sync::Arc;

fn failure(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::Cassie(CassieError::from(DiagnosticError::new(state, message)))
}

/// Runs `function` when it is one of the `cassie_graph` table functions.
pub(super) fn dispatch(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> Option<SourceExecution> {
    crate::sql::functions::graph_types::is_table_function(&function.name)
        .then(|| execute(env, name, function, outer_row, qualify))
}

fn execute(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    let arguments = super::source_table_function::evaluate_args(env, function, outer_row)?;
    let args = &arguments.0;
    let local = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    #[cfg(test)]
    crate::executor::execution::graph_memory_test_observer::record(
        "before_graph_rows",
        env.controls.current_query_memory_bytes(),
    );
    let mut output = env.controls.reserve_query_memory(0)?;
    let rows = match (local.as_str(), args.as_slice()) {
        ("key_parts", [reference]) => key_part_rows(env, reference, &mut output)?,
        ("path_steps", [path]) => step_rows(env, path, None, &mut output)?,
        // A strict function: a NULL path or NULL costs yield no rows.
        ("weighted_path_steps", [_, Value::Null]) => Vec::new(),
        ("weighted_path_steps", [path, costs]) => step_rows(env, path, Some(costs), &mut output)?,
        _ => return Err(failure("42883", format!("function {name} does not exist"))),
    };
    env.controls.check_interruption()?;
    // Batch directories and the shared owner are admitted before chunking/attachment.
    output.try_grow(add(256, mul(rows.len(), size_of::<Vec<BatchRow>>())?)?)?;
    let lease = Arc::new(output);
    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    for row in batches.iter_mut().flatten() {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&lease)));
    }
    finalize_source_batches(env, batches, Vec::new(), qualify, name)
}

/// Includes the entries directory, its temporary input slots and eager row lookup.
fn row_bytes(names: &[&str]) -> Result<usize, QueryError> {
    let names_bytes = names
        .iter()
        .try_fold(0, |bytes, name| add(bytes, name.len()))?;
    Ok(add(
        size_of::<BatchRow>() + 64,
        add(
            mul(names.len(), 2 * size_of::<(String, Value)>())?,
            add(names_bytes, lookup_bytes(names.len(), names_bytes)?)?,
        )?,
    )?)
}

/// A decoded graph model only owns element/key vectors and copied text. The native
/// record field directories already bound those slots; two copies include decode scratch.
fn model_memory(
    env: &SourceExecutionEnv<'_>,
    argument: &Value,
) -> Result<QueryMemoryReservation, QueryError> {
    Ok(env
        .controls
        .reserve_query_memory(add(512, mul(value_clone_bytes(argument)?, 2)?)?)?)
}

fn row(columns: Vec<(&str, Value)>) -> BatchRow {
    BatchRow::new(
        columns
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

fn key_part_rows(
    env: &SourceExecutionEnv<'_>,
    argument: &Value,
    output: &mut QueryMemoryReservation,
) -> Result<Vec<BatchRow>, QueryError> {
    env.controls.check_interruption()?;
    let _model_memory = model_memory(env, argument)?;
    let reference = match argument {
        Value::Null => return Ok(Vec::new()),
        Value::Typed(TypedValue::Record(record)) => ElementRef::from_record(record)
            .map_err(|error| failure("22023", error.message().to_owned()))?,
        _ => {
            return Err(failure(
                "42804",
                "argument must be of type cassie_graph.graph_element_ref",
            ))
        }
    };
    let base = row_bytes(&[
        "ordinal",
        "type_tag",
        "integer_value",
        "text_value",
        "uuid_value",
    ])?;
    output.try_grow(mul(
        grown_capacity(reference.key.len(), 4)?,
        size_of::<BatchRow>(),
    )?)?;
    for part in &reference.key {
        let heap = match part {
            KeyPart::Text(text) => text.len(),
            KeyPart::Uuid(_) => 36,
            _ => 0,
        };
        output.try_grow(add(base, heap)?)?;
    }
    reference
        .key
        .iter()
        .enumerate()
        .map(|(index, part)| {
            env.controls.check_interruption()?;
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record_current(
                "copy_graph_rows",
            );
            let integer = match part {
                KeyPart::SmallInt(value) => Value::Int64(i64::from(*value)),
                KeyPart::Int(value) => Value::Int64(i64::from(*value)),
                KeyPart::BigInt(value) => Value::Int64(*value),
                KeyPart::Text(_) | KeyPart::Uuid(_) => Value::Null,
            };
            let text = match part {
                KeyPart::Text(value) => Value::String(value.clone()),
                _ => Value::Null,
            };
            let uuid = match part {
                KeyPart::Uuid(value) => Value::String(value.to_string()),
                _ => Value::Null,
            };
            Ok(row(vec![
                (
                    "ordinal",
                    Value::Int64(i64::try_from(index + 1).unwrap_or(i64::MAX)),
                ),
                ("type_tag", Value::Int64(i64::from(part.type_tag()))),
                ("integer_value", integer),
                ("text_value", text),
                ("uuid_value", uuid),
            ]))
        })
        .collect::<Result<Vec<_>, QueryError>>()
}

/// One finite, non-negative NUMERIC cost.
fn cost(value: &Value) -> Result<Numeric, QueryError> {
    let numeric = match value {
        Value::Typed(TypedValue::Numeric(numeric)) => numeric.clone(),
        Value::Int64(number) => Numeric::from_i64(*number),
        Value::Float64(number) => {
            Numeric::from_f64(*number).map_err(|error| failure("22023", error.message))?
        }
        Value::Null => return Err(failure("22004", "null value not allowed in edge_costs")),
        _ => return Err(failure("42804", "edge_costs must be numeric[]")),
    };
    match numeric.signum() {
        Some(sign) if sign >= 0 && !matches!(numeric, Numeric::Infinity) => Ok(numeric),
        _ => Err(failure(
            "22023",
            "edge_costs must be finite and non-negative",
        )),
    }
}

/// The per-edge costs of a weighted call, validated against the path.
fn edge_costs(costs: &Value, path: &Path) -> Result<Vec<Numeric>, QueryError> {
    let Value::Array(array) = costs else {
        return Err(failure("42804", "edge_costs must be numeric[]"));
    };
    let parsed = array
        .elements()
        .iter()
        .map(cost)
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.len() == path.edges.len() {
        Ok(parsed)
    } else {
        Err(failure(
            "22023",
            format!(
                "edge_costs has {} elements but the path has {} edges",
                parsed.len(),
                path.edges.len()
            ),
        ))
    }
}

fn step_rows(
    env: &SourceExecutionEnv<'_>,
    argument: &Value,
    costs: Option<&Value>,
    output: &mut QueryMemoryReservation,
) -> Result<Vec<BatchRow>, QueryError> {
    env.controls.check_interruption()?;
    let _model_memory = model_memory(env, argument)?;
    let Some(path) = crate::executor::filter::graph_functions::path_argument(argument)? else {
        return Ok(Vec::new());
    };
    let numeric_bytes = costs.map_or(Ok(0), numeric_bound)?;
    let _cost_memory = env.controls.reserve_query_memory(mul(
        add(mul(path.edges.len(), 2)?, 1)?,
        add(size_of::<Numeric>(), numeric_bytes)?,
    )?)?;
    let costs = costs.map(|costs| edge_costs(costs, &path)).transpose()?;
    // Every cumulative cost is computed before the first row is emitted, so
    // an overflow never yields a partial result.
    let cumulative = costs
        .as_deref()
        .map(|costs| cumulative_costs(env, costs))
        .transpose()?;
    let descriptors = crate::executor::filter::graph_functions::descriptors_of(env.session)?;
    let mut names = vec!["position", "direction", "vertex", "edge"];
    if cumulative.is_some() {
        names.push("cumulative_cost");
    }
    let base = row_bytes(&names)?;
    output.try_grow(mul(
        grown_capacity(path.vertices.len(), 4)?,
        size_of::<BatchRow>(),
    )?)?;
    for (index, vertex) in path.vertices.iter().enumerate() {
        env.controls.check_interruption()?;
        let edge = index
            .checked_sub(1)
            .and_then(|index| path.edges.get(index))
            .map_or(Ok(0), element_bytes)?;
        output.try_grow(add(
            base,
            add(element_bytes(vertex)?, add(edge, numeric_bytes)?)?,
        )?)?;
    }
    (0..path.vertices.len())
        .map(|index| {
            env.controls.check_interruption()?;
            step(&descriptors, &path, index, cumulative.as_deref())
        })
        .collect()
}

/// Bound nested native reference fields, UUID text, key records and array directories.
fn element_bytes(element: &ElementRef) -> Result<usize, QueryError> {
    let mut bytes = add(
        mul(5, size_of::<Value>())?,
        add(3 * 36, size_of::<crate::types::ArrayDimension>())?,
    )?;
    bytes = add(
        bytes,
        mul(grown_capacity(element.key.len(), 4)?, size_of::<Value>())?,
    )?;
    bytes = add(bytes, mul(element.key.len(), 4 * size_of::<Value>())?)?;
    for part in &element.key {
        bytes = add(
            bytes,
            match part {
                KeyPart::Text(text) => text.len(),
                KeyPart::Uuid(_) => 36,
                _ => 0,
            },
        )?;
    }
    Ok(bytes)
}

/// Addition can align each operand to the largest scale and gain at most one
/// binary carry per input. Four bytes per decimal place conservatively cover that
/// alignment before arithmetic allocates; no formatted numeric string is required.
fn numeric_bound(costs: &Value) -> Result<usize, QueryError> {
    let Value::Array(array) = costs else {
        return Ok(0);
    };
    let mut bytes = 512;
    for value in array.elements() {
        let scale = match value {
            Value::Typed(TypedValue::Numeric(numeric)) => numeric.scale().unwrap_or(0),
            _ => 0,
        };
        bytes = bytes.max(add(
            value_clone_bytes(value)?,
            mul(usize::try_from(scale).unwrap_or(usize::MAX), 4)?,
        )?);
    }
    Ok(add(bytes, array.len())?)
}

fn cumulative_costs(
    env: &SourceExecutionEnv<'_>,
    costs: &[Numeric],
) -> Result<Vec<Numeric>, QueryError> {
    let mut totals = Vec::with_capacity(costs.len() + 1);
    totals.push(Numeric::zero());
    for cost in costs {
        env.controls.check_interruption()?;
        let previous = totals.last().cloned().unwrap_or_else(Numeric::zero);
        totals.push(
            math::add(&previous, cost).map_err(|error| failure(error.sqlstate, error.message))?,
        );
    }
    Ok(totals)
}

fn step(
    descriptors: &GraphDescriptors,
    path: &Path,
    index: usize,
    cumulative: Option<&[Numeric]>,
) -> Result<BatchRow, QueryError> {
    #[cfg(test)]
    crate::executor::execution::graph_memory_test_observer::record_current("copy_graph_rows");
    let encode = |element: &ElementRef| {
        element
            .to_value(descriptors)
            .map_err(|message| failure("XX000", message))
    };
    let previous = index.checked_sub(1);
    let direction = previous
        .and_then(|edge| path.directions.get(edge))
        .map_or(Value::Null, |direction| {
            Value::Int64(i64::from(direction.tag()))
        });
    let edge = match previous.and_then(|edge| path.edges.get(edge)) {
        Some(edge) => encode(edge)?,
        None => Value::Null,
    };
    let mut columns = vec![
        (
            "position",
            Value::Int64(i64::try_from(index).unwrap_or(i64::MAX)),
        ),
        ("direction", direction),
        ("vertex", encode(&path.vertices[index])?),
        ("edge", edge),
    ];
    if let Some(totals) = cumulative {
        columns.push((
            "cumulative_cost",
            Value::Typed(TypedValue::Numeric(totals[index].clone())),
        ));
    }
    Ok(row(columns))
}

#[cfg(test)]
#[path = "source_graph_types_memory_tests.rs"]
mod memory_tests;
