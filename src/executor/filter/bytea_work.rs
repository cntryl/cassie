//! Scratch admission shared by codecs, bit scans and hash updates.
use super::{QueryError, Value};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct Work<'a> {
    controls: Option<&'a QueryExecutionControls>,
    _memory: Option<QueryMemoryReservation>,
}
impl<'a> Work<'a> {
    pub(super) fn admit(
        args: &[Value],
        controls: Option<&'a QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        // Covers decoded input, codec expansion, carrier and result copies.
        // Output collectors separately own retained result memory.
        let input = args
            .iter()
            .try_fold(0_usize, |length, argument| {
                length.checked_add(argument.as_str().map_or(0, str::len))
            })
            .and_then(|bytes| bytes.checked_mul(12))
            .and_then(|bytes| bytes.checked_add(1024))
            .ok_or_else(|| {
                QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                    "BYTEA function scratch overflow".into(),
                ))
            })?;
        let memory = controls
            .map(|controls| controls.reserve_query_memory(input))
            .transpose()?;
        Ok(Self {
            controls,
            _memory: memory,
        })
    }
    pub(super) fn check(&self) -> Result<(), QueryError> {
        self.controls
            .map_or(Ok(()), QueryExecutionControls::check_interruption)
            .map_err(Into::into)
    }
}
