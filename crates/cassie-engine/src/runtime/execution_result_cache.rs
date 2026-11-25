//! SQL result-cache ownership; serialized limits remain distinct from retained capacity.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::executor::QueryResult;

use super::cache_state::{ExecutionResultCacheEntry, ExecutionResultCacheState};
use super::owned_memory::allocation_error;
use super::{
    ExecutionResultCacheKey, QueryExecutionControls, QueryMemoryReservation, RuntimeState,
};

impl RuntimeState {
    pub(crate) fn execution_result_cache_lookup_with_controls(
        &self,
        key: &ExecutionResultCacheKey,
        controls: &QueryExecutionControls,
    ) -> Result<Option<QueryResult>, CassieError> {
        controls.check_interruption()?;
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        let Some(entry) = cache.entries.get(key) else {
            drop(cache);
            self.record_execution_result_cache_miss();
            return Ok(None);
        };
        let result = entry.result.try_clone_for_query(controls)?;
        touch(&mut cache.order, key);
        drop(cache);
        self.record_execution_result_cache_hit();
        Ok(Some(result))
    }

    /// Shares the existing admitted producer view. Distinct queries must admit their own view.
    ///
    /// # Panics
    /// Panics if the cache lock is poisoned.
    pub fn execution_result_cache_lookup(
        &self,
        key: &ExecutionResultCacheKey,
    ) -> Option<QueryResult> {
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        let result = cache.entries.get(key).map(|entry| entry.result.clone());
        if result.is_some() {
            touch(&mut cache.order, key);
        }
        drop(cache);
        if result.is_some() {
            self.record_execution_result_cache_hit();
        } else {
            self.record_execution_result_cache_miss();
        }
        result
    }

    /// Retains a shared owner; optional key/container allocations may bypass the cache.
    ///
    /// # Panics
    /// Panics if the cache lock is poisoned.
    pub fn execution_result_cache_store(&self, key: &ExecutionResultCacheKey, result: QueryResult) {
        let max_entries = self.limits.execution_result_cache_max_entries;
        let max_bytes = self.limits.execution_result_cache_max_bytes;
        if !self.limits.execution_result_cache_enabled.is_enabled()
            || max_entries == 0
            || max_bytes == 0
        {
            return;
        }
        let Ok(bytes) = serialized_bytes(&result) else {
            self.record_execution_result_cache_bypass("serialization");
            return;
        };
        if bytes > max_bytes {
            self.record_execution_result_cache_bypass("oversized_entry");
            return;
        }
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        if let Some(entry) = cache.entries.get_mut(key) {
            let previous_bytes = entry.bytes;
            entry.result = result;
            entry.bytes = bytes;
            cache.bytes = cache
                .bytes
                .saturating_sub(previous_bytes)
                .saturating_add(bytes);
            touch(&mut cache.order, key);
        } else {
            let admission = result.admission();
            let prepared = key_clone_bytes(key)
                .and_then(|bytes| admission.try_sibling(mul(bytes, 2)?))
                .and_then(|memory| {
                    prepare_capacity(&mut cache, admission)?;
                    Ok(memory)
                });
            let Ok(key_memory) = prepared else {
                drop(cache);
                self.record_execution_result_cache_bypass("memory_budget");
                return;
            };
            // Both owned key copies and both backing containers are admitted before cloning.
            let map_key = key.clone();
            let order_key = key.clone();
            cache.entries.insert(
                map_key,
                ExecutionResultCacheEntry {
                    result,
                    bytes,
                    key_memory,
                },
            );
            cache.order.push_back(order_key);
            cache.bytes = cache.bytes.saturating_add(bytes);
        }
        let mut evictions = 0;
        while cache.entries.len() > max_entries || cache.bytes > max_bytes {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            let removed = cache.entries.remove(&oldest);
            drop(oldest);
            if let Some(removed) = removed {
                cache.bytes = cache.bytes.saturating_sub(removed.bytes);
                // The deque key and map key have dropped before their entry-held lease.
                drop(removed);
                evictions += 1;
            }
        }
        drop(cache);
        self.record_execution_result_cache_eviction(evictions);
    }

