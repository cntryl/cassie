//! Immutable query results whose physical and requesting-query leases follow their owners.

use std::mem::size_of;
use std::sync::Arc;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::runtime::owned_memory::{allocation_error, grow_vec, try_reserve_vec};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};

use super::types::ColumnMeta;
#[path = "result_rendering.rs"]
mod rendering;
#[doc(hidden)]
pub use rendering::bound as native_result_text_bound;

#[derive(Debug, serde::Serialize)]
struct QueryResultPayload {
    columns: Vec<ColumnMeta>,
    rows: Vec<Vec<Value>>,
    command: String,
    /// Binary metadata for results whose public rows contain rendered text.
    #[serde(skip)]
    binary_metadata: Vec<BinaryMetadata>,
    /// Columns whose direct text protocol output uses PostgreSQL array text.
    #[serde(skip)]
    postgres_array_text_columns: Vec<usize>,
    #[serde(skip)]
    temporal: Option<Arc<crate::types::extended::temporal::TemporalSession>>,
    #[serde(skip)]
    memory: QueryMemoryReservation,
}

#[derive(Debug)]
enum BinaryMetadata {
    ObjectOids {
        column: usize,
        oids: Vec<Option<u32>>,
    },
    // Ordered by row and column. When present this is the first entry.
    RealNaNs(Vec<RealNan>),
}

#[derive(Clone, Copy, Debug)]
struct RealNan {
    row: usize,
    column: usize,
    bits: u32,
}

fn real_nan_bits(value: &Value) -> Option<u32> {
    let Value::Typed(TypedValue::Real(real)) = value else {
        return None;
    };
    real.is_nan().then(|| real.to_bits())
}

struct PendingRow {
    values: Vec<Value>,
    slots: QueryMemoryReservation,
    heap: QueryMemoryReservation,
}

/// An immutable result. Clones share admitted storage and its lifetime.
#[derive(Debug, Clone)]
pub struct QueryResult {
    payload: Arc<QueryResultPayload>,
    exposure: Option<Arc<QueryMemoryReservation>>,
}

impl serde::Serialize for QueryResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.payload.serialize(serializer)
    }
}

impl QueryResult {
    #[must_use]
    pub fn columns(&self) -> &[ColumnMeta] {
        &self.payload.columns
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.payload.rows
    }

    /// The OID a rendered `reg*` value in `row`/`column` names, for binary output.
    #[doc(hidden)]
    #[must_use]
    pub fn object_oid(&self, row: usize, column: usize) -> Option<u32> {
        self.payload
            .binary_metadata
            .iter()
            .find_map(|entry| match entry {
                BinaryMetadata::ObjectOids {
                    column: index,
                    oids,
                } if *index == column => Some(oids),
                _ => None,
            })
            .and_then(|oids| oids.get(row).copied().flatten())
    }

    /// The original binary payload for a REAL result rendered as NaN.
    #[doc(hidden)]
    #[must_use]
    pub fn real_nan_bits(&self, row: usize, column: usize) -> Option<u32> {
        let BinaryMetadata::RealNaNs(bits) = self.payload.binary_metadata.first()? else {
            return None;
        };
        bits.binary_search_by_key(&(row, column), |value| (value.row, value.column))
            .ok()
            .map(|index| bits[index].bits)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn postgres_array_text_columns(&self) -> &[usize] {
        &self.payload.postgres_array_text_columns
    }
    #[doc(hidden)]
    #[must_use]
    pub fn temporal_context(&self) -> &crate::types::extended::temporal::TemporalSession {
        self.payload
            .temporal
            .as_deref()
            .unwrap_or_else(|| crate::types::extended::temporal::default_session())
    }

    #[must_use]
    pub fn command(&self) -> &str {
        &self.payload.command
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.payload.rows.len()
    }

    /// # Errors
    /// Returns cancellation, deadline or resource errors before allocating retained storage.
    pub fn try_builder(
        controls: &QueryExecutionControls,
    ) -> Result<QueryResultBuilder<'_>, CassieError> {
        controls.check_interruption()?;
        let memory = controls.reserve_query_memory(add(
            size_of::<QueryResultPayload>(),
            2 * size_of::<usize>(),
        )?)?;
        Ok(QueryResultBuilder {
            columns: Vec::new(),
            rows: Vec::new(),
            command: String::new(),
            binary_metadata: Vec::new(),
            postgres_array_text_columns: Vec::new(),
            memory,
            controls,
        })
    }

