//! Retained plain CTE rows and independently admitted copies.
use std::mem::size_of;
use std::ops::{Deref, DerefMut};

use super::*;
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul, value_clone_bytes};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryMemoryReservation;
use crate::types::FieldSchema;

pub(super) fn allocation(error: &std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain CTE backing: {error}")).into()
}

pub(super) fn row_bytes(row: &Vec<(String, Value)>) -> Result<usize, QueryError> {
    row.iter().try_fold(
        mul(row.capacity(), size_of::<(String, Value)>())?,
        |bytes, (name, value)| {
            Ok(add(
                bytes,
                add(name.capacity(), value_clone_bytes(value)?)?,
            )?)
        },
    )
}

pub(super) fn rows_bytes(rows: &CteRows) -> Result<usize, QueryError> {
    rows.iter().try_fold(
        mul(rows.capacity(), size_of::<Vec<(String, Value)>>())?,
        |bytes, row| Ok(add(bytes, row_bytes(row)?)?),
    )
}

pub(super) fn fields_bytes(fields: &Vec<FieldSchema>) -> Result<usize, QueryError> {
    fields.iter().try_fold(
        mul(fields.capacity(), size_of::<FieldSchema>())?,
        |bytes, field| {
            Ok(add(
                bytes,
                add(
                    field.name.capacity(),
                    data_type_clone_bytes(&field.data_type)?,
                )?,
            )?)
        },
    )
}

pub(super) struct RetainedRows {
    pub(super) rows: CteRows,
    pub(super) memory: QueryMemoryReservation,
}

impl Deref for RetainedRows {
    type Target = CteRows;
    fn deref(&self) -> &CteRows {
        &self.rows
    }
}
impl DerefMut for RetainedRows {
    fn deref_mut(&mut self) -> &mut CteRows {
        &mut self.rows
    }
}

impl RetainedRows {
    pub(super) fn from_output(
        rows: Vec<BatchRow>,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        check_timeout(controls)?;
        let _input_slots =
            controls.reserve_query_memory(mul(rows.capacity(), size_of::<BatchRow>())?)?;
        let mut retained = AccountedVec::try_new(controls)?;
        for row in rows {
            check_timeout(controls)?;
            let bytes = row.plain_entries_bytes()?;
            retained.try_push_with(bytes, || row.into_entries())?;
        }
        check_timeout(controls)?;
        let (rows, memory) = retained.into_parts();
        Ok(Self { rows, memory })
    }

    pub(super) fn copy(
        rows: &CteRows,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        check_timeout(controls)?;
        let memory = controls.reserve_query_memory(rows_bytes(rows)?)?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(rows.len())
            .map_err(|error| allocation(&error))?;
        for row in rows {
            check_timeout(controls)?;
            copied.push(row.clone());
        }
        check_timeout(controls)?;
        Ok(Self {
            rows: copied,
            memory,
        })
    }

    pub(super) fn append(
        &mut self,
        new: &CteRows,
        controls: &QueryExecutionControls,
    ) -> Result<(), QueryError> {
        check_timeout(controls)?;
        // New vector backing may overlap the entire old vector until reallocation completes.
        let next = add(self.rows.len(), new.len())?;
        let extra = new.iter().try_fold(
            mul(next, size_of::<Vec<(String, Value)>>())?,
            |bytes, row| Ok::<_, QueryError>(add(bytes, row_bytes(row)?)?),
        )?;
        self.memory.try_grow(extra)?;
        self.rows
            .try_reserve_exact(new.len())
            .map_err(|error| allocation(&error))?;
        for row in new {
            check_timeout(controls)?;
            self.rows.push(row.clone());
        }
        self.memory.shrink_to(rows_bytes(&self.rows)?);
        Ok(())
    }

    pub(super) fn rename(
        mut self,
        aliases: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        if aliases.is_empty() || aliases.iter().any(|alias| alias == "*") {
            return Ok(self);
        }
        let extra = self.rows.iter().try_fold(0, |bytes, row| {
            aliases
                .iter()
                .take(row.len())
                .try_fold(bytes, |bytes, alias| add(bytes, alias.len()))
        })?;
        self.memory.try_grow(extra)?;
        for row in &mut self.rows {
            check_timeout(controls)?;
            for ((name, _), alias) in row.iter_mut().zip(aliases) {
                *name = alias.clone();
            }
        }
        self.memory.shrink_to(rows_bytes(&self.rows)?);
        Ok(self)
    }
}

pub(super) fn serialization_scratch(rows: &CteRows) -> Result<usize, QueryError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("CTE serializer byte overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    rows.iter().try_fold(0, |maximum, row| {
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, row).map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to estimate CTE serialization: {error}"
            ))
        })?;
        Ok(maximum.max(mul(counter.0, 2)?.max(128)))
    })
}
