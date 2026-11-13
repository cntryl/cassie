//! Bounded `json[b]_to_record[set]` and `json[b]_populate_record[set]` FROM sources.
//!
//! The input decode bound is reserved before parsing and held until the rows exist; the
//! retained size of every row is admitted before its fields are converted, so a malformed
//! document, a failed conversion or a resource limit never publishes a partial result.
use std::mem::size_of;
use std::sync::Arc;

use super::source_json_fragment::{
    decode_document, evaluate_argument, reserve_decode, Fragment, Kind,
};
use super::source_json_record_fields::{base_fields, field_values, not_an_object};
use super::{BatchRow, QueryError, SourceExecution, SourceExecutionEnv, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::retained_memory::value_clone_bytes;
use crate::sql::ast::{FunctionCall, TableFunctionForm};
use crate::sql::table_sources::{self, RecordSource};
use crate::types::DataType;

fn diagnostic(state: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::from(CassieError::from(DiagnosticError::new(state, message)))
}

/// Per-call state: the declared columns, the optional base row and the output descriptors.
struct Shape {
    declared: Vec<(String, DataType)>,
    base: Option<Value>,
    types: Arc<Vec<DataType>>,
    ordinal: bool,
}

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    form: Option<&TableFunctionForm>,
    ordinal: bool,
    outer: Option<&BatchRow>,
    row_budget: Option<usize>,
    ctes: &super::CteContext,
) -> SourceExecution {
    env.controls.check_interruption()?;
    let record = table_sources::record_source(&function.name).ok_or_else(|| {
        QueryError::General("record expansion requires a selected function".into())
    })?;
    if row_budget == Some(0) {
        return Ok((Vec::new(), Vec::new()));
    }
    let declared = table_sources::record_declared_columns(function, form)
        .ok_or_else(|| QueryError::General("record expansion requires a row type".into()))?;
    let columns = table_sources::record_output_columns(function, ordinal, form)
        .ok_or_else(|| QueryError::General("record expansion requires a row type".into()))?;
    let base = if record.populate {
        Some(evaluate_argument(env, &function.args[0], outer, ctes)?)
    } else {
        None
    };
    let argument = evaluate_argument(env, &function.args[record.document_index()], outer, ctes)?;
    let _decode = reserve_decode(env, &argument)?;
    let root = decode_document(env, record.binary, &record.function_name(), argument)?;
    let shape = Shape {
        types: Arc::new(
            columns
                .into_iter()
                .map(|(_, data_type)| data_type)
                .collect(),
        ),
        declared,
        base,
        ordinal,
    };
    let rows = if record.set {
        set_rows(env, record, &shape, root, row_budget)?
    } else {
        single_row(env, record, &shape, root)?
    };
    Ok((
        super::batch::chunk_rows(rows, super::batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
    ))
}

fn single_row(
    env: &SourceExecutionEnv<'_>,
    record: RecordSource,
    shape: &Shape,
    root: Option<Fragment>,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut builder = RowBuilder::new(env, shape)?;
    let values = match root {
        // A SQL NULL document yields the base row (all NULL without a base).
        None => base_values(shape),
        Some(document) => match document.kind() {
            Kind::Object => {
                builder.admit_input(document.retained_bytes()?)?;
                convert_object(document, shape)?
            }
            kind => return Err(not_an_object(kind)),
        },
    };
    let row = builder.finish(values, 1)?;
    builder.seal(vec![row], env)
}

fn set_rows(
    env: &SourceExecutionEnv<'_>,
    record: RecordSource,
    shape: &Shape,
    root: Option<Fragment>,
    row_budget: Option<usize>,
) -> Result<Vec<BatchRow>, QueryError> {
    let Some(document) = root else {
        return Ok(Vec::new());
    };
    let name = record.function_name();
    let kind = document.kind();
    if kind != Kind::Array {
        let found = match (record.binary, kind) {
            (true, _) => "a non-array",
            (false, Kind::Object) => "an object",
            (false, _) => "a scalar",
        };
        return Err(diagnostic(
            "22023",
            format!("cannot call {name} on {found}"),
        ));
    }
    let elements = document.elements()?;
    let take = elements.len().min(row_budget.unwrap_or(usize::MAX));
    let mut builder = RowBuilder::new(env, shape)?;
    // Admit the retained input of every produced row before any conversion allocates.
    for element in elements.iter().take(take) {
        env.controls.check_interruption()?;
        builder.admit_input(element.retained_bytes()?)?;
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(take).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to retain JSON record rows: {error}"))
    })?;
    // PostgreSQL validates every element's shape before returning rows.
    let mut objects = Vec::with_capacity(take);
    for element in elements.into_iter().take(take) {
        env.controls.check_interruption()?;
        if element.kind() != Kind::Object {
            return Err(diagnostic(
                "22023",
                format!("argument of {name} must be an array of objects"),
            ));
        }
        objects.push(element);
    }
    for (index, element) in objects.into_iter().enumerate() {
        env.controls.check_interruption()?;
        let values = convert_object(element, shape)?;
        rows.push(builder.finish(values, index + 1)?);
    }
    builder.seal(rows, env)
}

