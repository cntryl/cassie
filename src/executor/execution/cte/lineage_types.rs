//! Borrowed catalog authority for the private lineage's finite key markers.
use super::super::QueryError;
use crate::executor::lineage_keys::KeyType;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{CteCycle, CteSearch};
use crate::types::FieldSchema;

pub(super) struct LineageTypes {
    pub(super) search: Vec<KeyType>,
    pub(super) cycle: Vec<KeyType>,
    _memory: QueryMemoryReservation,
}
impl LineageTypes {
    pub(super) fn new(
        fields: &[FieldSchema],
        search: Option<&CteSearch>,
        cycle: Option<&CteCycle>,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        controls.check_interruption()?;
        let search_names = search.map_or(&[][..], |search| search.by.as_slice());
        let cycle_names = cycle.map_or(&[][..], |cycle| cycle.by.as_slice());
        let count = search_names
            .len()
            .checked_add(cycle_names.len())
            .ok_or_else(overflow)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<KeyType>())
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<Vec<KeyType>>()))
            .ok_or_else(overflow)?;
        let memory = controls.reserve_query_memory(bytes)?;
        Ok(Self {
            search: collect(fields, search_names, controls)?,
            cycle: collect(fields, cycle_names, controls)?,
            _memory: memory,
        })
    }
    pub(super) fn count(&self) -> usize {
        self.search.len() + self.cycle.len()
    }
}
fn collect(
    fields: &[FieldSchema],
    names: &[String],
    controls: &QueryExecutionControls,
) -> Result<Vec<KeyType>, QueryError> {
    let mut types = Vec::new();
    types
        .try_reserve_exact(names.len())
        .map_err(|error| super::retention::allocation(&error))?;
    for name in names {
        controls.check_interruption()?;
        let field = fields
            .iter()
            .find(|field| field.name == *name)
            .ok_or_else(|| {
                QueryError::General(format!(
                    "private SEARCH/CYCLE key '{name}' has no declared field"
                ))
            })?;
        types.push(KeyType::from_type(&field.data_type));
    }
    Ok(types)
}
fn overflow() -> QueryError {
    crate::app::CassieError::ResourceLimit("private lineage type accounting overflow".into()).into()
}
