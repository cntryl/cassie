use std::mem::size_of;
use std::sync::Arc;

use serde::ser::SerializeStruct;

use crate::app::{CassieError, QueryExplainOutput, QueryExplainPlan};
use crate::executor::QueryResult;
use crate::runtime::accounted::json;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::{rest_json_conversion_error, RestColumnMeta};

#[derive(Debug, Clone)]
pub struct RestQueryResult {
    payload: Arc<RestQueryPayload>,
}

#[derive(Debug)]
struct RestQueryPayload {
    columns: Vec<RestColumnMeta>,
    rows: Vec<Vec<serde_json::Value>>,
    command: String,
    reservation: QueryMemoryReservation,
}

impl RestQueryResult {
    #[must_use]
    pub fn columns(&self) -> &[RestColumnMeta] {
        &self.payload.columns
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<serde_json::Value>] {
        &self.payload.rows
    }

    #[must_use]
    pub fn command(&self) -> &str {
        &self.payload.command
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows().len()
    }

    /// # Errors
    ///
    /// Returns an interruption, conversion or memory-limit error before publishing output.
    pub fn try_from_result_with_controls(
        result: QueryResult,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let admitted = result.try_clone_for_query(controls)?;
        Self::convert(&admitted, Some(controls))
    }

    #[cfg(test)]
    pub(super) fn row_capacity(&self) -> usize {
        self.payload.rows.capacity()
    }

    pub(crate) fn admission(&self) -> &QueryMemoryReservation {
        &self.payload.reservation
    }

    fn convert(
        result: &QueryResult,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, CassieError> {
        check(controls)?;
        let reservation = result.admission().try_sibling(conversion_bytes(result, controls)?)?;
        let mut columns = fresh_vec(result.columns().len())?;
        for column in result.columns() {
            check(controls)?;
            columns.push(RestColumnMeta {
                name: copy_text(&column.name, controls)?,
                data_type: copy_text(&column.data_type, controls)?,
                type_oid: column.type_oid,
                typlen: column.typlen,
                atttypmod: column.atttypmod,
                format_code: column.format_code,
                nullable: column.nullable,
            });
        }
        let mut rows = fresh_vec(result.row_count())?;
        for row in result.rows() {
            check(controls)?;
            let mut converted = fresh_vec(row.len())?;
            for value in row {
                check(controls)?;
                converted.push(convert_value(value, controls)?);
            }
            rows.push(converted);
        }
        let command = copy_text(result.command(), controls)?;
        check(controls)?;
        Ok(Self {
            payload: Arc::new(RestQueryPayload {
                columns,
                rows,
                command,
                reservation,
            }),
        })
    }
}

impl TryFrom<QueryResult> for RestQueryResult {
    type Error = CassieError;

    fn try_from(result: QueryResult) -> Result<Self, Self::Error> {
        Self::convert(&result, None)
    }
}

impl serde::Serialize for RestQueryResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut output = serializer.serialize_struct("RestQueryResult", 3)?;
        output.serialize_field("columns", self.columns())?;
        output.serialize_field("rows", self.rows())?;
        output.serialize_field("command", self.command())?;
        output.end()
    }
}

#[derive(Debug, Clone)]
pub struct RestQueryExplainResponse {
    result: RestQueryResult,
    plan: QueryExplainPlan,
}

impl RestQueryExplainResponse {
    #[must_use]
    pub fn columns(&self) -> &[RestColumnMeta] {
        self.result.columns()
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<serde_json::Value>] {
        self.result.rows()
    }

    #[must_use]
    pub fn command(&self) -> &str {
        self.result.command()
    }

    #[must_use]
    pub fn plan(&self) -> &QueryExplainPlan {
        &self.plan
    }

