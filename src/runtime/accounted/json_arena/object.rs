//! Private stable-index AVL object directory; no parser or production caller yet.
use std::cmp::Ordering;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::cursor::string::DecodedText;
use super::directory::Directory;

#[cfg(test)]
#[path = "object_tests.rs"]
mod tests;

pub(super) struct Key {
    text: String,
    _memory: QueryMemoryReservation,
}

impl Key {
    pub(super) fn adopt(
        token: DecodedText,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        if !token.shares_tracker(controls) {
            return Err(CassieError::Execution(
                "JSON key token uses a different query tracker".into(),
            ));
        }
        let (text, memory) = token.into_parts();
        Ok(Self {
            text,
            _memory: memory,
        })
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub(super) fn new(text: &str, controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let memory = controls.reserve_query_memory(text.len())?;
        let mut copy = String::new();
        copy.try_reserve_exact(text.len())
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if copy.capacity() > text.len() {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted JSON key capacity".into(),
            ));
        }
        let mut start = 0;
        while start < text.len() {
            controls.check_interruption()?;
            let mut end = start.saturating_add(1024).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            copy.push_str(&text[start..end]);
            start = end;
        }
        controls.check_interruption()?;
        Ok(Self {
            text: copy,
            _memory: memory,
        })
    }
}

#[derive(Clone, Copy)]
enum Progress {
    Compare,
    Move,
    ChunkPublished,
}

struct Entry<V> {
    key: Key,
    value: V,
    left: Option<usize>,
    right: Option<usize>,
    height: usize,
}

const CHUNK_SIZE: usize = 4;

struct EntryChunk<V> {
    entries: Vec<Option<Entry<V>>>,
    _memory: QueryMemoryReservation,
}

pub(super) struct ObjectIndex<V> {
    entries: Directory<EntryChunk<V>>,
    length: usize,
    controls: QueryExecutionControls,
    root: Option<usize>,
    poisoned: bool,
}

struct Path {
    values: Vec<usize>,
    memory: QueryMemoryReservation,
}

impl Path {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn push(&mut self, value: usize, controls: &QueryExecutionControls) -> Result<(), CassieError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .checked_add(1)
                .and_then(|n| n.max(4).checked_next_power_of_two())
                .ok_or_else(|| CassieError::ResourceLimit("JSON object path overflow".into()))?;
            let replacement_memory;
            let mut replacement;
            (replacement, replacement_memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            for (index, existing) in self.values.iter().copied().enumerate() {
                if index.is_multiple_of(128) {
                    controls.check_interruption()?;
                }
                replacement.push(existing);
            }
            let old = std::mem::replace(&mut self.values, replacement);
            let old_memory = std::mem::replace(&mut self.memory, replacement_memory);
            drop(old);
            drop(old_memory);
        }
        self.values.push(value);
        Ok(())
    }
}

