use std::fmt::{self, Write};
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(crate) struct PlanAllocation<'a> {
    controls: &'a QueryExecutionControls,
    memory: QueryMemoryReservation,
}

impl<'a> PlanAllocation<'a> {
    pub(super) fn new(
        controls: &'a QueryExecutionControls,
        owner_bytes: usize,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        Ok(Self {
            controls,
            memory: controls.reserve_query_memory(owner_bytes)?,
        })
    }

    pub(crate) fn text(&mut self, text: &str) -> Result<String, CassieError> {
        self.format(format_args!("{text}"))
    }

    pub(crate) fn format(&mut self, arguments: fmt::Arguments<'_>) -> Result<String, CassieError> {
        let mut counter = CountingFormatter {
            controls: self.controls,
            bytes: 0,
            error: None,
        };
        if fmt::write(&mut counter, arguments).is_err() {
            return Err(counter.error.unwrap_or_else(format_error));
        }
        let before = self.memory.bytes();
        self.memory.try_grow(counter.bytes)?;
        let mut value = String::new();
        if let Err(error) = value.try_reserve_exact(counter.bytes) {
            self.memory.shrink_to(before);
            return Err(allocation_error(error));
        }
        if value.capacity() > counter.bytes {
            drop(value);
            self.memory.shrink_to(before);
            return Err(capacity_error());
        }
        let mut output = CheckedFormatter {
            controls: self.controls,
            value: &mut value,
            error: None,
        };
        if fmt::write(&mut output, arguments).is_err() {
            let error = output.error.take().unwrap_or_else(format_error);
            drop(value);
            self.memory.shrink_to(before);
            return Err(error);
        }
        Ok(value)
    }

    pub(crate) fn vector<T>(&mut self, length: usize) -> Result<Vec<T>, CassieError> {
        self.controls.check_interruption()?;
        let before = self.memory.bytes();
        self.memory.try_grow(mul(length, size_of::<T>())?)?;
        let mut values = Vec::new();
        if let Err(error) = values.try_reserve_exact(length) {
            self.memory.shrink_to(before);
            return Err(allocation_error(error));
        }
        if values.capacity() > length && size_of::<T>() != 0 {
            drop(values);
            self.memory.shrink_to(before);
            return Err(capacity_error());
        }
        Ok(values)
    }

    pub(super) fn strings(&mut self, values: &[String]) -> Result<Vec<String>, CassieError> {
        let mut output = self.vector(values.len())?;
        for value in values {
            output.push(self.text(value)?);
        }
        Ok(output)
    }

    pub(crate) fn join<'b>(
        &mut self,
        values: impl IntoIterator<Item = &'b str> + Clone,
        separator: &str,
    ) -> Result<String, CassieError> {
        let mut length = 0;
        let mut count = 0;
        for value in values.clone() {
            self.controls.check_interruption()?;
            length = add(length, value.len())?;
            count += 1;
        }
        if count > 1 {
            length = add(length, mul(count - 1, separator.len())?)?;
        }
        let before = self.memory.bytes();
        self.memory.try_grow(length)?;
        let mut output = String::new();
        if let Err(error) = output.try_reserve_exact(length) {
            self.memory.shrink_to(before);
            return Err(allocation_error(error));
        }
        if output.capacity() > length {
            drop(output);
            self.memory.shrink_to(before);
            return Err(capacity_error());
        }
        for (index, value) in values.into_iter().enumerate() {
            self.controls.check_interruption()?;
            // Separators and fields share the same admitted-capacity guard.
            let mut writer = CheckedFormatter {
                controls: self.controls,
                value: &mut output,
                error: None,
            };
            let separator_result = if index > 0 {
                writer.write_str(separator)
            } else {
                Ok(())
            };
            if separator_result
                .and_then(|()| writer.write_str(value))
                .is_err()
            {
                let error = writer.error.take().unwrap_or_else(format_error);
                drop(output);
                self.memory.shrink_to(before);
                return Err(error);
            }
        }
        Ok(output)
    }

    pub(super) fn check(&self) -> Result<(), CassieError> {
        self.controls.check_interruption()
    }

    pub(super) fn controls(&self) -> &'a QueryExecutionControls {
        self.controls
    }

    pub(super) fn push<T>(&mut self, values: &mut Vec<T>, value: T) -> Result<(), CassieError> {
        self.check()?;
        crate::runtime::owned_memory::grow_vec(values, 1, &mut self.memory)?;
        values.push(value);
        Ok(())
    }

    pub(super) fn field_key(&mut self, raw: &str) -> Result<String, CassieError> {
        self.check()?;
        // The existing identifier helper admits at most five components before rejecting.
        // Bound its geometric text buffers, component directories and formatting copies.
        // Its existing infallible allocator internals are not replaced by this output owner.
        let scratch = self
            .controls
            .reserve_query_memory(add(mul(raw.len(), 32)?, 1024)?)?;
        let canonical = crate::sql::ColumnIdentifierPath::reference_field_key(raw);
        let output = self.text(&canonical)?;
        drop(canonical);
        drop(scratch);
        Ok(output)
    }

    pub(super) fn finish_retained(&mut self, retained_bytes: usize) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if retained_bytes > self.memory.bytes() {
            return Err(CassieError::ResourceLimit(
                "EXPLAIN capacity exceeds its construction admission".to_owned(),
            ));
        }
        self.memory.shrink_to(retained_bytes);
        Ok(())
    }

    pub(super) fn into_memory(self) -> QueryMemoryReservation {
        self.memory
    }
}

struct CountingFormatter<'a> {
    controls: &'a QueryExecutionControls,
    bytes: usize,
    error: Option<CassieError>,
}

impl Write for CountingFormatter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        match self
            .controls
            .check_interruption()
            .and_then(|()| add(self.bytes, value.len()))
        {
            Ok(bytes) => {
                self.bytes = bytes;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error);
                Err(fmt::Error)
            }
        }
    }
}

struct CheckedFormatter<'a> {
    controls: &'a QueryExecutionControls,
    value: &'a mut String,
    error: Option<CassieError>,
}

impl Write for CheckedFormatter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let admitted = self.controls.check_interruption().and_then(|()| {
            let needed = add(self.value.len(), value.len())?;
            if needed <= self.value.capacity() {
                Ok(())
            } else {
                Err(CassieError::ResourceLimit(
                    "EXPLAIN formatting exceeded its admitted capacity".to_owned(),
                ))
            }
        });
        if let Err(error) = admitted {
            self.error = Some(error);
            return Err(fmt::Error);
        }
        let mut start = 0;
        while start < value.len() {
            if let Err(error) = self.controls.check_interruption() {
                self.error = Some(error);
                return Err(fmt::Error);
            }
            let mut end = start.saturating_add(4096).min(value.len());
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            self.value.push_str(&value[start..end]);
            start = end;
        }
        Ok(())
    }
}

fn allocation_error(error: std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("unable to allocate EXPLAIN output: {error}"))
}

fn format_error() -> CassieError {
    CassieError::Execution("unable to format EXPLAIN output".to_owned())
}

fn capacity_error() -> CassieError {
    CassieError::ResourceLimit("allocator exceeded admitted EXPLAIN capacity".to_owned())
}

#[cfg(test)]
#[path = "allocation_tests.rs"]
mod tests;
