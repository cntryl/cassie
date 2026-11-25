use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::allocation::PlanAllocation;

pub(crate) struct PlanText {
    value: Value,
    _memory: QueryMemoryReservation,
}

impl PlanText {
    pub(super) fn try_build(
        controls: &QueryExecutionControls,
        build: impl FnOnce(&mut PlanAllocation<'_>) -> Result<String, CassieError>,
    ) -> Result<Self, CassieError> {
        let mut allocation = PlanAllocation::new(controls, 0)?;
        let value = build(&mut allocation)?;
        allocation.finish_retained(value.capacity())?;
        let memory = allocation.into_memory();
        Ok(Self {
            value: Value::String(value),
            _memory: memory,
        })
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }

    pub(crate) fn try_append(
        &mut self,
        controls: &QueryExecutionControls,
        build: impl FnOnce(&mut PlanAllocation<'_>) -> Result<String, CassieError>,
    ) -> Result<(), CassieError> {
        let Value::String(original) = &self.value else {
            unreachable!("plan text is a string")
        };
        let mut allocation = PlanAllocation::new(controls, 0)?;
        let suffix = build(&mut allocation)?;
        let value = allocation.format(format_args!("{original}{suffix}"))?;
        drop(suffix);
        allocation.finish_retained(value.capacity())?;
        let memory = allocation.into_memory();
        let previous = std::mem::replace(
            self,
            Self {
                value: Value::String(value),
                _memory: memory,
            },
        );
        drop(previous);
        Ok(())
    }
}
