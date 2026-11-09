//! Detached, owned object keys and raw sources for iterative containment.
use super::{containment, object, render::RenderError};
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use serde_json::value::RawValue;
use std::cmp::Ordering;

pub(in crate::executor::filter) struct ObjectPair {
    candidate: Box<RawValue>,
    required: Box<RawValue>,
    candidate_fields: object::DetachedFields,
    required_fields: object::DetachedFields,
    candidate_index: usize,
    required_index: usize,
}

pub(in crate::executor::filter) enum ObjectStep<'a> {
    Child(&'a str, &'a str),
    Missing,
    Done,
}

impl ObjectPair {
    pub(in crate::executor::filter) fn build(
        candidate: Box<RawValue>,
        required: Box<RawValue>,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        let candidates = object::parse(candidate.get(), controls)
            .map_err(RenderError::from)
            .map_err(RenderError::into_query_error)?;
        let requirements = object::parse(required.get(), controls)
            .map_err(RenderError::from)
            .map_err(RenderError::into_query_error)?;
        let candidate_fields = candidates.into_lexical(controls)?.into_detached();
        let required_fields = requirements.into_lexical(controls)?.into_detached();
        Ok(Self {
            candidate,
            required,
            candidate_fields,
            required_fields,
            candidate_index: 0,
            required_index: 0,
        })
    }

    pub(in crate::executor::filter) fn next(
        &mut self,
        controls: &QueryExecutionControls,
    ) -> Result<ObjectStep<'_>, QueryError> {
        loop {
            controls.check_interruption()?;
            let Some((key, value)) = self
                .required_fields
                .get(self.required.get(), self.required_index)
            else {
                return Ok(ObjectStep::Done);
            };
            let Some((candidate_key, candidate_value)) = self
                .candidate_fields
                .get(self.candidate.get(), self.candidate_index)
            else {
                return Ok(ObjectStep::Missing);
            };
            match containment::compare(candidate_key, key, controls)? {
                Ordering::Less => self.candidate_index += 1,
                Ordering::Greater => return Ok(ObjectStep::Missing),
                Ordering::Equal => {
                    self.candidate_index += 1;
                    self.required_index += 1;
                    return Ok(ObjectStep::Child(candidate_value, value));
                }
            }
        }
    }
}
