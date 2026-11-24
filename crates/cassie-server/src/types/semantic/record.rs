//! Private normalized SEARCH records retain only their narrow memory owner.
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

use super::SemanticValue;
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
struct Owned {
    value: SemanticValue,
    _memory: Option<QueryMemoryReservation>,
}
#[derive(Debug, Clone)]
pub(crate) struct RecordKey(Arc<Owned>);
impl RecordKey {
    pub(crate) fn build(
        controls: Option<&QueryExecutionControls>,
        workspace: usize,
        build: impl FnOnce() -> Result<Option<(SemanticValue, usize)>, CassieError>,
    ) -> Result<Option<Self>, CassieError> {
        check(controls)?;
        let header = size_of::<Owned>() + 2 * size_of::<usize>();
        let bound = header.checked_add(workspace).ok_or_else(overflow)?;
        let mut memory = controls
            .map(|controls| controls.reserve_query_memory(bound))
            .transpose()?;
        let Some((value, retained)) = build()? else {
            return Ok(None);
        };
        check(controls)?;
        if retained > workspace {
            return Err(overflow());
        }
        if let Some(memory) = memory.as_mut() {
            memory.shrink_to(header.checked_add(retained).ok_or_else(overflow)?);
        }
        Ok(Some(Self(Arc::new(Owned {
            value,
            _memory: memory,
        }))))
    }
}
impl PartialEq for RecordKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.value == other.0.value
    }
}
impl Eq for RecordKey {}
impl Hash for RecordKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.value.hash(state);
    }
}
impl PartialOrd for RecordKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RecordKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.value.cmp(&other.0.value)
    }
}
fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)
}
fn overflow() -> CassieError {
    CassieError::ResourceLimit("normalized SEARCH record exceeded its admitted shape".into())
}
