//! Private text owners for pattern preparation and nested scalar handoff.

use super::{check_interruption, resource_limit, QueryError};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct PatternText {
    text: String,
    memory: Option<QueryMemoryReservation>,
}

pub(super) struct Writer {
    text: String,
    admitted: usize,
}

impl Writer {
    pub(super) fn len(&self) -> usize {
        self.text.len()
    }

    pub(super) fn push_str(&mut self, text: &str) -> Result<(), QueryError> {
        let length = self
            .text
            .len()
            .checked_add(text.len())
            .ok_or_else(|| resource_limit("pattern text size overflow"))?;
        if length > self.admitted {
            return Err(resource_limit(
                "pattern text exceeded pre-admitted capacity",
            ));
        }
        self.text.push_str(text);
        Ok(())
    }

    pub(super) fn push(&mut self, character: char) -> Result<(), QueryError> {
        self.push_str(character.encode_utf8(&mut [0; 4]))
    }
}

impl PatternText {
    pub(super) fn build(
        capacity: usize,
        controls: Option<&QueryExecutionControls>,
        build: impl FnOnce(&mut Writer) -> Result<(), QueryError>,
    ) -> Result<Self, QueryError> {
        check_interruption(controls)?;
        let memory = reserve(controls, capacity)?;
        let mut text = String::new();
        text.try_reserve_exact(capacity).map_err(|error| {
            resource_limit(&format!(
                "unable to allocate admitted pattern text: {error}"
            ))
        })?;
        if text.capacity() > capacity {
            return Err(resource_limit(
                "pattern allocator exceeded admitted capacity",
            ));
        }
        let mut writer = Writer {
            text,
            admitted: capacity,
        };
        build(&mut writer)?;
        check_interruption(controls)?;
        Ok(Self {
            text: writer.text,
            memory,
        })
    }

    pub(super) fn copy(
        input: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        Self::build(input.len(), controls, |writer| writer.push_str(input))
    }

    pub(super) fn transform(
        self,
        capacity: usize,
        controls: Option<&QueryExecutionControls>,
        build: impl FnOnce(&str, &mut Writer) -> Result<(), QueryError>,
    ) -> Result<Self, QueryError> {
        Self::build(capacity, controls, |writer| build(&self.text, writer))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    pub(super) fn into_parts(self) -> (String, Option<QueryMemoryReservation>) {
        (self.text, self.memory)
    }
}

pub(super) fn reserve(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    check_interruption(controls)?;
    controls
        .map(|controls| controls.reserve_query_memory(bytes).map_err(Into::into))
        .transpose()
}

pub(super) fn syntax_scratch(
    pattern_bytes: usize,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    // AST nodes, span metadata, parser/visitor stacks and transition records
    // have a linear source-size envelope in locked regex-syntax 0.8.11.
    // This is not the Unicode-expanded HIR/native compiler workspace bound.
    let bytes = pattern_bytes
        .checked_mul(512)
        .and_then(|bytes| bytes.checked_add(4096))
        .ok_or_else(|| resource_limit("pattern AST workspace size overflow"))?;
    reserve(controls, bytes)
}

#[cfg(test)]
mod tests {
    use super::PatternText;
    use crate::{
        app::CassieError, config::CassieRuntimeLimits, executor::filter::QueryError,
        runtime::QueryExecutionControls,
    };

    #[test]
    fn should_keep_pattern_translation_overlap_charged() {
        // Arrange
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 7,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
        let original = PatternText::copy("abc", Some(&controls)).expect("original admitted text");

        // Act
        let translated = original.transform(4, Some(&controls), |input, writer| {
            writer.push_str(input)?;
            writer.push('!')
        });
        let peak = controls.peak_query_memory_bytes();
        let retained = controls.current_query_memory_bytes();
        let text = translated.expect("old/new overlap fits seven bytes");
        let observed = text.as_str().to_owned();
        drop(text);

        // Assert
        assert_eq!(observed, "abc!");
        assert_eq!(peak, 7);
        assert_eq!(retained, 4);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reject_pattern_growth_before_reallocation() {
        // Arrange
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 3,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());

        // Act
        let result = PatternText::build(3, Some(&controls), |writer| writer.push_str("four"));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 3);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