    /// Admits exposure against a distinct requesting query before publishing a shared view.
    ///
    /// # Errors
    /// Returns cancellation, deadline or resource errors without affecting the source result.
    pub fn try_clone_for_query(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        if self.admission().shares_tracker(controls) {
            return Ok(self.clone());
        }
        if self.payload.memory.shares_tracker(controls) {
            return Ok(Self {
                payload: Arc::clone(&self.payload),
                exposure: None,
            });
        }
        let exposure = controls
            .reserve_query_memory(add(self.retained_bytes(), Self::exposure_header_bytes())?)?;
        controls.check_interruption()?;
        Ok(Self {
            payload: Arc::clone(&self.payload),
            exposure: Some(Arc::new(exposure)),
        })
    }

    /// # Errors
    /// Returns an admission or interruption error while preserving the original result.
    pub fn try_with_columns(
        &self,
        controls: &QueryExecutionControls,
        columns: &[ColumnMeta],
    ) -> Result<Self, CassieError> {
        if columns == self.columns() {
            return self.try_clone_for_query(controls);
        }
        Self::try_rebuild(
            controls,
            columns,
            self.rows(),
            self.command(),
            &self.payload.binary_metadata,
            &self.payload.postgres_array_text_columns,
        )
    }

    /// # Errors
    /// Returns an admission or interruption error while preserving the original result.
    pub fn try_with_rows(
        &self,
        controls: &QueryExecutionControls,
        rows: &[Vec<Value>],
    ) -> Result<Self, CassieError> {
        Self::try_rebuild(
            controls,
            self.columns(),
            rows,
            self.command(),
            &[],
            &self.payload.postgres_array_text_columns,
        )
    }

    /// # Errors
    /// Returns an admission or interruption error while preserving the original result.
    pub fn try_with_command(
        &self,
        controls: &QueryExecutionControls,
        command: &str,
    ) -> Result<Self, CassieError> {
        if command == self.command() {
            return self.try_clone_for_query(controls);
        }
        Self::try_rebuild(
            controls,
            self.columns(),
            self.rows(),
            command,
            &self.payload.binary_metadata,
            &self.payload.postgres_array_text_columns,
        )
    }

    /// Transfers rows without detaching their physical or requesting-query owners.
    #[must_use]
    pub fn into_owned_rows(self) -> OwnedQueryRows {
        OwnedQueryRows { result: self }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn admission(&self) -> &QueryMemoryReservation {
        self.exposure.as_deref().unwrap_or(&self.payload.memory)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.payload.memory.bytes()
    }

    pub(crate) fn shares_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.payload, &other.payload)
    }

    pub(crate) fn exposure_storage_bytes(&self) -> usize {
        if self.exposure.is_some() {
            Self::exposure_header_bytes()
        } else {
            0
        }
    }

    pub(crate) fn shares_exposure(&self, other: &Self) -> bool {
        match (&self.exposure, &other.exposure) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    pub(crate) const fn exposure_header_bytes() -> usize {
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>()
    }

    /// Rebuilds the result, retaining binary metadata when rows are unchanged.
    fn try_rebuild(
        controls: &QueryExecutionControls,
        columns: &[ColumnMeta],
        rows: &[Vec<Value>],
        command: &str,
        binary_metadata: &[BinaryMetadata],
        postgres_array_text_columns: &[usize],
    ) -> Result<Self, CassieError> {
        let mut builder = Self::try_builder(controls)?;
        for column in columns {
            builder.try_push_column(column)?;
        }
        for row in rows {
            builder.try_push_row(row)?;
        }
        for entry in binary_metadata {
            match entry {
                BinaryMetadata::ObjectOids { column, oids } => {
                    let (mut retained, memory) = try_reserve_vec(oids.len(), &builder.memory)?;
                    retained.extend_from_slice(oids);
                    builder.push_object_oids(*column, retained, memory)?;
                }
                BinaryMetadata::RealNaNs(bits) => {
                    builder.try_extend_real_nan_bits(bits.len(), bits.iter().copied())?;
                }
            }
        }
        builder.try_set_postgres_array_text_columns(postgres_array_text_columns)?;
        builder.try_set_command(command)?;
        builder.finish()
    }
}

