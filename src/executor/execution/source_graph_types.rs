//! `cassie_graph.key_parts`, `path_steps` and `weighted_path_steps` as FROM
//! sources (#1268). The rows are computed from the supplied value alone: the
//! functions never consult current graph rows.
use super::{
    batch, finalize_source_batches, BatchRow, QueryError, SourceExecution, SourceExecutionEnv,
    Value,
};
use crate::app::{CassieError, DiagnosticError};
use crate::types::extended::numeric::math;
use crate::types::extended::numeric::Numeric;
use crate::types::graph::{ElementRef, GraphDescriptors, KeyPart, Path};
use crate::types::typed::TypedValue;

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
    let args = super::source_table_function::evaluate_args(env, function, outer_row)?;
    let local = name.rsplit('.').next().unwrap_or(name);
    let rows = match (local, args.as_slice()) {
        ("key_parts", [reference]) => key_part_rows(reference)?,
        ("path_steps", [path]) => step_rows(env, path, None)?,
        // A strict function: a NULL path or NULL costs yield no rows.
        ("weighted_path_steps", [_, Value::Null]) => Vec::new(),
        ("weighted_path_steps", [path, costs]) => step_rows(env, path, Some(costs))?,
        _ => return Err(failure("42883", format!("function {name} does not exist"))),
    };
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

fn row(columns: Vec<(&str, Value)>) -> BatchRow {
    BatchRow::new(
        columns
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

fn key_part_rows(argument: &Value) -> Result<Vec<BatchRow>, QueryError> {
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
    Ok(reference
        .key
        .iter()
        .enumerate()
        .map(|(index, part)| {
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
            row(vec![
                (
                    "ordinal",
                    Value::Int64(i64::try_from(index + 1).unwrap_or(i64::MAX)),
                ),
                ("type_tag", Value::Int64(i64::from(part.type_tag()))),
                ("integer_value", integer),
                ("text_value", text),
                ("uuid_value", uuid),
            ])
        })
        .collect())
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
) -> Result<Vec<BatchRow>, QueryError> {
    let Some(path) = crate::executor::filter::graph_functions::path_argument(argument)? else {
        return Ok(Vec::new());
    };
    let costs = costs.map(|costs| edge_costs(costs, &path)).transpose()?;
    // Every cumulative cost is computed before the first row is emitted, so
    // an overflow never yields a partial result.
    let cumulative = costs.as_deref().map(cumulative_costs).transpose()?;
    let descriptors = crate::executor::filter::graph_functions::descriptors_of(env.session)?;
    (0..path.vertices.len())
        .map(|index| step(&descriptors, &path, index, cumulative.as_deref()))
        .collect()
}

fn cumulative_costs(costs: &[Numeric]) -> Result<Vec<Numeric>, QueryError> {
    let mut totals = Vec::with_capacity(costs.len() + 1);
    totals.push(Numeric::zero());
    for cost in costs {
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