impl<V: Copy> ObjectIndex<V> {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            entries: Directory::new(controls)?,
            length: 0,
            controls: controls.clone(),
            root: None,
            poisoned: false,
        })
    }

    #[cfg(test)]
    fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub(super) fn insert(&mut self, key: Key, value: V) -> Result<Option<V>, CassieError> {
        self.insert_with_observer(key, value, |_| Ok(()))
    }

    pub(super) fn find(&self, key: &str) -> Result<Option<V>, CassieError> {
        let mut current = self.root;
        while let Some(index) = current {
            self.controls.check_interruption()?;
            let entry = self.entry(index);
            match compare(key, &entry.key.text, &self.controls, &mut |_| Ok(()))? {
                Ordering::Less => current = entry.left,
                Ordering::Greater => current = entry.right,
                Ordering::Equal => return Ok(Some(entry.value)),
            }
        }
        self.controls.check_interruption()?;
        Ok(None)
    }

    pub(super) const fn values_len(&self) -> usize {
        self.length
    }

    pub(super) fn value_at(&self, index: usize) -> V {
        self.entry(index).value
    }

    pub(super) fn first_index(&self) -> Result<Option<usize>, CassieError> {
        self.minimum(self.root)
    }

    pub(super) fn next_index(
        &self,
        index: usize,
        observer: &mut impl FnMut(),
    ) -> Result<Option<usize>, CassieError> {
        let entry = self.entry(index);
        if entry.right.is_some() {
            return self.minimum(entry.right);
        }
        let key = entry.key.text.as_str();
        let mut current = self.root;
        let mut candidate = None;
        while let Some(index) = current {
            self.controls.check_interruption()?;
            let entry = self.entry(index);
            match compare(key, &entry.key.text, &self.controls, &mut |_| {
                observer();
                Ok(())
            })? {
                Ordering::Less => {
                    candidate = Some(index);
                    current = entry.left;
                }
                Ordering::Greater => current = entry.right,
                Ordering::Equal => break,
            }
        }
        Ok(candidate)
    }

    pub(super) fn item(&self, index: usize) -> (&str, V) {
        let entry = self.entry(index);
        (&entry.key.text, entry.value)
    }

    fn minimum(&self, mut current: Option<usize>) -> Result<Option<usize>, CassieError> {
        while let Some(index) = current {
            self.controls.check_interruption()?;
            let left = self.entry(index).left;
            if left.is_none() {
                return Ok(Some(index));
            }
            current = left;
        }
        Ok(None)
    }

    #[cfg(test)]
    fn insert_observed(
        &mut self,
        key: Key,
        value: V,
        observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<Option<V>, CassieError> {
        self.insert_with_observer(key, value, observer)
    }

    fn insert_with_observer(
        &mut self,
        key: Key,
        value: V,
        mut observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<Option<V>, CassieError> {
        if self.poisoned {
            return Err(CassieError::Execution(
                "JSON object directory is poisoned".into(),
            ));
        }
        self.controls.check_interruption()?;
        if !key._memory.shares_tracker(&self.controls) {
            return Err(CassieError::Execution(
                "JSON object key uses a different query tracker".into(),
            ));
        }
        let mut path = Path::new(&self.controls)?;
        let mut current = self.root;
        let mut insert_left = false;
        while let Some(index) = current {
            let entry = self.entry(index);
            let order = compare(&key.text, &entry.key.text, &self.controls, &mut observer)?;
            if order == Ordering::Equal {
                self.controls.check_interruption()?;
                let previous = std::mem::replace(&mut self.entry_mut(index).value, value);
                return Ok(Some(previous));
            }
            path.push(index, &self.controls)?;
            insert_left = order == Ordering::Less;
            current = if insert_left { entry.left } else { entry.right };
        }
        let index = self.length;
        let next_length = index
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON object entry count overflow".into()))?;
        self.grow_entries(&mut observer)?;
        self.controls.check_interruption()?;
        let chunk = self
            .entries
            .get_mut(index / CHUNK_SIZE)
            .expect("entry chunk");
        chunk.entries[index % CHUNK_SIZE] = Some(Entry {
            key,
            value,
            left: None,
            right: None,
            height: 1,
        });
        self.length = next_length;
        if let Some(parent) = path.values.last().copied() {
            if insert_left {
                self.entry_mut(parent).left = Some(index);
            } else {
                self.entry_mut(parent).right = Some(index);
            }
        } else {
            self.root = Some(index);
        }
        // An AVL tree over at most usize::MAX slots has height below twice
        // the machine word width. All fallible work precedes this bounded commit.
        for position in (0..path.values.len()).rev() {
            let old = path.values[position];
            let new = self.balance(old);
            if position == 0 {
                self.root = Some(new);
            } else {
                let parent = path.values[position - 1];
                if self.entry(parent).left == Some(old) {
                    self.entry_mut(parent).left = Some(new);
                } else {
                    self.entry_mut(parent).right = Some(new);
                }
            }
        }
        Ok(None)
    }

    fn grow_entries(
        &mut self,
        observer: &mut impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        if !self.length.is_multiple_of(CHUNK_SIZE) {
            return Ok(());
        }
        if self.entries.get(self.length / CHUNK_SIZE).is_some() {
            return Ok(());
        }
        let admission = self.controls.reserve_query_memory(0)?;
        let memory;
        let mut entries;
        (entries, memory) = crate::runtime::owned_memory::try_reserve_vec(CHUNK_SIZE, &admission)?;
        for _ in 0..CHUNK_SIZE {
            self.controls.check_interruption()?;
            entries.push(None);
        }
        self.entries.push_observed(
            EntryChunk {
                entries,
                _memory: memory,
            },
            |progress| {
                if progress.is_moved() {
                    observer(Progress::Move)?;
                }
                Ok(())
            },
        )?;
        observer(Progress::ChunkPublished)?;
        self.controls.check_interruption()
    }

    fn entry(&self, index: usize) -> &Entry<V> {
        self.entries
            .get(index / CHUNK_SIZE)
            .expect("entry chunk")
            .entries[index % CHUNK_SIZE]
            .as_ref()
            .expect("valid AVL entry")
    }
    fn entry_mut(&mut self, index: usize) -> &mut Entry<V> {
        self.entries
            .get_mut(index / CHUNK_SIZE)
            .expect("entry chunk")
            .entries[index % CHUNK_SIZE]
            .as_mut()
            .expect("valid AVL entry")
    }
    fn height(&self, index: Option<usize>) -> usize {
        index.map_or(0, |index| self.entry(index).height)
    }
    fn update_height(&mut self, index: usize) {
        let entry = self.entry(index);
        let height = 1 + self.height(entry.left).max(self.height(entry.right));
        self.entry_mut(index).height = height;
    }
    fn rotate_left(&mut self, index: usize) -> usize {
        let right = self.entry(index).right.expect("right child");
        let middle = self.entry(right).left;
        self.entry_mut(index).right = middle;
        self.entry_mut(right).left = Some(index);
        self.update_height(index);
        self.update_height(right);
        right
    }
    fn rotate_right(&mut self, index: usize) -> usize {
        let left = self.entry(index).left.expect("left child");
        let middle = self.entry(left).right;
        self.entry_mut(index).left = middle;
        self.entry_mut(left).right = Some(index);
        self.update_height(index);
        self.update_height(left);
        left
    }
    fn balance(&mut self, index: usize) -> usize {
        self.update_height(index);
        let entry = self.entry(index);
        let (left, right) = (entry.left, entry.right);
        if self.height(left) > self.height(right) + 1 {
            let child = left.expect("left child");
            if self.height(self.entry(child).left) < self.height(self.entry(child).right) {
                let rotated = self.rotate_left(child);
                self.entry_mut(index).left = Some(rotated);
            }
            self.rotate_right(index)
        } else if self.height(right) > self.height(left) + 1 {
            let child = right.expect("right child");
            if self.height(self.entry(child).right) < self.height(self.entry(child).left) {
                let rotated = self.rotate_right(child);
                self.entry_mut(index).right = Some(rotated);
            }
            self.rotate_left(index)
        } else {
            index
        }
    }
}

fn compare(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(Progress) -> Result<(), CassieError>,
) -> Result<Ordering, CassieError> {
    for (left, right) in left
        .as_bytes()
        .chunks(1024)
        .zip(right.as_bytes().chunks(1024))
    {
        controls.check_interruption()?;
        let order = left.cmp(right);
        observer(Progress::Compare)?;
        controls.check_interruption()?;
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(left.len().cmp(&right.len()))
}

#[cfg(test)]
impl<V: Copy> ObjectIndex<V> {
    fn test_order(&self) -> Vec<&str> {
        fn walk<'a, V: Copy>(
            object: &'a ObjectIndex<V>,
            index: Option<usize>,
            values: &mut Vec<&'a str>,
        ) {
            if let Some(index) = index {
                let entry = object.entry(index);
                walk(object, entry.left, values);
                values.push(&entry.key.text);
                walk(object, entry.right, values);
            }
        }
        let mut values = Vec::new();
        walk(self, self.root, &mut values);
        values
    }
    fn test_value(&self, key: &str) -> Option<V> {
        let mut node = self.root;
        while let Some(index) = node {
            let entry = self.entry(index);
            match key.cmp(&entry.key.text) {
                Ordering::Less => node = entry.left,
                Ordering::Greater => node = entry.right,
                Ordering::Equal => return Some(entry.value),
            }
        }
        None
    }
    fn test_invariants(&self) -> bool {
        fn check<V: Copy>(object: &ObjectIndex<V>, index: Option<usize>) -> Option<usize> {
            let Some(index) = index else {
                return Some(0);
            };
            let entry = object.entry(index);
            let left = check(object, entry.left)?;
            let right = check(object, entry.right)?;
            (left.abs_diff(right) <= 1 && entry.height == 1 + left.max(right))
                .then_some(entry.height)
        }
        let values = self.test_order();
        values.windows(2).all(|pair| pair[0] < pair[1]) && check(self, self.root).is_some()
    }
}
