//! Immutable fingerprint indexes whose values retain leased semantic records.
use std::collections::{hash_map::RandomState, HashMap};
use std::hash::{BuildHasher, Hash};
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

mod entry;
mod ordered;
pub(crate) use entry::Entry;
use entry::{OccupiedEntry, VacantEntry};
pub(crate) use ordered::OrderedEntries;

#[derive(Debug)]
struct Bucket<K, V> {
    values: Vec<(K, V)>,
    memory: Option<QueryMemoryReservation>,
}

#[derive(Debug)]
pub(crate) struct KeyedMap<K, V> {
    buckets: HashMap<u64, Bucket<K, V>>,
    memory: Option<QueryMemoryReservation>,
    prepaid: Option<(usize, QueryMemoryReservation)>,
    hasher: RandomState,
    length: usize,
}

impl<K, V> Default for KeyedMap<K, V> {
    fn default() -> Self {
        Self {
            buckets: HashMap::new(),
            memory: None,
            prepaid: None,
            hasher: RandomState::new(),
            length: 0,
        }
    }
}

impl<K: Eq + Hash, V> KeyedMap<K, V> {
    pub(crate) fn new(controls: Option<&QueryExecutionControls>) -> Result<Self, CassieError> {
        Ok(Self {
            buckets: HashMap::new(),
            memory: controls
                .map(|controls| controls.reserve_query_memory(0))
                .transpose()?,
            prepaid: None,
            hasher: RandomState::new(),
            length: 0,
        })
    }

    /// Admits the entire bounded build before callers construct their first key.
    pub(crate) fn with_prepaid_capacity(
        controls: &QueryExecutionControls,
        capacity: usize,
    ) -> Result<Self, CassieError> {
        let mut result = Self {
            prepaid: Some((
                capacity,
                controls.reserve_query_memory(Self::build_storage_bytes(capacity)?)?,
            )),
            ..Self::default()
        };
        result.try_reserve(capacity)?;
        Ok(result)
    }

    /// Full fingerprint table plus collision directories, including replacement overlap.
    pub(crate) fn build_storage_bytes(capacity: usize) -> Result<usize, CassieError> {
        add(
            hash_table_bytes::<(u64, Bucket<K, V>)>(capacity)?,
            mul(mul(capacity, 3)?, size_of::<(K, V)>())?,
        )
    }

    pub(crate) fn len(&self) -> usize {
        self.length
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.length == 0
    }
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.buckets.capacity()
    }

    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), CassieError> {
        let needed = add(self.buckets.len(), additional)?;
        if self
            .prepaid
            .as_ref()
            .is_some_and(|(limit, _)| needed > *limit)
        {
            return Err(CassieError::ResourceLimit(
                "semantic build capacity exhausted".into(),
            ));
        }
        if needed <= self.buckets.capacity() {
            return Ok(());
        }
        let bound = hash_table_bytes::<(u64, Bucket<K, V>)>(needed)?;
        let replacement_memory = self
            .memory
            .as_ref()
            .map(|memory| memory.try_sibling(bound))
            .transpose()?;
        let mut replacement = HashMap::new();
        replacement
            .try_reserve(needed)
            .map_err(|error| allocation(&error))?;
        if hash_table_bytes::<(u64, Bucket<K, V>)>(replacement.capacity())? > bound {
            return Err(CassieError::ResourceLimit(
                "semantic index allocator exceeded admission".into(),
            ));
        }
        replacement.extend(self.buckets.drain());
        let old = std::mem::replace(&mut self.buckets, replacement);
        drop(old);
        self.memory = replacement_memory;
        Ok(())
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.buckets
            .get(&self.hasher.hash_one(key))?
            .values
            .iter()
            .find_map(|(stored, value)| (stored == key).then_some(value))
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.buckets
            .get_mut(&self.hasher.hash_one(key))?
            .values
            .iter_mut()
            .find_map(|(stored, value)| (stored == key).then_some(value))
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let bucket = self.buckets.get_mut(&self.hasher.hash_one(key))?;
        let position = bucket.values.iter().position(|(stored, _)| stored == key)?;
        self.length -= 1;
        Some(bucket.values.swap_remove(position).1)
    }

    #[cfg(test)]
    pub(crate) fn index_storage_bytes(&self) -> usize {
        self.memory
            .as_ref()
            .map_or(0, QueryMemoryReservation::bytes)
            + self
                .buckets
                .values()
                .map(|bucket| {
                    bucket
                        .memory
                        .as_ref()
                        .map_or(0, QueryMemoryReservation::bytes)
                })
                .sum::<usize>()
    }

    pub(crate) fn entry(&mut self, key: K) -> Result<Entry<'_, K, V>, CassieError> {
        let hash = self.hasher.hash_one(&key);
        let position = self
            .buckets
            .get(&hash)
            .and_then(|bucket| bucket.values.iter().position(|(stored, _)| stored == &key));
        if let Some(position) = position {
            return Ok(Entry::Occupied(OccupiedEntry {
                value: &mut self.buckets.get_mut(&hash).expect("located bucket").values[position],
            }));
        }
        if self
            .prepaid
            .as_ref()
            .is_some_and(|(limit, _)| self.length >= *limit)
        {
            return Err(CassieError::ResourceLimit(
                "semantic build capacity exhausted".into(),
            ));
        }
        self.length
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("semantic index length overflow".into()))?;
        if !self.buckets.contains_key(&hash) {
            self.try_reserve(1)?;
            let memory = self
                .memory
                .as_ref()
                .map(|memory| memory.try_sibling(0))
                .transpose()?;
            self.buckets.insert(
                hash,
                Bucket {
                    values: Vec::new(),
                    memory,
                },
            );
        }
        let bucket = self.buckets.get_mut(&hash).expect("admitted bucket");
        grow(&mut bucket.values, 1, bucket.memory.as_mut())?;
        Ok(Entry::Vacant(VacantEntry {
            bucket,
            key,
            length: &mut self.length,
        }))
    }

    pub(crate) fn insert(&mut self, key: K, value: V) -> Result<Option<V>, CassieError> {
        match self.entry(key)? {
            Entry::Occupied(mut entry) => Ok(Some(std::mem::replace(entry.get_mut(), value))),
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(None)
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.buckets
            .values()
            .flat_map(|bucket| bucket.values.iter())
            .map(|(key, value)| (key, value))
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.buckets
            .values_mut()
            .flat_map(|bucket| bucket.values.iter_mut())
            .map(|(_, value)| value)
    }

    pub(crate) fn into_sorted_entries(mut self) -> Result<OrderedEntries<K, V>, CassieError>
    where
        K: Ord,
    {
        let memory = self
            .memory
            .as_ref()
            .or_else(|| self.prepaid.as_ref().map(|(_, memory)| memory))
            .map(|memory| memory.try_sibling(mul(self.length, size_of::<(K, V)>())?))
            .transpose()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.length)
            .map_err(|error| allocation(&error))?;
        if values.capacity() > self.length && size_of::<(K, V)>() != 0 {
            return Err(CassieError::ResourceLimit(
                "semantic drain allocator exceeded admission".into(),
            ));
        }
        for (_, mut bucket) in self.buckets.drain() {
            values.append(&mut bucket.values);
        }
        // The in-place unstable sort uses no heap scratch and preserves key ordering.
        values.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        Ok(OrderedEntries { values, memory })
    }
}

