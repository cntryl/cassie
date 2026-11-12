//! Commit-owned index and cleanup targets for transactional DDL.
use super::{ExpectedStorageValue, Midge, StorageMutation, StorageOverlay};
use crate::app::CassieError;
use crate::catalog::IndexMeta;
use crate::midge::adapter::schema_cleanup::{
    guard_decode, PendingSchemaCleanup, PendingSchemaCleanupAction,
};
use crate::runtime::owned_memory::try_reserve_vec;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

struct IndexEntry<'a> {
    metadata: guard_decode::Decoded<IndexMeta>,
    change: &'a StorageMutation,
}

struct MissingIndexSource {
    collection: String,
    name: String,
    _memory: QueryMemoryReservation,
}

pub(crate) struct IndexCleanupCommitGuard<'a> {
    indexes: Vec<IndexEntry<'a>>,
    markers: Vec<guard_decode::Decoded<PendingSchemaCleanup>>,
    sources: Vec<MissingIndexSource>,
    _index_memory: QueryMemoryReservation,
    _marker_memory: QueryMemoryReservation,
    _source_memory: QueryMemoryReservation,
}

pub(crate) struct IndexCleanupGateNames {
    pub(crate) values: Vec<String>,
    _directory: QueryMemoryReservation,
    _strings: QueryMemoryReservation,
}

impl<'a> IndexCleanupCommitGuard<'a> {
    pub(crate) fn is_empty(&self) -> bool {
        self.indexes.is_empty() && self.markers.is_empty() && self.sources.is_empty()
    }

    pub(crate) fn capture(
        overlay: &'a StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let index_prefix = super::super::key_encoding::index_prefix();
        let cleanup_prefix = super::super::key_encoding::schema_cleanup_prefix();
        let (index_count, marker_count, source_count) =
            count_staged_targets(overlay, &index_prefix, &cleanup_prefix, controls)?;
        let root = controls.reserve_query_memory(0)?;
        let (indexes, index_memory) = try_reserve_vec(index_count, &root)?;
        let (markers, marker_memory) = try_reserve_vec(marker_count, &root)?;
        let (sources, source_memory) = try_reserve_vec(source_count, &root)?;
        let mut plan = Self {
            indexes,
            markers,
            sources,
            _index_memory: index_memory,
            _marker_memory: marker_memory,
            _source_memory: source_memory,
        };
        for (key, change) in &overlay.changes {
            controls.check_interruption()?;
            if key.starts_with(&index_prefix) {
                let raw = match (&change.value, &change.expected) {
                    (Some(raw), _) | (None, ExpectedStorageValue::Present(raw)) => raw,
                    (None, ExpectedStorageValue::Missing) => continue,
                    (None, _) => {
                        return Err(CassieError::Unsupported(
                            "staged index deletion requires a retained metadata preimage"
                                .to_owned(),
                        ))
                    }
                };
                let metadata = guard_decode::decode::<IndexMeta>(
                    raw,
                    Some(controls),
                    "staged index metadata",
                )?;
                if Midge::index_key(&metadata.value.collection, &metadata.value.name) != *key {
                    return Err(CassieError::Unsupported(
                        "staged index metadata has an ambiguous key".to_owned(),
                    ));
                }
                plan.indexes.push(IndexEntry { metadata, change });
            } else if key.starts_with(&cleanup_prefix) {
                let Some(raw) = change.value.as_ref() else {
                    continue;
                };
                let marker = guard_decode::decode::<PendingSchemaCleanup>(
                    raw,
                    Some(controls),
                    "staged schema cleanup",
                )?;
                if matches!(
                    &marker.value.action,
                    PendingSchemaCleanupAction::Index { .. }
                ) {
                    if Midge::schema_cleanup_key(
                        marker.value.collection(),
                        &marker.value.cleanup_id,
                    ) != *key
                    {
                        return Err(CassieError::Unsupported(
                            "staged index cleanup has an ambiguous key".to_owned(),
                        ));
                    }
                    plan.markers.push(marker);
                }
            }
        }
        for (key, change) in &overlay.changes {
            controls.check_interruption()?;
            if !key.starts_with(&index_prefix)
                || change.value.is_some()
                || !matches!(&change.expected, ExpectedStorageValue::Missing)
            {
                continue;
            }
            let source = missing_index_source(&plan.indexes, key, controls)?;
            plan.sources.push(source);
        }
        Ok(plan)
    }