    /// # Panics
    /// Panics if the cache lock is poisoned.
    pub fn invalidate_execution_result_cache(&self) {
        let mut cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        let retired = std::mem::take(&mut *cache);
        drop(retired);
    }

    pub(super) fn execution_result_cache_size(&self) -> (usize, usize) {
        let cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        (cache.entries.len(), cache.bytes)
    }

    pub(super) fn execution_result_cache_retained_bytes(&self) -> usize {
        let cache = self
            .execution_result_cache
            .lock()
            .expect("execution result cache");
        let capacity = cache
            .map_memory
            .as_ref()
            .map_or(0, QueryMemoryReservation::bytes)
            .saturating_add(
                cache
                    .order_memory
                    .as_ref()
                    .map_or(0, QueryMemoryReservation::bytes),
            );
        cache
            .entries
            .values()
            .enumerate()
            .fold(capacity, |bytes, (index, entry)| {
                let unique = !cache
                    .entries
                    .values()
                    .take(index)
                    .any(|previous| previous.result.shares_storage(&entry.result));
                let unique_exposure = !cache
                    .entries
                    .values()
                    .take(index)
                    .any(|previous| previous.result.shares_exposure(&entry.result));
                bytes
                    .saturating_add(entry.key_memory.bytes())
                    .saturating_add(if unique {
                        entry.result.retained_bytes()
                    } else {
                        0
                    })
                    .saturating_add(if unique_exposure {
                        entry.result.exposure_storage_bytes()
                    } else {
                        0
                    })
            })
    }
}

fn touch(order: &mut VecDeque<ExecutionResultCacheKey>, key: &ExecutionResultCacheKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        let owned = order.remove(position).expect("located cache key");
        order.push_back(owned);
    }
}

fn key_clone_bytes(key: &ExecutionResultCacheKey) -> Result<usize, CassieError> {
    let strings = add(
        key.user.capacity(),
        key.database.as_ref().map_or(0, String::capacity),
    )?;
    key.search_path.iter().try_fold(
        add(
            strings,
            mul(key.search_path.capacity(), size_of::<String>())?,
        )?,
        |bytes, value| add(bytes, value.capacity()),
    )
}

fn prepare_capacity(
    cache: &mut ExecutionResultCacheState,
    admission: &QueryMemoryReservation,
) -> Result<(), CassieError> {
    let needed = add(cache.entries.len(), 1)?;
    if needed <= cache.entries.capacity() && needed <= cache.order.capacity() {
        return Ok(());
    }
    let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
        CassieError::ResourceLimit("execution result cache capacity overflow".to_owned())
    })?;
    let map_bytes =
        hash_table_bytes::<(ExecutionResultCacheKey, ExecutionResultCacheEntry)>(capacity)?;
    let map_memory = admission.try_sibling(map_bytes)?;
    let order_memory =
        admission.try_sibling(mul(capacity, size_of::<ExecutionResultCacheKey>())?)?;
    let mut entries = HashMap::new();
    let mut order = VecDeque::new();
    entries
        .try_reserve(capacity)
        .map_err(|error| allocation_error(&error))?;
    order
        .try_reserve_exact(capacity)
        .map_err(|error| allocation_error(&error))?;
    if hash_table_bytes::<(ExecutionResultCacheKey, ExecutionResultCacheEntry)>(entries.capacity())?
        > map_bytes
        || order.capacity() > capacity
    {
        return Err(CassieError::ResourceLimit(
            "cache allocator exceeded admitted capacity".to_owned(),
        ));
    }
    entries.extend(cache.entries.drain());
    order.extend(cache.order.drain(..));
    drop(std::mem::replace(&mut cache.entries, entries));
    drop(std::mem::replace(&mut cache.order, order));
    // Old directories are physically retired before their reservations are replaced.
    cache.map_memory = Some(map_memory);
    cache.order_memory = Some(order_memory);
    Ok(())
}

fn serialized_bytes(result: &QueryResult) -> Result<usize, serde_json::Error> {
    let mut counter = CountingWriter { bytes: 0 };
    serde_json::to_writer(&mut counter, result)?;
    Ok(counter.bytes)
}

struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("cache serialized length overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