    /// # Errors
    ///
    /// Returns an interruption, conversion or memory-limit error before publishing output.
    pub fn try_from_output_with_controls(
        output: QueryExplainOutput,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let (result, plan) = output.into_parts();
        let result = RestQueryResult::try_from_result_with_controls(result, controls)?;
        controls.check_interruption()?;
        Ok(Self { result, plan })
    }

    pub(crate) fn admission(&self) -> &QueryMemoryReservation {
        self.result.admission()
    }
}

impl TryFrom<QueryExplainOutput> for RestQueryExplainResponse {
    type Error = CassieError;

    fn try_from(output: QueryExplainOutput) -> Result<Self, Self::Error> {
        let (result, plan) = output.into_parts();
        Ok(Self {
            result: RestQueryResult::try_from(result)?,
            plan,
        })
    }
}

impl serde::Serialize for RestQueryExplainResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut output = serializer.serialize_struct("RestQueryExplainResponse", 4)?;
        output.serialize_field("columns", self.columns())?;
        output.serialize_field("rows", self.rows())?;
        output.serialize_field("command", self.command())?;
        output.serialize_field("plan", self.plan())?;
        output.end()
    }
}

fn conversion_bytes(
    result: &QueryResult,
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, CassieError> {
    let mut bytes = add(size_of::<RestQueryPayload>(), 2 * size_of::<usize>())?;
    bytes = add(bytes, mul(result.columns().len(), size_of::<RestColumnMeta>())?)?;
    bytes = add(bytes, result.command().len())?;
    for column in result.columns() {
        check(controls)?;
        bytes = add(bytes, add(column.name.len(), column.data_type.len())?)?;
    }
    bytes = add(bytes, mul(result.row_count(), size_of::<Vec<serde_json::Value>>())?)?;
    for row in result.rows() {
        check(controls)?;
        bytes = add(bytes, mul(row.len(), size_of::<serde_json::Value>())?)?;
        for value in row {
            check(controls)?;
            let heap = match value {
                Value::String(text) => text.len(),
                Value::Vector(vector) => mul(vector.values.len(), size_of::<serde_json::Value>())?,
                Value::Json(value) => json::retained_bytes(value)? - size_of::<serde_json::Value>(),
                Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
            };
            bytes = add(bytes, heap)?;
        }
    }
    Ok(bytes)
}

fn convert_value(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Int64(value) => Ok(serde_json::Value::Number((*value).into())),
        Value::Float64(value) => json_number(*value),
        Value::String(value) => Ok(serde_json::Value::String(copy_text(value, controls)?)),
        Value::Json(value) => Ok(value.clone()),
        Value::Vector(value) => {
            let mut values = fresh_vec(value.values.len())?;
            for component in &value.values {
                check(controls)?;
                values.push(json_number(f64::from(*component))?);
            }
            Ok(serde_json::Value::Array(values))
        }
    }
}

fn json_number(value: f64) -> Result<serde_json::Value, CassieError> {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| rest_json_conversion_error(crate::types::json::NON_FINITE_JSON_NUMBER))
}

fn fresh_vec<T>(length: usize) -> Result<Vec<T>, CassieError> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(allocation_error)?;
    if values.capacity() > length && size_of::<T>() != 0 {
        return Err(capacity_error());
    }
    Ok(values)
}

fn copy_text(
    text: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, CassieError> {
    check(controls)?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).map_err(allocation_error)?;
    if copy.capacity() > text.len() {
        return Err(capacity_error());
    }
    let mut start = 0;
    while start < text.len() {
        check(controls)?;
        let mut end = start.saturating_add(4096).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        copy.push_str(&text[start..end]);
        start = end;
    }
    Ok(copy)
}

fn allocation_error(error: std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("unable to allocate REST result: {error}"))
}

fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)
}

fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    crate::executor::retained_memory::add(left, right)
}

fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    crate::executor::retained_memory::mul(left, right)
}

fn capacity_error() -> CassieError {
    CassieError::ResourceLimit("allocator exceeded admitted REST result capacity".to_owned())
}