    pub(crate) fn gate_names(
        &self,
        existing: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<IndexCleanupGateNames, CassieError> {
        let names = existing
            .iter()
            .map(String::as_str)
            .chain(
                self.indexes
                    .iter()
                    .map(|entry| entry.metadata.value.collection.as_str()),
            )
            .chain(self.markers.iter().map(|entry| entry.value.collection()))
            .chain(self.sources.iter().map(|source| source.collection.as_str()));
        let mut count = 0_usize;
        let mut bytes = 0_usize;
        for name in names.clone() {
            controls.check_interruption()?;
            count = count.checked_add(1).ok_or_else(|| {
                CassieError::ResourceLimit("index gate count overflow".to_owned())
            })?;
            bytes = bytes
                .checked_add(name.len())
                .ok_or_else(|| CassieError::ResourceLimit("index gate name overflow".to_owned()))?;
        }
        let strings = controls.reserve_query_memory(bytes)?;
        let (values, directory) = try_reserve_vec(count, &strings)?;
        let mut result = IndexCleanupGateNames {
            values,
            _directory: directory,
            _strings: strings,
        };
        for name in names {
            controls.check_interruption()?;
            let mut copy = String::new();
            copy.try_reserve_exact(name.len()).map_err(|error| {
                CassieError::ResourceLimit(format!("index gate name allocation failed: {error}"))
            })?;
            if copy.capacity() > name.len() {
                return Err(CassieError::ResourceLimit(
                    "index gate name exceeded admission".to_owned(),
                ));
            }
            let mut offset = 0;
            while offset < name.len() {
                controls.check_interruption()?;
                let mut end = (offset + 4096).min(name.len());
                while !name.is_char_boundary(end) {
                    end -= 1;
                }
                copy.push_str(&name[offset..end]);
                offset = end;
            }
            result.values.push(copy);
        }
        Ok(result)
    }

    pub(crate) fn validate(
        &self,
        midge: &Midge,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        for entry in &self.indexes {
            controls.check_interruption()?;
            let index = &entry.metadata.value;
            midge.ensure_no_persisted_index_cleanup(&index.collection, &index.name)?;
            if midge.has_persisted_index_publication(&index.collection, &index.name)? {
                return Err(CassieError::Unsupported(
                    "pending index publication prevents transactional index mutation".to_owned(),
                ));
            }
        }
        for source in &self.sources {
            controls.check_interruption()?;
            midge.ensure_no_persisted_index_cleanup(&source.collection, &source.name)?;
            if midge.has_persisted_index_publication(&source.collection, &source.name)? {
                return Err(CassieError::Unsupported(
                    "pending index publication prevents transactional index mutation".to_owned(),
                ));
            }
        }
        for marker in &self.markers {
            controls.check_interruption()?;
            let PendingSchemaCleanupAction::Index { table, index } = &marker.value.action else {
                continue;
            };
            let mut matched = None;
            for entry in &self.indexes {
                controls.check_interruption()?;
                let metadata = &entry.metadata.value;
                if guard_decode::names_match(&metadata.collection, table, Some(controls))?
                    && (guard_decode::names_match(&metadata.name, index, Some(controls))?
                        || guard_decode::names_match(index, &metadata.name, Some(controls))?)
                {
                    if matched.is_some() {
                        return Err(CassieError::Unsupported(
                            "staged index cleanup target is ambiguous".to_owned(),
                        ));
                    }
                    matched = Some(entry);
                }
            }
            let entry = matched.ok_or_else(|| {
                CassieError::Unsupported(
                    "staged index cleanup lost its metadata witness".to_owned(),
                )
            })?;
            match &entry.change.expected {
                ExpectedStorageValue::Missing => {}
                ExpectedStorageValue::Present(expected) => {
                    let actual = entry.change.value.as_deref().ok_or_else(|| {
                        CassieError::Unsupported(
                            "staged index cleanup lost its metadata".to_owned(),
                        )
                    })?;
                    if expected.len() != actual.len() {
                        return Err(CassieError::Unsupported(
                            "staged index cleanup metadata changed".to_owned(),
                        ));
                    }
                    for (left, right) in expected.chunks(4096).zip(actual.chunks(4096)) {
                        controls.check_interruption()?;
                        if left != right {
                            return Err(CassieError::Unsupported(
                                "staged index cleanup metadata changed".to_owned(),
                            ));
                        }
                    }
                }
                ExpectedStorageValue::Untracked => {
                    return Err(CassieError::Unsupported(
                        "staged index cleanup requires a retained metadata preimage".to_owned(),
                    ))
                }
            }
        }
        Ok(())
    }
}

fn copy_name(name: &str, controls: &QueryExecutionControls) -> Result<String, CassieError> {
    let mut output = String::new();
    output.try_reserve_exact(name.len()).map_err(|error| {
        CassieError::ResourceLimit(format!("index source allocation failed: {error}"))
    })?;
    if output.capacity() > name.len() {
        return Err(CassieError::ResourceLimit(
            "index source exceeded admission".to_owned(),
        ));
    }
    let mut offset = 0;
    while offset < name.len() {
        controls.check_interruption()?;
        let mut end = (offset + 4096).min(name.len());
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&name[offset..end]);
        offset = end;
    }
    Ok(output)
}

