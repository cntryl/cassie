//! Owned scalar text for the isolated JSON forest.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::super::cursor::number::NumberToken;
use super::super::cursor::string::DecodedText;
#[cfg(test)]
use super::Progress;

pub(super) struct OwnedText {
    value: String,
    _memory: QueryMemoryReservation,
}

impl OwnedText {
    pub(super) fn adopt_number(
        token: NumberToken,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        if !token.shares_tracker(controls) {
            return Err(CassieError::Execution(
                "JSON number token uses a different query tracker".into(),
            ));
        }
        let (value, memory) = token.into_parts();
        Ok(Self {
            value,
            _memory: memory,
        })
    }

    pub(super) fn adopt(
        token: DecodedText,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        if !token.shares_tracker(controls) {
            return Err(CassieError::Execution(
                "JSON token uses a different query tracker".into(),
            ));
        }
        let (value, memory) = token.into_parts();
        Ok(Self {
            value,
            _memory: memory,
        })
    }

    #[cfg(test)]
    pub(super) fn copy(
        input: &str,
        controls: &QueryExecutionControls,
        observer: &mut impl FnMut(Progress),
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let memory = controls.reserve_query_memory(input.len())?;
        let mut value = String::new();
        value
            .try_reserve_exact(input.len())
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if value.capacity() > input.len() {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted JSON text capacity".into(),
            ));
        }
        let mut start = 0;
        while start < input.len() {
            controls.check_interruption()?;
            let mut end = start.saturating_add(1024).min(input.len());
            while !input.is_char_boundary(end) {
                end -= 1;
            }
            value.push_str(&input[start..end]);
            start = end;
            observer(Progress::TextCopy);
            controls.check_interruption()?;
        }
        Ok(Self {
            value,
            _memory: memory,
        })
    }

    pub(super) fn as_str(&self) -> &str {
        &self.value
    }
}
