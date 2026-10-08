use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{
    add, data_type_clone_bytes, grown_capacity, hash_table_bytes, lookup_bytes, mul,
    serialized_json_bytes, value_clone_bytes,
};
use crate::executor::semantic::{SemanticKey, SemanticValue};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value};

use super::{check_timeout, BatchRow, QueryError};

/// Joined output owns one continuous reservation from candidate construction to finalization.
#[derive(Debug)]
pub(super) struct JoinRows {
    rows: Vec<BatchRow>,
    memory: QueryMemoryReservation,
    controls: QueryExecutionControls,
}

impl JoinRows {
    pub(super) fn try_new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        check_timeout(controls)?;
        Ok(Self {
            rows: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls: controls.clone(),
        })
    }

    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The builder runs only after admission and may reject a candidate without retaining it.
    pub(super) fn try_push_combined(
        &mut self,
        left: &BatchRow,
        right: &BatchRow,
        build: impl FnOnce() -> Result<Option<BatchRow>, QueryError>,
    ) -> Result<bool, QueryError> {
        check_timeout(&self.controls)?;
        let previous_bytes = self.memory.bytes();
        self.memory.try_grow(combined_row_bytes(left, right)?)?;
        let candidate = match build() {
            Ok(candidate) => candidate,
            Err(error) => {
                self.memory.shrink_to(previous_bytes);
                return Err(error);
            }
        };
        if let Err(error) = check_timeout(&self.controls) {
            drop(candidate);
            self.memory.shrink_to(previous_bytes);
            return Err(error);
        }
        let Some(candidate) = candidate else {
            self.memory.shrink_to(previous_bytes);
            return Ok(false);
        };
        if let Err(error) = self.rows.try_reserve_exact(1) {
            drop(candidate);
            self.memory.shrink_to(previous_bytes);
            return Err(CassieError::ResourceLimit(format!(
                "unable to retain joined output: {error}"
            ))
            .into());
        }
        self.rows.push(candidate);
        Ok(true)
    }

    pub(super) fn into_parts(self) -> (Vec<BatchRow>, QueryMemoryReservation) {
        (self.rows, self.memory)
    }
}

/// Exact-capacity combined entries/aliases/types plus eager lookup and one output-row slot.
pub(in crate::executor::execution) fn combined_row_bytes(
    left: &BatchRow,
    right: &BatchRow,
) -> Result<usize, CassieError> {
    let entries = add(left.entries().len(), right.entries().len())?;
    let aliases = add(left.aliases().len(), right.aliases().len())?;
    let mut names = 0;
    let mut bytes = add(
        size_of::<BatchRow>(),
        add(
            mul(entries, size_of::<(String, Value)>())?,
            mul(aliases, size_of::<(String, usize)>())?,
        )?,
    )?;
    for row in [left, right] {
        bytes = add(bytes, entries_heap(row)?)?;
        names = add(names, row_names_bytes(row)?)?;
    }
    bytes = add(bytes, lookup_bytes(add(entries, aliases)?, names)?)?;
    if !left.data_types().is_empty() || !right.data_types().is_empty() {
        bytes = add(bytes, type_arc_bytes(entries)?)?;
        for row in [left, right] {
            for data_type in row.data_types().iter().take(row.entries().len()) {
                bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
            }
        }
    }
    Ok(bytes)
}

/// A copied row's owned buffers, including shared type metadata as a conservative lifetime bound.
pub(super) fn cloned_row_bytes(row: &BatchRow) -> Result<usize, CassieError> {
    let entries = row.entries().len();
    let aliases = row.aliases().len();
    let mut bytes = add(
        size_of::<BatchRow>(),
        add(
            mul(entries, size_of::<(String, Value)>())?,
            mul(aliases, size_of::<(String, usize)>())?,
        )?,
    )?;
    bytes = add(bytes, entries_heap(row)?)?;
    bytes = add(
        bytes,
        lookup_bytes(add(entries, aliases)?, row_names_bytes(row)?)?,
    )?;
    if let Some(types) = row.shared_data_types() {
        bytes = add(bytes, type_arc_bytes(types.capacity())?)?;
        for data_type in types.iter() {
            bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
        }
    }
    Ok(bytes)
}

/// Moving a retained row preserves spare buffers that a fresh clone can discard.
pub(super) fn moved_row_bytes(row: &BatchRow) -> Result<usize, CassieError> {
    add(cloned_row_bytes(row)?, row.retained_buffer_spare_bytes()?)
}

fn entries_heap(row: &BatchRow) -> Result<usize, CassieError> {
    let mut bytes = 0;
    for (name, value) in row.entries() {
        bytes = add(bytes, add(name.len(), value_clone_bytes(value)?)?)?;
    }
    for (name, _) in row.aliases() {
        bytes = add(bytes, name.len())?;
    }
    Ok(bytes)
}

