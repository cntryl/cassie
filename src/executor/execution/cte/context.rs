//! The existing independent CTE namespace with its own retained map reservation.
use std::ops::Deref;

use super::*;
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::runtime::QueryMemoryReservation;

pub(in crate::executor::execution) struct CteContext {
    relations: HashMap<String, CteRelation>,
    memory: Option<QueryMemoryReservation>,
}

impl Deref for CteContext {
    type Target = HashMap<String, CteRelation>;
    fn deref(&self) -> &Self::Target {
        &self.relations
    }
}

impl CteContext {
    pub(in crate::executor::execution) fn new() -> Self {
        Self {
            relations: HashMap::new(),
            memory: None,
        }
    }

    fn retained_bytes(&self) -> Result<usize, QueryError> {
        self.relations.keys().try_fold(
            hash_table_bytes::<(String, CteRelation)>(self.relations.capacity())?,
            |bytes, name| Ok(add(bytes, name.capacity())?),
        )
    }

    pub(in crate::executor::execution) fn remove(&mut self, name: &str) -> Option<CteRelation> {
        // Capacity persists after removal, so its owner stays in the context.
        self.relations.remove(name)
    }

    pub(in crate::executor::execution) fn insert(
        &mut self,
        name: &str,
        relation: CteRelation,
        controls: &QueryExecutionControls,
    ) -> Result<Option<CteRelation>, QueryError> {
        check_timeout(controls)?;
        let growing =
            !self.relations.contains_key(name) && self.relations.len() == self.relations.capacity();
        let slots = if growing {
            mul(self.relations.capacity().max(2), 2)?
        } else {
            0
        };
        let extra = add(
            name.len(),
            hash_table_bytes::<(String, CteRelation)>(slots)?,
        )?;
        let memory = self.memory.get_or_insert(controls.reserve_query_memory(0)?);
        memory.try_grow(extra)?;
        if growing {
            self.relations
                .try_reserve(1)
                .map_err(|error| retention::allocation(&error))?;
        }
        #[cfg(test)]
        retention_tests::observe_key_copy(controls, name);
        let previous = self.relations.insert(name.to_owned(), relation);
        let bytes = self.retained_bytes()?;
        let memory = self.memory.as_mut().expect("map reservation admitted");
        memory.shrink_to(bytes);
        Ok(previous)
    }

    pub(in crate::executor::execution) fn copy(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        check_timeout(controls)?;
        let memory = controls.reserve_query_memory(self.retained_bytes()?)?;
        let mut relations = HashMap::new();
        relations
            .try_reserve(self.relations.len())
            .map_err(|error| retention::allocation(&error))?;
        for (name, relation) in &self.relations {
            check_timeout(controls)?;
            let copied = relation.copy(controls)?;
            relations.insert(name.clone(), copied);
        }
        check_timeout(controls)?;
        Ok(Self {
            relations,
            memory: Some(memory),
        })
    }

    #[cfg(test)]
    pub(in crate::executor::execution) fn unleased(
        relations: HashMap<String, CteRelation>,
    ) -> Self {
        Self {
            relations,
            memory: None,
        }
    }
}