/// A fallible construction scope borrowing live controls; finished results retain only memory.
#[derive(Debug)]
pub struct QueryResultBuilder<'a> {
    columns: Vec<ColumnMeta>,
    rows: Vec<Vec<Value>>,
    command: String,
    binary_metadata: Vec<BinaryMetadata>,
    postgres_array_text_columns: Vec<usize>,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
}

impl QueryResultBuilder<'_> {
    /// # Errors
    /// Returns an interruption or resource error before cloning a new column.
    pub fn try_push_column(&mut self, column: &ColumnMeta) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        let memory = self
            .memory
            .try_sibling(add(column.name.capacity(), column.data_type.capacity())?)?;
        grow_vec(&mut self.columns, 1, &mut self.memory)?;
        self.columns.push(column.clone());
        self.memory.absorb(memory);
        Ok(())
    }

    /// # Errors
    /// Returns an interruption or resource error before cloning a new row.
    pub fn try_push_row(&mut self, row: &[Value]) -> Result<(), CassieError> {
        self.try_push_values(row.iter())
    }

    #[doc(hidden)]
    pub fn try_push_values<'v, I>(&mut self, values: I) -> Result<(), CassieError>
    where
        I: Iterator<Item = &'v Value> + Clone,
    {
        self.controls.check_interruption()?;
        let temporal = crate::executor::filter::temporal::session_of(Some(self.controls));
        let _rendering_memory = rendering::admit(values.clone(), self.controls, &temporal)?;
        let (count, variable_bytes, real_nans) =
            values
                .clone()
                .try_fold((0, 0, 0), |(count, bytes, real_nans), value| {
                    self.controls.check_interruption()?;
                    let value_bytes = match value {
                        Value::Typed(typed) => rendering::format(typed, &temporal).capacity(),
                        other => value_clone_bytes(other)?,
                    };
                    Ok::<_, CassieError>((
                        add(count, 1)?,
                        add(bytes, value_bytes)?,
                        add(real_nans, usize::from(real_nan_bits(value).is_some()))?,
                    ))
                })?;
        let variable_memory = self.memory.try_sibling(variable_bytes)?;
        grow_vec(&mut self.rows, 1, &mut self.memory)?;
        let (row, row_memory) = try_reserve_vec(count, &self.memory)?;
        let mut pending = PendingRow {
            values: row,
            slots: row_memory,
            heap: variable_memory,
        };
        for value in values.clone() {
            self.controls.check_interruption()?;
            // Natively represented types publish their PostgreSQL text output.
            pending.values.push(match value {
                Value::Typed(typed) => Value::String(rendering::format(typed, &temporal)),
                other => other.clone(),
            });
        }
        let actual_heap = pending
            .values
            .iter()
            .try_fold(0, |bytes, value| add(bytes, value_clone_bytes(value)?))?;
        if actual_heap > pending.heap.bytes() {
            pending.heap.try_grow(actual_heap - pending.heap.bytes())?;
        }
        let row = self.rows.len();
        self.try_extend_real_nan_bits(
            real_nans,
            values.enumerate().filter_map(|(column, value)| {
                real_nan_bits(value).map(|bits| RealNan { row, column, bits })
            }),
        )?;
        self.rows.push(pending.values);
        self.memory.absorb(pending.slots);
        self.memory.absorb(pending.heap);
        Ok(())
    }

    /// # Errors
    /// Returns an interruption or resource error without replacing the previous command.
    pub fn try_set_command(&mut self, command: &str) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if command.len() <= self.command.capacity() {
            self.command.clear();
            self.command.push_str(command);
            return Ok(());
        }
        let memory = self.memory.try_sibling(command.len())?;
        let mut replacement = String::new();
        replacement
            .try_reserve_exact(command.len())
            .map_err(|error| allocation_error(&error))?;
        if replacement.capacity() > command.len() {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted command capacity".to_owned(),
            ));
        }
        let old_bytes = self.command.capacity();
        replacement.push_str(command);
        let old = std::mem::replace(&mut self.command, replacement);
        drop(old);
        self.memory.shrink_to(self.memory.bytes() - old_bytes);
        self.memory.absorb(memory);
        Ok(())
    }

    /// Records the OIDs behind a rendered `reg*` column, one per row.
    ///
    /// # Errors
    /// Returns a resource error when the identities cannot be admitted.
    pub(crate) fn try_set_object_oids(
        &mut self,
        column: usize,
        oids: Vec<Option<u32>>,
    ) -> Result<(), CassieError> {
        let bytes = mul(oids.capacity(), size_of::<Option<u32>>())?;
        let memory = self.memory.try_sibling(bytes)?;
        self.push_object_oids(column, oids, memory)
    }

    fn push_object_oids(
        &mut self,
        column: usize,
        oids: Vec<Option<u32>>,
        memory: QueryMemoryReservation,
    ) -> Result<(), CassieError> {
        self.reserve_binary_metadata_slot()?;
        self.binary_metadata
            .push(BinaryMetadata::ObjectOids { column, oids });
        self.memory.absorb(memory);
        Ok(())
    }

    fn reserve_binary_metadata_slot(&mut self) -> Result<(), CassieError> {
        if self.binary_metadata.capacity() == 0 {
            let (metadata, memory) = try_reserve_vec(1, &self.memory)?;
            self.binary_metadata = metadata;
            self.memory.absorb(memory);
            return Ok(());
        }
        grow_vec(&mut self.binary_metadata, 1, &mut self.memory)
    }

    fn try_extend_real_nan_bits(
        &mut self,
        count: usize,
        bits: impl Iterator<Item = RealNan>,
    ) -> Result<(), CassieError> {
        if count == 0 {
            return Ok(());
        }
        if let Some(BinaryMetadata::RealNaNs(retained)) = self.binary_metadata.first_mut() {
            grow_vec(retained, count, &mut self.memory)?;
            retained.extend(bits);
        } else {
            let (mut retained, memory) = try_reserve_vec(count, &self.memory)?;
            self.reserve_binary_metadata_slot()?;
            retained.extend(bits);
            self.binary_metadata
                .insert(0, BinaryMetadata::RealNaNs(retained));
            self.memory.absorb(memory);
        }
        Ok(())
    }

    /// Records columns whose evaluator carrier needs PostgreSQL array text output.
    ///
    /// # Errors
    /// Returns a resource error when the column directory cannot be admitted.
    pub(crate) fn try_set_postgres_array_text_columns(
        &mut self,
        columns: &[usize],
    ) -> Result<(), CassieError> {
        if columns.is_empty() {
            return Ok(());
        }
        let (mut retained, memory) = try_reserve_vec(columns.len(), &self.memory)?;
        retained.extend_from_slice(columns);
        self.postgres_array_text_columns = retained;
        self.memory.absorb(memory);
        Ok(())
    }

    /// # Errors
    /// Returns cancellation or a deadline error without publishing a result.
    pub fn finish(mut self) -> Result<QueryResult, CassieError> {
        self.controls.check_interruption()?;
        let temporal = if self.columns.iter().any(|column| {
            u32::try_from(column.type_oid)
                .ok()
                .and_then(crate::catalog::builtin_identity::builtin_array_element_oid)
                .is_some()
        }) {
            let temporal = self.controls.temporal_session();
            if let Some(snapshot) = &temporal {
                self.memory.try_grow(rendering::context_bytes(snapshot)?)?;
            }
            temporal
        } else {
            None
        };
        Ok(QueryResult {
            payload: Arc::new(QueryResultPayload {
                columns: self.columns,
                rows: self.rows,
                command: self.command,
                binary_metadata: self.binary_metadata,
                postgres_array_text_columns: self.postgres_array_text_columns,
                temporal,
                memory: self.memory,
            }),
            exposure: None,
        })
    }
}

/// An owner-preserving row transfer. Borrowed rows remain admitted through its last clone.
#[derive(Debug, Clone)]
pub struct OwnedQueryRows {
    result: QueryResult,
}

impl OwnedQueryRows {
    #[must_use]
    pub fn as_slice(&self) -> &[Vec<Value>] {
        self.result.rows()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.result.row_count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.result.rows().is_empty()
    }
}