fn row_names_bytes(row: &BatchRow) -> Result<usize, CassieError> {
    row.entries()
        .iter()
        .map(|(name, _)| name.len())
        .chain(row.aliases().iter().map(|(name, _)| name.len()))
        .try_fold(0, add)
}

fn type_arc_bytes(capacity: usize) -> Result<usize, CassieError> {
    add(
        size_of::<Vec<DataType>>() + 2 * size_of::<usize>(),
        mul(capacity, size_of::<DataType>())?,
    )
}

/// Join key selectors resolve exact entry/alias names before choosing these indexed paths.
fn exact_key_value<'a>(row: &'a BatchRow, column: &str) -> Option<&'a Value> {
    row.entries()
        .iter()
        .find(|(name, _)| name == column)
        .map(|(_, value)| value)
        .or_else(|| {
            row.aliases()
                .iter()
                .find(|(name, _)| name == column)
                .and_then(|(_, index)| row.entries().get(*index))
                .map(|(_, value)| value)
        })
}

pub(super) fn join_key_bytes(row: &BatchRow, column: &str) -> Result<usize, CassieError> {
    let Some(value) = exact_key_value(row, column).filter(|value| !value.is_null()) else {
        return Ok(0);
    };
    let heap = match value {
        Value::String(text) => text.len().max(27),
        Value::Vector(vector) => mul(vector.values.len(), size_of::<u32>())?,
        Value::Json(value) => mul(serialized_json_bytes(value)?, 2)?.max(128),
        // Numeric key normalization formats only integral values within the i64 range.
        Value::Float64(_) => 64,
        Value::Null | Value::Bool(_) | Value::Int64(_) => 0,
    };
    add(size_of::<SemanticValue>(), heap)
}

/// Reusable parser/candidate scratch plus lookup tables first materialized by `row.get`.
pub(super) fn key_lookup_scratch_bytes(
    rows: &[BatchRow],
    column: &str,
) -> Result<usize, CassieError> {
    if rows.is_empty() {
        return Ok(0);
    }
    let longest_name = rows
        .iter()
        .flat_map(|row| {
            row.entries()
                .iter()
                .map(|(name, _)| name.len())
                .chain(row.aliases().iter().map(|(name, _)| name.len()))
        })
        .max()
        .unwrap_or(0);
    let mut bytes = add(512, add(mul(column.len(), 32)?, mul(longest_name, 8)?)?)?;
    for row in rows.iter().filter(|row| !row.lookup_initialized()) {
        bytes = add(
            bytes,
            lookup_bytes(
                add(row.entries().len(), row.aliases().len())?,
                row_names_bytes(row)?,
            )?,
        )?;
    }
    Ok(bytes)
}

/// Exact non-NULL key count bounds the table before any key is constructed.
pub(super) fn hash_build_capacity(rows: &[BatchRow], column: &str) -> usize {
    rows.iter()
        .filter(|row| exact_key_value(row, column).is_some_and(|value| !value.is_null()))
        .count()
}

/// Full table and worst-case singleton group capacities are admitted before any build key.
pub(super) fn hash_build_bytes<T>(rows: &[BatchRow], column: &str) -> Result<usize, CassieError> {
    let mut count = 0;
    let mut bytes = key_lookup_scratch_bytes(rows, column)?;
    for row in rows {
        let key_bytes = join_key_bytes(row, column)?;
        if key_bytes != 0 {
            count = add(count, 1)?;
            bytes = add(bytes, key_bytes)?;
        }
    }
    bytes = add(bytes, hash_table_bytes::<(SemanticKey, Vec<T>)>(count)?)?;
    add(bytes, mul(mul(count, 4)?, size_of::<T>())?)
}

/// Includes copied rows/keys, both fallible-collect capacities and stable-sort temporary slots.
pub(super) fn keyed_rows_bytes<T>(
    left: &[BatchRow],
    left_column: &str,
    right: &[BatchRow],
    right_column: &str,
) -> Result<usize, CassieError> {
    let capacity = add(
        grown_capacity(left.len(), 4)?,
        grown_capacity(right.len(), 4)?,
    )?;
    let mut bytes = mul(add(capacity, left.len().max(right.len()))?, size_of::<T>())?;
    bytes = add(
        bytes,
        key_lookup_scratch_bytes(left, left_column)?
            .max(key_lookup_scratch_bytes(right, right_column)?),
    )?;
    for (rows, column) in [(left, left_column), (right, right_column)] {
        for row in rows {
            bytes = add(bytes, cloned_row_bytes(row)? - size_of::<BatchRow>())?;
            bytes = add(bytes, join_key_bytes(row, column)?)?;
        }
    }
    Ok(bytes)
}
