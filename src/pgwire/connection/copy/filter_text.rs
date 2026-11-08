//! Checked temporary strings for COPY WHERE expression rewriting.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct WhereText {
    text: String,
    memory: QueryMemoryReservation,
}

impl WhereText {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            text: String::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    pub(super) fn push(
        &mut self,
        value: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let needed = self.text.len().checked_add(value.len()).ok_or_else(|| {
            CassieError::ResourceLimit("COPY WHERE text capacity overflow".into())
        })?;
        if needed > self.text.capacity() {
            let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
                CassieError::ResourceLimit("COPY WHERE text capacity overflow".into())
            })?;
            let replacement_memory = controls.reserve_query_memory(capacity)?;
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "allocator exceeded admitted COPY WHERE capacity".into(),
                ));
            }
            copy_chunks(&mut replacement, &self.text, controls)?;
            let old = std::mem::replace(&mut self.text, replacement);
            drop(old);
            self.memory.shrink_to(0);
            self.memory.absorb(replacement_memory);
        }
        copy_chunks(&mut self.text, value, controls)
    }

    pub(super) fn push_parameter(
        &mut self,
        index: usize,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let mut digits = [0_u8; 20];
        let mut value = index
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("COPY parameter index overflow".into()))?;
        let mut offset = digits.len();
        while value != 0 {
            offset -= 1;
            digits[offset] = b'0' + u8::try_from(value % 10).expect("decimal digit");
            value /= 10;
        }
        self.push("$", controls)?;
        let text = std::str::from_utf8(&digits[offset..])
            .map_err(|_| CassieError::Execution("invalid COPY parameter index".into()))?;
        self.push(text, controls)
    }

    pub(super) fn into_parts(self) -> (String, QueryMemoryReservation) {
        (self.text, self.memory)
    }
}

fn copy_chunks(
    output: &mut String,
    value: &str,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let mut start = 0;
    while start < value.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&value[start..end]);
        start = end;
    }
    Ok(())
}
