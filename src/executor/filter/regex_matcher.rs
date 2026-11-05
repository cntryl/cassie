//! One private match/capture interface for native regular and owned constraint paths.

use super::{bounded, constraints, QueryError};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use regex_automata::{util::captures::Captures as NativeCaptures, Input, Match, Span};

pub(super) enum Regex {
    Native(NativeRegex),
    Constraint(constraints::Regex),
}

pub(super) struct NativeRegex {
    regex: Box<bounded::Regex>,
    _memory: Option<QueryMemoryReservation>,
}

pub(super) enum Captures {
    Native(NativeCaptures),
    Constraint(constraints::Captures),
}

impl Regex {
    pub(super) fn native(
        regex: bounded::Regex,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        let memory = controls
            .map(|controls| controls.reserve_query_memory(size_of::<bounded::Regex>()))
            .transpose()?;
        Ok(Self::Native(NativeRegex {
            regex: Box::new(regex),
            _memory: memory,
        }))
    }

    pub(super) fn is_match(
        &self,
        input: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<bool, QueryError> {
        match self {
            Self::Native(regex) => Ok(regex.regex.is_match(input)),
            Self::Constraint(regex) => regex
                .find(&Input::new(input), controls)
                .map(|matched| matched.is_some()),
        }
    }

    pub(super) fn find(
        &self,
        input: Input<'_>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Option<Match>, QueryError> {
        match self {
            Self::Native(regex) => Ok(regex.regex.find(input)),
            Self::Constraint(regex) => regex.find(&input, controls),
        }
    }

    pub(super) fn create_captures(
        &self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Captures, QueryError> {
        match self {
            Self::Native(regex) => Ok(Captures::Native(regex.regex.create_captures())),
            Self::Constraint(regex) => regex.create_captures(controls).map(Captures::Constraint),
        }
    }

    pub(super) fn search_captures(
        &self,
        input: &Input<'_>,
        captures: &mut Captures,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), QueryError> {
        match (self, captures) {
            (Self::Native(regex), Captures::Native(captures)) => {
                regex.regex.search_captures(input, captures);
                Ok(())
            }
            (Self::Constraint(regex), Captures::Constraint(captures)) => {
                regex.search_captures(input, captures, controls)
            }
            _ => unreachable!("captures are created by their matching private regex"),
        }
    }
}

impl Captures {
    pub(super) fn get_match(&self) -> Option<Match> {
        match self {
            Self::Native(captures) => captures.get_match(),
            Self::Constraint(captures) => captures.get_match(),
        }
    }

    pub(super) fn get_group(&self, group: usize) -> Option<Span> {
        match self {
            Self::Native(captures) => captures.get_group(group),
            Self::Constraint(captures) => captures.get_group(group),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bounded, Regex};
    use crate::{
        app::CassieError, config::CassieRuntimeLimits, executor::filter::QueryError,
        runtime::QueryExecutionControls,
    };

    #[test]
    fn should_admit_the_native_wrapper_before_heap_allocation() {
        // Arrange
        let size = size_of::<bounded::Regex>();
        let budget = 64 * 1024;
        for spare in [size - 1, size] {
            let limits = CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            };
            let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
            let native = bounded::Regex::build(
                "x",
                regex_automata::util::syntax::Config::new(),
                None,
                Some(&controls),
            )
            .expect("small admitted regex");
            let retained = controls.current_query_memory_bytes();
            let filler = controls
                .reserve_query_memory(budget - retained - spare)
                .expect("boundary filler");

            // Act
            let result = Regex::native(native, Some(&controls));

            // Assert
            if spare < size {
                assert!(matches!(
                    result,
                    Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
                ));
            } else {
                let wrapper = result.expect("exact wrapper admission");
                assert_eq!(controls.current_query_memory_bytes(), budget);
                drop(wrapper);
            }
            assert_eq!(controls.current_query_memory_bytes(), filler.bytes());
            drop(filler);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
