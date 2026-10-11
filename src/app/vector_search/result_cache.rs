//! Owned execution-result keys and directory storage; input-vector caches remain separate.

use std::mem::size_of;

use crate::executor::QueryResult;
use crate::runtime::owned_memory::{allocation_error, try_reserve_vec};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::accounting::{checked_add, checked_mul};
use super::{CassieError, DistanceMetric, VectorSearchInput, VectorSearchResultCacheKey};

pub(super) struct AdmittedVectorResultKey {
    pub(super) key: VectorSearchResultCacheKey,
    memory: QueryMemoryReservation,
}

impl AdmittedVectorResultKey {
    pub(super) fn new(
        catalog_version: u64,
        provider: &str,
        model: &str,
        dimensions: usize,
        input: VectorSearchInput<'_>,
        metric: DistanceMetric,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let memory = super::cache::reserve_cache_key::<VectorSearchResultCacheKey>(
            controls,
            &[
                provider,
                model,
                input.collection,
                input.vector_field,
                metric.as_str(),
                input.query,
            ],
        )?;
        let key = VectorSearchResultCacheKey {
            catalog_version,
            provider: copy_text(provider, controls)?,
            model: copy_text(model, controls)?,
            dimensions,
            collection: copy_text(input.collection, controls)?,
            field: copy_text(input.vector_field, controls)?,
            metric: copy_text(metric.as_str(), controls)?,
            limit: input.limit,
            offset: input.offset,
            query: copy_text(input.query, controls)?,
        };
        let admitted = Self { key, memory };
        controls.check_interruption()?;
        Ok(admitted)
    }

    fn into_entry(mut self, result: QueryResult) -> VectorResultEntry {
        // The directory now owns the inline key; this guard follows only its text allocations.
        self.memory
            .shrink_to(self.memory.bytes() - size_of::<VectorSearchResultCacheKey>());
        VectorResultEntry {
            key: self.key,
            result,
            _key_memory: self.memory,
        }
    }
}

fn copy_text(text: &str, controls: &QueryExecutionControls) -> Result<String, CassieError> {
    controls.check_interruption()?;
    let mut copied = String::new();
    copied
        .try_reserve_exact(text.len())
        .map_err(allocation_error)?;
    if copied.capacity() > text.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted vector result key capacity".to_owned(),
        ));
    }
    let mut offset = 0;
    while offset < text.len() {
        controls.check_interruption()?;
        let mut end = offset.saturating_add(4096).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        copied.push_str(&text[offset..end]);
        offset = end;
    }
    Ok(copied)
}

struct VectorResultEntry {
    key: VectorSearchResultCacheKey,
    result: QueryResult,
    _key_memory: QueryMemoryReservation,
}

#[derive(Default)]
pub(in crate::app) struct VectorResultCache {
    entries: Vec<VectorResultEntry>,
    directory_memory: Option<QueryMemoryReservation>,
}

impl VectorResultCache {
    pub(super) fn get(&self, key: &VectorSearchResultCacheKey) -> Option<&QueryResult> {
        self.entries
            .binary_search_by(|entry| entry.key.cmp(key))
            .ok()
            .map(|index| &self.entries[index].result)
    }

    pub(super) fn insert(
        &mut self,
        admitted: AdmittedVectorResultKey,
        result: &QueryResult,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let version = admitted.key.catalog_version;
        let valid = self
            .entries
            .iter()
            .filter(|entry| entry.key.catalog_version == version);
        let count = valid.clone().count();
        let first = valid.clone().next();
        let evicts_first = count >= 256;
        let replaces = valid.clone().any(|entry| entry.key == admitted.key)
            && (!evicts_first || first.is_none_or(|entry| entry.key != admitted.key));
        let needed = count - usize::from(evicts_first) + usize::from(!replaces);
        let replacement = self.prepare_directory(needed, &admitted.memory, controls)?;
        controls.check_interruption()?;
        if let Some((mut entries, memory)) = replacement {
            entries.append(&mut self.entries);
            let old = std::mem::replace(&mut self.entries, entries);
            drop(old);
            self.directory_memory = Some(memory);
        }
        // Preserve the previous BTreeMap policy, including first-key eviction on replacement.
        self.entries
            .retain(|entry| entry.key.catalog_version == version);
        if self.entries.len() >= 256 {
            self.entries.remove(0);
        }
        match self
            .entries
            .binary_search_by(|entry| entry.key.cmp(&admitted.key))
        {
            Ok(index) => self.entries[index].result = result.clone(),
            Err(index) => self
                .entries
                .insert(index, admitted.into_entry(result.clone())),
        }
        Ok(())
    }

    fn prepare_directory(
        &self,
        needed: usize,
        admission: &QueryMemoryReservation,
        controls: &QueryExecutionControls,
    ) -> Result<Option<(Vec<VectorResultEntry>, QueryMemoryReservation)>, CassieError> {
        if needed <= self.entries.capacity() {
            return Ok(None);
        }
        controls.check_interruption()?;
        let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
            CassieError::ResourceLimit("vector result directory capacity overflow".to_owned())
        })?;
        // Check the exact old/new directory overlap before allocating the fresh backing buffer.
        let _ = checked_add(
            checked_mul(self.entries.capacity(), size_of::<VectorResultEntry>())?,
            checked_mul(capacity, size_of::<VectorResultEntry>())?,
        )?;
        try_reserve_vec(capacity, admission).map(Some)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    #[cfg(test)]
    pub(super) fn contains_key(&self, key: &VectorSearchResultCacheKey) -> bool {
        self.get(key).is_some()
    }

    #[cfg(test)]
    pub(super) fn keys(&self) -> impl Iterator<Item = &VectorSearchResultCacheKey> {
        self.entries.iter().map(|entry| &entry.key)
    }
}
