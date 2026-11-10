//! Owned NUMERIC input diagnostics for controlled validation.
use std::fmt;
use std::mem::size_of;

use super::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

/// A NUMERIC input error that retains the query-memory lease for its message.
///
/// Constructed internally by controlled literal validation. The public type is
/// exposed because it is carried by the corresponding `CassieError` variant.
#[derive(Debug)]
pub struct OwnedNumericLiteralError {
    message: String,
    sqlstate: &'static str,
    controls: QueryExecutionControls,
    _memory: QueryMemoryReservation,
}

impl OwnedNumericLiteralError {
    pub(crate) fn syntax(
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Box<Self>, CassieError> {
        Self::syntax_observing(input, controls, || {})
    }

    pub(super) fn syntax_observing(
        input: &str,
        controls: &QueryExecutionControls,
        mut progress: impl FnMut(),
    ) -> Result<Box<Self>, CassieError> {
        const PREFIX: &str = "invalid input syntax for type numeric: \"";
        let capacity = PREFIX
            .len()
            .checked_add(input.len())
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| {
                CassieError::ResourceLimit("numeric diagnostic capacity overflow".into())
            })?;
        Self::create("22P02", capacity, controls, |message| {
            message.push_str(PREFIX);
            append_checked(message, input, controls, &mut progress)?;
            message.push('"');
            Ok(())
        })
    }

    pub(crate) fn range(controls: &QueryExecutionControls) -> Result<Box<Self>, CassieError> {
        const MESSAGE: &str = "value overflows numeric format";
        Self::create("22003", MESSAGE.len(), controls, |message| {
            message.push_str(MESSAGE);
            Ok(())
        })
    }

    fn create(
        sqlstate: &'static str,
        capacity: usize,
        controls: &QueryExecutionControls,
        fill: impl FnOnce(&mut String) -> Result<(), CassieError>,
    ) -> Result<Box<Self>, CassieError> {
        controls.check_interruption()?;
        let bytes = capacity.checked_add(size_of::<Self>()).ok_or_else(|| {
            CassieError::ResourceLimit("numeric diagnostic capacity overflow".into())
        })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let mut message = String::new();
        message.try_reserve_exact(capacity).map_err(|error| {
            CassieError::ResourceLimit(format!("unable to allocate numeric diagnostic: {error}"))
        })?;
        if message.capacity() > capacity {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted numeric diagnostic capacity".into(),
            ));
        }
        controls.check_interruption()?;
        fill(&mut message)?;
        controls.check_interruption()?;
        Ok(Box::new(Self {
            message,
            sqlstate,
            controls: controls.clone(),
            _memory: memory,
        }))
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
    pub(crate) fn sqlstate(&self) -> &'static str {
        self.sqlstate
    }
    pub(crate) fn controls(&self) -> &QueryExecutionControls {
        &self.controls
    }
}

impl fmt::Display for OwnedNumericLiteralError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OwnedNumericLiteralError {}

fn append_checked(
    output: &mut String,
    input: &str,
    controls: &QueryExecutionControls,
    progress: &mut impl FnMut(),
) -> Result<(), CassieError> {
    let mut offset = 0;
    while offset < input.len() {
        controls.check_interruption()?;
        let mut end = (offset + 1024).min(input.len());
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&input[offset..end]);
        progress();
        controls.check_interruption()?;
        offset = end;
    }
    Ok(())
}
