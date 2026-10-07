//! Existing recursive UNION identity, with admitted candidate and retained key backing.
use std::mem::size_of;

use super::*;
use crate::executor::retained_memory::{add, hash_table_bytes, mul, serialized_json_bytes};
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryMemoryReservation;

pub(super) struct SeenRows {
    keys: HashSet<SemanticKey>,
    memory: QueryMemoryReservation,
    table_bytes: usize,
}

impl SeenRows {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            keys: HashSet::new(),
            memory: controls.reserve_query_memory(0)?,
            table_bytes: 0,
        })
    }

    pub(super) fn insert(
        &mut self,
        row: &Vec<(String, Value)>,
        controls: &QueryExecutionControls,
    ) -> Result<bool, QueryError> {
        check_timeout(controls)?;
        let key_bytes = row.iter().try_fold(
            mul(row.len(), size_of::<SemanticValue>())?,
            |bytes, (_, value)| {
                let heap = match value {
                    Value::String(text) => text.len().max(27),
                    Value::Vector(vector) => mul(vector.values.len(), size_of::<u32>())?,
                    Value::Json(value) => mul(serialized_json_bytes(value)?, 2)?.max(128),
                    Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
                };
                add(bytes, heap)
            },
        )?;
        // Integral FLOAT canonicalization/comparison can create a sequential formatter temporary.
        let _candidate_memory = controls.reserve_query_memory(add(key_bytes, 64)?)?;
        let key = row_signature(row);
        if self.keys.contains(&key) {
            return Ok(false);
        }
        let growing = self.keys.len() == self.keys.capacity();
        let extra_table = if growing {
            hash_table_bytes::<SemanticKey>(mul(self.keys.capacity().max(2), 2)?)?
        } else {
            0
        };
        self.memory.try_grow(add(key_bytes, extra_table)?)?;
        if growing {
            self.keys
                .try_reserve(1)
                .map_err(|error| retention::allocation(&error))?;
        }
        self.keys.insert(key);
        let retained_table = hash_table_bytes::<SemanticKey>(self.keys.capacity())?;
        let old_plus_new = self.memory.bytes();
        self.memory.shrink_to(add(
            old_plus_new - extra_table - self.table_bytes,
            retained_table,
        )?);
        self.table_bytes = retained_table;
        check_timeout(controls)?;
        Ok(true)
    }

    pub(super) fn retain_unique(
        &mut self,
        rows: &mut RetainedRows,
        controls: &QueryExecutionControls,
    ) -> Result<(), QueryError> {
        let mut failure = None;
        rows.rows.retain(|row| {
            if failure.is_some() {
                return false;
            }
            #[cfg(test)]
            let before = controls.current_query_memory_bytes();
            match self.insert(row, controls) {
                Ok(inserted) => {
                    #[cfg(test)]
                    if inserted {
                        retention_tests::observe_seen(
                            before,
                            controls.current_query_memory_bytes(),
                        );
                    }
                    inserted
                }
                Err(error) => {
                    failure = Some(error);
                    false
                }
            }
        });
        failure.map_or(Ok(()), Err)
    }
}