/// Counts staged index rows, index cleanup markers, and missing index sources.
fn count_staged_targets(
    overlay: &StorageOverlay,
    index_prefix: &[u8],
    cleanup_prefix: &[u8],
    controls: &QueryExecutionControls,
) -> Result<(usize, usize, usize), CassieError> {
    let mut index_count = 0_usize;
    let mut marker_count = 0_usize;
    let mut source_count = 0_usize;
    for (key, change) in &overlay.changes {
        controls.check_interruption()?;
        if key.starts_with(index_prefix) {
            index_count += 1;
            if change.value.is_none() && matches!(&change.expected, ExpectedStorageValue::Missing) {
                source_count += 1;
            }
        }
        if change.value.is_some() && key.starts_with(cleanup_prefix) {
            marker_count += 1;
        }
    }
    Ok((index_count, marker_count, source_count))
}

/// Resolves the retained collection witness for a staged index-source deletion.
fn missing_index_source(
    indexes: &[IndexEntry<'_>],
    key: &[u8],
    controls: &QueryExecutionControls,
) -> Result<MissingIndexSource, CassieError> {
    let mut found: Option<(&str, &[u8])> = None;
    for entry in indexes {
        controls.check_interruption()?;
        let collection = entry.metadata.value.collection.as_str();
        let prefix = Midge::index_collection_prefix(collection);
        if let Some(suffix) = key.strip_prefix(prefix.as_slice()) {
            if found.is_some_and(|(old, _)| old != collection) {
                return Err(CassieError::Unsupported(
                    "staged index source collection is ambiguous".to_owned(),
                ));
            }
            found = Some((collection, suffix));
        }
    }
    let (collection, suffix) = found.ok_or_else(|| {
        CassieError::Unsupported(
            "staged index source has no retained collection witness".to_owned(),
        )
    })?;
    controls.check_interruption()?;
    let name = std::str::from_utf8(suffix).map_err(|_| {
        CassieError::Unsupported("staged index source name is not UTF-8".to_owned())
    })?;
    if name.is_empty() || suffix.contains(&0) || Midge::index_key(collection, name) != key {
        return Err(CassieError::Unsupported(
            "staged index source key is ambiguous".to_owned(),
        ));
    }
    let bytes = collection.len().checked_add(name.len()).ok_or_else(|| {
        CassieError::ResourceLimit("staged index source size overflow".to_owned())
    })?;
    let memory = controls.reserve_query_memory(bytes)?;
    let collection_copy = copy_name(collection, controls)?;
    let name_copy = copy_name(name, controls)?;
    Ok(MissingIndexSource {
        collection: collection_copy,
        name: name_copy,
        _memory: memory,
    })
}
