//! The SQL array directory owns its lease independently of element JSONB keys.
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

use super::{compare_sql_arrays, SemanticValue};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{SqlArray, Value};

#[derive(Debug)]
struct Owned {
    dimensions: Vec<(i32, i32)>,
    elements: Vec<SemanticValue>,
    _memory: Option<QueryMemoryReservation>,
}
#[derive(Debug, Clone)]
pub(crate) struct JsonbArrayKey(Arc<Owned>);
impl JsonbArrayKey {
    pub(crate) fn build(
        array: &SqlArray,
        controls: Option<&QueryExecutionControls>,
        mut element_key: impl FnMut(&Value) -> Result<SemanticValue, CassieError>,
    ) -> Result<Self, CassieError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let bytes = array
            .dimensions()
            .len()
            .checked_mul(size_of::<(i32, i32)>())
            .and_then(|bytes| {
                bytes.checked_add(
                    array
                        .elements()
                        .len()
                        .checked_mul(size_of::<SemanticValue>())?,
                )
            })
            .and_then(|bytes| bytes.checked_add(size_of::<Owned>() + 2 * size_of::<usize>()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("JSONB array key accounting overflow".into())
            })?;
        let memory = controls
            .map(|controls| controls.reserve_query_memory(bytes))
            .transpose()?;
        let mut dimensions = Vec::new();
        dimensions
            .try_reserve_exact(array.dimensions().len())
            .map_err(|error| allocation(&error))?;
        for dimension in array.dimensions() {
            dimensions.push((dimension.length, dimension.lower_bound));
        }
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(array.elements().len())
            .map_err(|error| allocation(&error))?;
        for value in array.elements() {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            elements.push(element_key(value)?);
        }
        Ok(Self(Arc::new(Owned {
            dimensions,
            elements,
            _memory: memory,
        })))
    }
}
impl PartialEq for JsonbArrayKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.dimensions == other.0.dimensions && self.0.elements == other.0.elements
    }
}
impl Eq for JsonbArrayKey {}
impl Hash for JsonbArrayKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.dimensions.hash(state);
        self.0.elements.hash(state);
    }
}
impl PartialOrd for JsonbArrayKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for JsonbArrayKey {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_sql_arrays(
            &self.0.dimensions,
            &self.0.elements,
            &other.0.dimensions,
            &other.0.elements,
        )
    }
}
fn allocation(error: &std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("JSONB array key allocation: {error}"))
}
