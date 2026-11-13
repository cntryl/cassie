//! Bounded `JSON_TABLE` FROM source.
//!
//! The context item is decoded under a reserved bound, every created row is admitted to the
//! query memory budget before it is retained, and rows are published only once the whole
//! result exists, so an error or a resource limit never exposes a partial result.
use std::mem::size_of;
use std::sync::Arc;

use serde_json::Value as Json;

use super::source_json_fragment::{decode_document, evaluate_argument, reserve_decode};
use super::source_json_table_columns::{diagnostic, Evaluator};
use super::{BatchRow, CteContext, QueryError, SourceExecution, SourceExecutionEnv, Value};
use crate::app::CassieError;
use crate::executor::retained_memory::value_clone_bytes;
use crate::sql::ast::{
    FunctionCall, JsonTableColumn, JsonTableRootError, JsonTableSpec, TableFunctionForm,
};
use crate::sql::table_sources;
use crate::types::DataType;

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    form: Option<&TableFunctionForm>,
    outer: Option<&BatchRow>,
    row_budget: Option<usize>,
    ctes: &CteContext,
) -> SourceExecution {
    env.controls.check_interruption()?;
    let Some(TableFunctionForm::JsonTable(spec)) = form else {
        return Err(QueryError::General(
            "JSON_TABLE requires a specification".into(),
        ));
    };
    if row_budget == Some(0) {
        return Ok((Vec::new(), Vec::new()));
    }
    let context = evaluate_argument(env, &function.args[0], outer, ctes)?;
    let mut passing = serde_json::Map::new();
    for (name, expression) in spec.passing.iter().zip(&function.args[1..]) {
        let value = evaluate_argument(env, expression, outer, ctes)?;
        passing.insert(name.clone(), passing_value(&value)?);
    }
    let _decode = reserve_decode(env, &context)?;
    let Some(document) = decode_document(env, true, "json_table", context)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let document = document.into_tree()?;
    let mut evaluator = Evaluator::new(env, ctes, Json::Object(passing));
    validate_defaults(&evaluator, &spec.columns)?;
    let mut sink = Sink::new(env, spec, row_budget)?;
    // Selected items are copies of parts of the document: reserve their upper bound first.
    sink.reserve_tree(&document)?;
    let strict = spec.on_error == JsonTableRootError::Error;
    let roots = match evaluator.items(&spec.path, &document)? {
        Ok(items) => items,
        Err((state, message)) => {
            if strict {
                return Err(diagnostic(state, message));
            }
            Vec::new()
        }
    };
    let walk = Walk { strict };
    for (index, item) in roots.iter().enumerate() {
        env.controls.check_interruption()?;
        let rows = level_rows(
            &mut evaluator,
            &spec.columns,
            item,
            index + 1,
            (&walk, &mut sink),
        )?;
        for row in rows {
            if sink.full() {
                break;
            }
            sink.push(row)?;
        }
        if sink.full() {
            break;
        }
    }
    Ok((
        super::batch::chunk_rows(sink.seal(env)?, super::batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
    ))
}

fn passing_value(value: &Value) -> Result<Json, QueryError> {
    match value {
        Value::Null => Ok(Json::Null),
        Value::Bool(flag) => Ok(Json::Bool(*flag)),
        Value::Int64(number) => Ok(Json::from(*number)),
        Value::Float64(number) => serde_json::Number::from_f64(*number)
            .map(Json::Number)
            .ok_or_else(|| diagnostic("22003", "JSON_TABLE PASSING value is not a finite number")),
        Value::String(text) => Ok(Json::String(text.clone())),
        Value::Json(document) => Ok(document.clone()),
        _ => Err(CassieError::Unsupported(
            "JSON_TABLE PASSING supports boolean, numeric, text and jsonb values".into(),
        )
        .into()),
    }
}

fn validate_defaults(
    evaluator: &Evaluator<'_, '_>,
    columns: &[JsonTableColumn],
) -> Result<(), QueryError> {
    for column in columns {
        match column {
            JsonTableColumn::Value(column) => evaluator.validate_defaults(column)?,
            JsonTableColumn::Nested(nested) => validate_defaults(evaluator, &nested.columns)?,
            JsonTableColumn::Ordinality { .. } | JsonTableColumn::Exists(_) => {}
        }
    }
    Ok(())
}

/// Number of output columns a column list contributes, nested columns included.
fn width(columns: &[JsonTableColumn]) -> usize {
    columns
        .iter()
        .map(|column| match column {
            JsonTableColumn::Nested(nested) => width(&nested.columns),
            _ => 1,
        })
        .sum()
}

/// Walk-wide settings: whether nested path errors raise (root `ERROR ON ERROR`).
struct Walk {
    strict: bool,
}

/// Rows for one item of a path: own columns plus the union of the nested sibling rows.
/// A parent without nested rows keeps one row with the nested columns NULL.
fn level_rows(
    evaluator: &mut Evaluator<'_, '_>,
    columns: &[JsonTableColumn],
    item: &Json,
    ordinal: usize,
    (walk, sink): (&Walk, &mut Sink<'_, '_>),
) -> Result<Vec<Vec<Value>>, QueryError> {
    let mut local = Vec::with_capacity(width(columns));
    let mut nested = Vec::new();
    for column in columns {
        match column {
            JsonTableColumn::Ordinality { .. } => {
                let ordinal = i64::try_from(ordinal).map_err(|_| {
                    CassieError::ResourceLimit("JSON_TABLE ordinality overflow".into())
                })?;
                local.push(Value::Int64(ordinal));
            }
            JsonTableColumn::Value(column) => local.push(evaluator.value_column(column, item)?),
            JsonTableColumn::Exists(column) => local.push(evaluator.exists_column(column, item)?),
            JsonTableColumn::Nested(child) => {
                nested.push((local.len(), child));
                local.resize(local.len() + width(&child.columns), Value::Null);
            }
        }
    }
    if nested.is_empty() {
        sink.admit(&local)?;
        return Ok(vec![local]);
    }
    let mut rows = Vec::new();
    for (start, child) in nested {
        sink.reserve_tree(item)?;
        let items = match evaluator.items(&child.path, item)? {
            Ok(items) => items,
            Err((state, message)) if walk.strict => return Err(diagnostic(state, message)),
            Err(_) => Vec::new(),
        };
        for (index, selected) in items.iter().enumerate() {
            let parts = level_rows(evaluator, &child.columns, selected, index + 1, (walk, sink))?;
            for part in parts {
                let mut row = local.clone();
                row.splice(start..start + part.len(), part);
                sink.admit(&row)?;
                rows.push(row);
            }
        }
    }
    if rows.is_empty() {
        sink.admit(&local)?;
        rows.push(local);
    }
    Ok(rows)
}

/// Accumulates rows under the query memory budget.
struct Sink<'a, 'e> {
    env: &'a SourceExecutionEnv<'e>,
    memory: crate::runtime::QueryMemoryReservation,
    types: Arc<Vec<DataType>>,
    rows: Vec<BatchRow>,
    budget: usize,
    overhead: usize,
}

impl<'a, 'e> Sink<'a, 'e> {
    fn new(
        env: &'a SourceExecutionEnv<'e>,
        spec: &JsonTableSpec,
        row_budget: Option<usize>,
    ) -> Result<Self, QueryError> {
        let types = spec
            .output_columns()
            .into_iter()
            .map(|(_, data_type)| data_type)
            .collect::<Vec<_>>();
        let slots = types
            .len()
            .saturating_mul(size_of::<(String, Value)>() + 32);
        let mut memory = env.controls.reserve_query_memory(0)?;
        memory.try_grow(size_of::<Vec<BatchRow>>())?;
        Ok(Self {
            env,
            memory,
            types: Arc::new(types),
            rows: Vec::new(),
            budget: row_budget.unwrap_or(usize::MAX),
            overhead: size_of::<BatchRow>()
                .saturating_add(slots)
                .saturating_add(64),
        })
    }

    fn full(&self) -> bool {
        self.rows.len() >= self.budget
    }

    /// Reserves the retained size of a JSON tree that is about to be copied.
    fn reserve_tree(&mut self, tree: &Json) -> Result<(), QueryError> {
        self.memory
            .try_grow(crate::runtime::accounted::json::retained_bytes(tree)?)?;
        Ok(())
    }

    /// Reserves the retained size of a row about to be created.
    fn admit(&mut self, values: &[Value]) -> Result<(), QueryError> {
        self.env.controls.check_interruption()?;
        let mut bytes = self.overhead;
        for value in values {
            bytes = bytes.saturating_add(value_clone_bytes(value)?);
        }
        self.memory.try_grow(bytes)?;
        Ok(())
    }

    fn push(&mut self, values: Vec<Value>) -> Result<(), QueryError> {
        self.rows.try_reserve(1).map_err(|error| {
            CassieError::ResourceLimit(format!("unable to retain JSON_TABLE rows: {error}"))
        })?;
        let columns = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (table_sources::physical_name(index), value))
            .collect::<Vec<_>>();
        self.rows.push(
            BatchRow::from_projected_values(columns)
                .with_optional_data_types(Some(Arc::clone(&self.types))),
        );
        Ok(())
    }

    /// Attaches the shared reservation to every row once the whole result exists.
    fn seal(self, env: &SourceExecutionEnv<'_>) -> Result<Vec<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        let reservation = Arc::new(self.memory);
        let mut rows = self.rows;
        for row in &mut rows {
            row.attach_operator_memory(env.controls, Arc::clone(&reservation))?;
        }
        Ok(rows)
    }
}
