//! Private lease links preserve origin and all prior operator reservations.
use std::mem::size_of;
use std::sync::Arc;

use super::BatchRow;
use crate::app::CassieError;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
pub(crate) struct OperatorMemory {
    _parents: [Option<Arc<Self>>; 2],
    _reservation: Arc<QueryMemoryReservation>,
    _metadata: QueryMemoryReservation,
    controls: QueryExecutionControls,
}

impl OperatorMemory {
    pub(crate) fn hold_rows(
        controls: &QueryExecutionControls,
        rows: &[BatchRow],
    ) -> Result<AccountedVec<Arc<Self>>, CassieError> {
        let mut parents = AccountedVec::try_new(controls)?;
        for row in rows {
            if let Some(parent) = row.operator_memory() {
                parents.try_push_with(0, || parent)?;
            }
        }
        Ok(parents)
    }

    fn new(
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
        parents: [Option<Arc<Self>>; 2],
    ) -> Result<Arc<Self>, CassieError> {
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        let metadata = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            _parents: parents,
            _reservation: reservation,
            _metadata: metadata,
            controls: controls.clone(),
        }))
    }

    pub(crate) fn merge(
        left: Option<Arc<Self>>,
        right: Option<Arc<Self>>,
    ) -> Result<Option<Arc<Self>>, CassieError> {
        match (left, right) {
            (Some(left), Some(right)) if !Arc::ptr_eq(&left, &right) => {
                let controls = left.controls.clone();
                let reservation = Arc::new(controls.reserve_query_memory(
                    size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
                )?);
                Self::new(&controls, reservation, [Some(left), Some(right)]).map(Some)
            }
            (Some(left), _) => Ok(Some(left)),
            (_, right) => Ok(right),
        }
    }
}

impl BatchRow {
    /// Admission for an existing body that arrived without any retained source/operator owner.
    pub(crate) fn unleased_body_bytes(&self) -> Result<usize, CassieError> {
        use crate::executor::retained_memory::{
            add, data_type_clone_bytes, hash_table_bytes, mul, value_clone_bytes,
        };
        use crate::types::{DataType, Value};
        if self.query_memory.is_some() || self.operator_memory.is_some() {
            return Ok(0);
        }
        let mut bytes = add(
            mul(self.values.capacity(), size_of::<(String, Value)>())?,
            mul(self.aliases.capacity(), size_of::<(String, usize)>())?,
        )?;
        for (name, value) in &self.values {
            bytes = add(bytes, add(name.capacity(), value_clone_bytes(value)?)?)?;
        }
        for (name, _) in &self.aliases {
            bytes = add(bytes, name.capacity())?;
        }
        if let Some(lookup) = self.lookup.get() {
            bytes = add(
                bytes,
                hash_table_bytes::<(String, usize)>(lookup.capacity())?,
            )?;
            for name in lookup.keys() {
                bytes = add(bytes, name.capacity())?;
            }
        }
        if let Some(types) = &self.data_types {
            bytes = add(
                bytes,
                add(
                    size_of::<Vec<DataType>>() + 2 * size_of::<usize>(),
                    mul(types.capacity(), size_of::<DataType>())?,
                )?,
            )?;
            for data_type in types.iter() {
                bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
            }
        }
        if let Some(outer) = &self.outer_scope {
            bytes = add(bytes, outer.unleased_body_bytes()?)?;
        }
        Ok(bytes)
    }

    pub(crate) fn retain_operator_memory(
        mut self,
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
    ) -> Result<Self, CassieError> {
        self.attach_operator_memory(controls, reservation)?;
        Ok(self)
    }

    pub(crate) fn attach_operator_memory(
        &mut self,
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
    ) -> Result<(), CassieError> {
        let previous = self.operator_memory.clone();
        // A reconstruction may replace the current origin while keeping an older operator root.
        // Retain that current owner independently, without silently overwriting either origin.
        let origin = self
            .query_memory()
            .map(|origin| OperatorMemory::new(controls, origin, [None, None]))
            .transpose()?;
        self.operator_memory = Some(OperatorMemory::new(
            controls,
            reservation,
            [previous, origin],
        )?);
        Ok(())
    }

    pub(crate) fn operator_memory(&self) -> Option<Arc<OperatorMemory>> {
        self.operator_memory.clone()
    }

    pub(crate) fn with_operator_memory(mut self, memory: Option<Arc<OperatorMemory>>) -> Self {
        self.operator_memory = memory;
        self
    }
}