#[derive(Debug, Default)]
pub(crate) struct KeyedSet<K> {
    map: KeyedMap<K, ()>,
}
impl<K: Eq + Hash> KeyedSet<K> {
    pub(crate) fn new(controls: Option<&QueryExecutionControls>) -> Result<Self, CassieError> {
        Ok(Self {
            map: KeyedMap::new(controls)?,
        })
    }
    pub(crate) fn insert(&mut self, key: K) -> Result<bool, CassieError> {
        self.map.insert(key, ()).map(|old| old.is_none())
    }
    pub(crate) fn contains(&self, key: &K) -> bool {
        self.map.get(key).is_some()
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.map.capacity()
    }
    #[cfg(test)]
    pub(crate) fn index_storage_bytes(&self) -> usize {
        self.map.index_storage_bytes()
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &K> {
        self.map.iter().map(|(key, ())| key)
    }
    pub(crate) fn try_from_iter(
        values: impl IntoIterator<Item = K>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, CassieError> {
        let mut result = Self::new(controls)?;
        for value in values {
            result.insert(value)?;
        }
        Ok(result)
    }
}

fn grow<T>(
    values: &mut Vec<T>,
    additional: usize,
    memory: Option<&mut QueryMemoryReservation>,
) -> Result<(), CassieError> {
    if let Some(memory) = memory {
        let needed = add(values.len(), additional)?;
        if needed <= values.capacity() {
            return Ok(());
        }
        let capacity = needed
            .checked_next_power_of_two()
            .ok_or_else(|| CassieError::ResourceLimit("semantic directory overflow".into()))?;
        let old_bytes = mul(values.capacity(), size_of::<T>())?;
        let (mut replacement, replacement_memory) =
            crate::runtime::owned_memory::try_reserve_vec(capacity, memory)?;
        replacement.append(values);
        let old = std::mem::replace(values, replacement);
        drop(old);
        memory.shrink_to(memory.bytes() - old_bytes);
        memory.absorb(replacement_memory);
        return Ok(());
    }
    let needed = add(values.len(), additional)?;
    if needed > values.capacity() {
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(needed)
            .map_err(|error| allocation(&error))?;
        if replacement.capacity() > needed && size_of::<T>() != 0 {
            return Err(CassieError::ResourceLimit(
                "semantic directory exceeded admission".into(),
            ));
        }
        replacement.append(values);
        *values = replacement;
    }
    Ok(())
}
fn allocation(error: &std::collections::TryReserveError) -> CassieError {
    crate::runtime::owned_memory::allocation_error(error)
}

#[cfg(test)]
mod tests;
