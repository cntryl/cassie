//! One private match/capture interface for native regular and owned constraint paths.

use super::{bounded, constraints, QueryError};
use crate::runtime::QueryExecutionControls;
use regex_automata::{util::captures::Captures as NativeCaptures, Input, Match, Span};

pub(super) enum Regex {
    Native(bounded::Regex),
    Constraint(constraints::Regex),
}

pub(super) enum Captures {
    Native(NativeCaptures),
    Constraint(constraints::Captures),
}

impl Regex {
    pub(super) fn is_match(
        &self,
        input: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<bool, QueryError> {
        match self {
            Self::Native(regex) => Ok(regex.is_match(input)),
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
            Self::Native(regex) => Ok(regex.find(input)),
            Self::Constraint(regex) => regex.find(&input, controls),
        }
    }

    pub(super) fn create_captures(
        &self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Captures, QueryError> {
        match self {
            Self::Native(regex) => Ok(Captures::Native(regex.create_captures())),
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
                regex.search_captures(input, captures);
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