fn convert_object(document: Fragment, shape: &Shape) -> Result<Vec<Value>, QueryError> {
    let members = document.members()?;
    let fields = shape
        .declared
        .iter()
        .map(|(name, data_type)| (name.as_str(), data_type))
        .collect::<Vec<_>>();
    let base = base_fields(shape.base.as_ref(), fields.len());
    field_values(members, &fields, base)
}

/// The base row values, or NULL for each declared column.
fn base_values(shape: &Shape) -> Vec<Value> {
    base_fields(shape.base.as_ref(), shape.declared.len()).map_or_else(
        || vec![Value::Null; shape.declared.len()],
        <[Value]>::to_vec,
    )
}

/// Admits row memory incrementally and builds rows with the call's column descriptors.
struct RowBuilder<'a> {
    shape: &'a Shape,
    memory: crate::runtime::QueryMemoryReservation,
    admitted: usize,
    used: usize,
    row_overhead: usize,
}

impl<'a> RowBuilder<'a> {
    fn new(env: &SourceExecutionEnv<'_>, shape: &'a Shape) -> Result<Self, QueryError> {
        env.controls.check_interruption()?;
        let columns = shape.types.len();
        let slots = columns.saturating_mul(size_of::<(String, Value)>() + 32);
        let row_overhead = size_of::<BatchRow>()
            .saturating_add(slots)
            .saturating_add(64);
        let mut memory = env.controls.reserve_query_memory(0)?;
        memory.try_grow(size_of::<Vec<BatchRow>>())?;
        Ok(Self {
            shape,
            memory,
            admitted: 0,
            used: 0,
            row_overhead,
        })
    }

    /// Reserves the estimated retained size of a row built from `input_bytes` of JSON.
    fn admit_input(&mut self, input_bytes: usize) -> Result<(), QueryError> {
        let bytes = self
            .row_overhead
            .saturating_add(input_bytes.saturating_mul(2));
        self.memory.try_grow(bytes)?;
        self.admitted = self.admitted.saturating_add(bytes);
        Ok(())
    }

    fn finish(&mut self, values: Vec<Value>, position: usize) -> Result<BatchRow, QueryError> {
        let mut actual = self.row_overhead;
        for value in &values {
            actual = actual.saturating_add(value_clone_bytes(value)?);
        }
        self.used = self.used.saturating_add(actual);
        if self.used > self.admitted {
            let extra = self.used - self.admitted;
            self.memory.try_grow(extra)?;
            self.admitted = self.used;
        }
        let mut columns = (0..values.len())
            .map(table_sources::physical_name)
            .zip(values)
            .collect::<Vec<_>>();
        if self.shape.ordinal {
            let ordinal = i64::try_from(position).map_err(|_| {
                CassieError::ResourceLimit("JSON record ordinality overflow".into())
            })?;
            columns.push((
                table_sources::physical_name(columns.len()),
                Value::Int64(ordinal),
            ));
        }
        Ok(BatchRow::from_projected_values(columns)
            .with_optional_data_types(Some(Arc::clone(&self.shape.types))))
    }

    /// Attaches the shared reservation to every row once the whole result exists.
    fn seal(
        self,
        mut rows: Vec<BatchRow>,
        env: &SourceExecutionEnv<'_>,
    ) -> Result<Vec<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        let reservation = Arc::new(self.memory);
        for row in &mut rows {
            row.attach_operator_memory(env.controls, Arc::clone(&reservation))?;
        }
        Ok(rows)
    }
}
