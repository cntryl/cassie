//! Transaction-owned cursors for sequence objects that have not been published.

use super::{
    Arc, CassieError, ExpectedStorageValue, Midge, Mutex, StorageOverlay, ACTIVE_STORAGE_SCOPE,
};
use crate::catalog::{SequenceCursor, SequenceMeta};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

// Covers a B-tree node even when a split temporarily retains old and new nodes.
const CURSOR_BYTES: usize = 2048;

#[derive(Debug, Default)]
pub(crate) struct SequenceOwner {
    state: Mutex<CursorState>,
}

#[derive(Debug, Default)]
struct CursorState {
    cursors: BTreeMap<(u64, u64), SequenceCursor>,
    next_epoch: u64,
    inflight: BTreeMap<(u64, u64), usize>,
    admission: Option<QueryMemoryReservation>,
}

#[derive(Debug)]
pub(crate) struct SequenceForkLease {
    owner: Arc<SequenceOwner>,
    pairs: Mutex<BTreeMap<(u64, u64), QueryMemoryReservation>>,
}

impl SequenceForkLease {
    fn protect(
        &self,
        state: &mut CursorState,
        key: (u64, u64),
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let mut pairs = self.pairs.lock();
        if pairs.contains_key(&key) {
            return Ok(());
        }
        controls.check_interruption()?;
        let bytes = CURSOR_BYTES.checked_mul(2).ok_or_else(|| {
            CassieError::ResourceLimit("sequence fork accounting overflow".into())
        })?;
        let _request = controls.reserve_query_memory(bytes)?;
        if state.admission.is_none() {
            state.admission = Some(QueryMemoryReservation::independent_budget(
                controls.query_memory_budget_bytes,
                0,
            )?);
        }
        let memory = state
            .admission
            .as_ref()
            .expect("owner admitted")
            .try_sibling(bytes)?;
        let count = state
            .inflight
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("sequence fork count overflow".into()))?;
        state.inflight.insert(key, count);
        pairs.insert(key, memory);
        Ok(())
    }
}

impl Drop for SequenceForkLease {
    fn drop(&mut self) {
        let pairs = self.pairs.get_mut();
        let mut state = self.owner.state.lock();
        for key in pairs.keys() {
            if let Some(count) = state.inflight.get_mut(key) {
                *count -= 1;
                if *count == 0 {
                    state.inflight.remove(key);
                }
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct EpochSelections {
    pub(super) epochs: BTreeMap<u64, u64>,
    _memory: QueryMemoryReservation,
}

impl StorageOverlay {
    fn sequence_epoch(&self, identity: u64) -> u64 {
        self.sequence_epochs
            .as_ref()
            .and_then(|owner| owner.epochs.get(&identity))
            .copied()
            .unwrap_or(0)
    }
}

pub(crate) enum SequenceTarget {
    Private {
        metadata: SequenceMeta,
        owner: Arc<SequenceOwner>,
        overlay: Arc<Mutex<StorageOverlay>>,
        epoch: u64,
        scratch: QueryMemoryReservation,
    },
    Native,
}

fn controls() -> Result<QueryExecutionControls, CassieError> {
    super::super::publication::PublicationControlScope::current_query_controls()
        .ok_or_else(|| CassieError::Unsupported("staged sequence requires query controls".into()))
}

struct CheckingReader<'a> {
    bytes: &'a [u8],
    position: usize,
    controls: &'a QueryExecutionControls,
}

impl Read for CheckingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.position.is_multiple_of(4096) {
            self.controls
                .check_interruption()
                .map_err(|_| io::Error::other("query interrupted"))?;
        }
        let until_check = 4096 - self.position % 4096;
        let remaining = &self.bytes[self.position..];
        let count = remaining.len().min(output.len()).min(until_check);
        output[..count].copy_from_slice(&remaining[..count]);
        self.position += count;
        Ok(count)
    }
}

fn decode(
    raw: &[u8],
    controls: &QueryExecutionControls,
) -> Result<(SequenceMeta, QueryMemoryReservation), CassieError> {
    controls.check_interruption()?;
    let memory = controls.reserve_query_memory(
        crate::runtime::accounted::json::serialized_decode_bytes(raw.len())?,
    )?;
    let reader = CheckingReader {
        bytes: raw,
        position: 0,
        controls,
    };
    let metadata = match serde_json::from_reader(reader) {
        Ok(metadata) => metadata,
        Err(error) => {
            controls.check_interruption()?;
            return Err(CassieError::Parse(format!(
                "invalid staged sequence metadata: {error}"
            )));
        }
    };
    Ok((metadata, memory))
}

struct Counter<'a> {
    position: usize,
    controls: &'a QueryExecutionControls,
}
impl Write for Counter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.controls
            .check_interruption()
            .map_err(|_| io::Error::other("query interrupted"))?;
        let count = bytes.len().min(4096);
        self.position = self
            .position
            .checked_add(count)
            .ok_or_else(|| io::Error::other("sequence serialization overflow"))?;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct CheckingWriter<'a> {
    output: &'a mut Vec<u8>,
    position: usize,
    controls: &'a QueryExecutionControls,
}
impl Write for CheckingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.controls
            .check_interruption()
            .map_err(|_| io::Error::other("query interrupted"))?;
        let count = bytes.len().min(4096);
        let end = self
            .output
            .len()
            .checked_add(count)
            .ok_or_else(|| io::Error::other("sequence serialization overflow"))?;
        if end > self.output.capacity() {
            return Err(io::Error::other("sequence output exceeded admission"));
        }
        self.output.extend_from_slice(&bytes[..count]);
        self.position += count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Emit JSON string fragments in bounded scans. `serde_json`'s ordinary string
/// serializer scans a complete unescaped string before calling its writer.
fn write_json_string(
    writer: &mut impl Write,
    value: &str,
    controls: &QueryExecutionControls,
) -> io::Result<()> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    writer.write_all(b"\"")?;
    for chunk in value.as_bytes().chunks(4096) {
        controls
            .check_interruption()
            .map_err(|_| io::Error::other("query interrupted"))?;
        let mut start = 0;
        for (index, byte) in chunk.iter().copied().enumerate() {
            let escape: Option<&[u8]> = match byte {
                b'"' => Some(b"\\\""),
                b'\\' => Some(b"\\\\"),
                8 => Some(b"\\b"),
                9 => Some(b"\\t"),
                10 => Some(b"\\n"),
                12 => Some(b"\\f"),
                13 => Some(b"\\r"),
                _ => None,
            };
            if let Some(escape) = escape {
                writer.write_all(&chunk[start..index])?;
                writer.write_all(escape)?;
                start = index + 1;
            } else if byte < 32 {
                writer.write_all(&chunk[start..index])?;
                writer.write_all(&[
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 15)],
                ])?;
                start = index + 1;
            }
        }
        writer.write_all(&chunk[start..])?;
    }
    writer.write_all(b"\"")
}

/// Preserve the existing serde JSON representation while bounding every scan.
fn encode_sequence_metadata(
    writer: &mut impl Write,
    metadata: &SequenceMeta,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    use crate::catalog::SequencePersistence;
    use crate::types::DataType;
    let SequenceMeta {
        name: _,
        data_type: _,
        start_value: _,
        increment_by: _,
        current_value: _,
        is_called: _,
        min_value: _,
        max_value: _,
        cache: _,
        cycle: _,
        owned_by: _,
        persistence: _,
        object_id: _,
    } = metadata;
    let data_type = match &metadata.data_type {
        DataType::SmallInt => "smallint",
        DataType::Int => "int",
        DataType::BigInt => "bigint",
        _ => {
            return Err(CassieError::Unsupported(
                "unsupported transaction-owned sequence type".into(),
            ))
        }
    };
    let persistence = match metadata.persistence {
        SequencePersistence::Permanent => "permanent",
        SequencePersistence::Unlogged => "unlogged",
        SequencePersistence::Temporary => "temporary",
    };
    let mut encode = || -> io::Result<()> {
        writer.write_all(b"{\"name\":")?;
        write_json_string(writer, &metadata.name, controls)?;
        write!(writer, ",\"data_type\":\"{data_type}\",\"start_value\":{},\"increment_by\":{},\"current_value\":{},\"is_called\":{},\"min_value\":{},\"max_value\":{},\"cache\":{},\"cycle\":{},\"owned_by\":", metadata.start_value, metadata.increment_by, metadata.current_value, metadata.is_called, metadata.min_value, metadata.max_value, metadata.cache, metadata.cycle)?;
        if let Some(owner) = &metadata.owned_by {
            let crate::catalog::SequenceOwner {
                table: _,
                column: _,
            } = owner;
            writer.write_all(b"{\"table\":")?;
            write_json_string(writer, &owner.table, controls)?;
            writer.write_all(b",\"column\":")?;
            write_json_string(writer, &owner.column, controls)?;
            writer.write_all(b"}")?;
        } else {
            writer.write_all(b"null")?;
        }
        write!(
            writer,
            ",\"persistence\":\"{persistence}\",\"object_id\":{}}}",
            metadata.object_id
        )
    };
    match encode() {
        Ok(()) => Ok(()),
        Err(error) => {
            controls.check_interruption()?;
            Err(CassieError::Parse(format!(
                "sequence serialization failed: {error}"
            )))
        }
    }
}

fn encode_metadata(
    metadata: &SequenceMeta,
    controls: &QueryExecutionControls,
) -> Result<(Vec<u8>, QueryMemoryReservation), CassieError> {
    let mut counter = Counter {
        position: 0,
        controls,
    };
    encode_sequence_metadata(&mut counter, metadata, controls)?;
    let base = controls.reserve_query_memory(0)?;
    let (mut encoded, memory) =
        crate::runtime::owned_memory::try_reserve_vec(counter.position, &base)?;
    {
        let mut writer = CheckingWriter {
            output: &mut encoded,
            position: 0,
            controls,
        };
        encode_sequence_metadata(&mut writer, metadata, controls)?;
    }
    if encoded.len() != counter.position {
        return Err(CassieError::Execution(
            "sequence serialization changed size".into(),
        ));
    }
    Ok((encoded, memory))
}

impl SequenceOwner {
    pub(crate) fn advance<T>(
        &self,
        mut metadata: SequenceMeta,
        epoch: u64,
        fork: Option<&SequenceForkLease>,
        update: impl FnOnce(&mut SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, SequenceMeta), CassieError> {
        let mut state = self.state.lock();
        if let Some(cursor) = state.cursors.get(&(metadata.object_id, epoch)) {
            metadata.set_cursor(*cursor);
        }
        let controls = super::super::publication::PublicationControlScope::current_query_controls()
            .ok_or_else(|| {
                CassieError::Unsupported("staged sequence requires query controls".into())
            })?;
        controls.check_interruption()?;
        let is_new = !state.cursors.contains_key(&(metadata.object_id, epoch));
        if is_new {
            // Request admission remains separate from the transaction-owned aggregate bound.
            let _request = controls.reserve_query_memory(CURSOR_BYTES)?;
            if state.admission.is_none() {
                state.admission = Some(QueryMemoryReservation::independent_budget(
                    controls.query_memory_budget_bytes,
                    0,
                )?);
            }
            state
                .admission
                .as_mut()
                .expect("owner admitted")
                .try_grow(CURSOR_BYTES)?;
        }
        let result = match update(&mut metadata) {
            Ok(result) => result,
            Err(error) => {
                if is_new {
                    let admission = state.admission.as_mut().expect("owner admitted");
                    admission.shrink_to(admission.bytes() - CURSOR_BYTES);
                }
                return Err(error);
            }
        };
        if let Some(fork) = fork {
            if let Err(error) = fork.protect(&mut state, (metadata.object_id, epoch), &controls) {
                if is_new {
                    let admission = state.admission.as_mut().expect("owner admitted");
                    admission.shrink_to(admission.bytes() - CURSOR_BYTES);
                }
                return Err(error);
            }
        }
        state
            .cursors
            .insert((metadata.object_id, epoch), metadata.cursor());
        Ok((result, metadata))
    }
}

impl Midge {
    /// Whether a catalog mutation is currently staged in this database family.
    pub(crate) fn private_sequence_catalog_scope(&self) -> bool {
        ACTIVE_STORAGE_SCOPE.with(|current| {
            current.borrow().as_ref().is_some_and(|scope| {
                scope.midge_identity == std::ptr::from_ref(self) as usize && scope.stage_writes
            })
        })
    }

    pub(crate) fn update_private_sequence_transactionally<T>(
        &self,
        name: &str,
        expected_identity: u64,
        expected_epoch: u64,
        update: impl FnOnce(&mut SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, SequenceMeta), CassieError> {
        let scope = ACTIVE_STORAGE_SCOPE
            .with(|current| current.borrow().clone())
            .filter(|scope| {
                scope.midge_identity == std::ptr::from_ref(self) as usize && scope.stage_writes
            })
            .ok_or_else(|| {
                CassieError::Unsupported(
                    "transaction-owned sequence update lost its storage scope".into(),
                )
            })?;
        let controls = controls()?;
        let key = super::super::key_encoding::sequence_key(&self.canonical_collection_name(name));
        let mut overlay = scope.overlay.lock();
        let raw = overlay
            .changes
            .get(&key)
            .and_then(|change| change.value.as_ref())
            .ok_or_else(|| {
                CassieError::Unsupported("transaction-owned sequence disappeared".into())
            })?;
        let (mut metadata, _scratch) = decode(raw, &controls)?;
        if metadata.object_id != expected_identity
            || overlay.sequence_epoch(expected_identity) != expected_epoch
        {
            return Err(CassieError::Unsupported(
                "transaction-owned sequence changed during update".into(),
            ));
        }
        let owner = Arc::clone(&overlay.sequence_cursors);
        let mut state = owner.state.lock();
        if let Some(cursor) = state.cursors.get(&(expected_identity, expected_epoch)) {
            metadata.set_cursor(*cursor);
        }
        controls.check_interruption()?;
        let result = update(&mut metadata)?;
        let epoch = state
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("sequence epoch exhausted".into()))?;
        let (encoded, _encoded_memory) = encode_metadata(&metadata, &controls)?;
        let count = overlay
            .sequence_epochs
            .as_ref()
            .map_or(0, |selection| selection.epochs.len());
        let selection_bytes = count
            .checked_add(1)
            .and_then(|entries| entries.checked_mul(CURSOR_BYTES))
            .and_then(|bytes| bytes.checked_add(encoded.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("sequence epoch accounting overflow".into())
            })?;
        let _request = controls.reserve_query_memory(
            CURSOR_BYTES.checked_add(selection_bytes).ok_or_else(|| {
                CassieError::ResourceLimit("sequence epoch accounting overflow".into())
            })?,
        )?;
        if state.admission.is_none() {
            state.admission = Some(QueryMemoryReservation::independent_budget(
                controls.query_memory_budget_bytes,
                0,
            )?);
        }
        let admission = state.admission.as_ref().expect("owner admitted");
        let epoch_memory = admission.try_sibling(CURSOR_BYTES)?;
        let selection_memory = admission.try_sibling(selection_bytes)?;
        let mut epochs = overlay
            .sequence_epochs
            .as_ref()
            .map_or_else(BTreeMap::new, |selection| selection.epochs.clone());
        epochs.insert(expected_identity, epoch);
        let selection = Arc::new(EpochSelections {
            epochs,
            _memory: selection_memory,
        });
        controls.check_interruption()?;
        if let Some(fork) = &overlay.sequence_fork {
            fork.protect(&mut state, (expected_identity, epoch), &controls)?;
        }
        // No fallible operations after publishing the epoch or overlay bytes.
        state
            .admission
            .as_mut()
            .expect("owner admitted")
            .absorb(epoch_memory);
        state
            .cursors
            .insert((expected_identity, epoch), metadata.cursor());
        state.next_epoch = epoch;
        overlay.changes.get_mut(&key).expect("checked above").value = Some(encoded);
        overlay.sequence_epochs = Some(selection);
        Ok((result, metadata))
    }

    pub(crate) fn private_sequence_target(
        &self,
        name: &str,
    ) -> Result<SequenceTarget, CassieError> {
        let active = ACTIVE_STORAGE_SCOPE.with(|current| current.borrow().clone());
        let Some(scope) =
            active.filter(|scope| scope.midge_identity == std::ptr::from_ref(self) as usize)
        else {
            return Ok(SequenceTarget::Native);
        };
        let canonical = self.canonical_collection_name(name);
        let database = crate::catalog::relation_database_name(&canonical)
            .unwrap_or_else(|| self.default_database.clone());
        if self.database_family(&database)?.id() != scope.family_id {
            return Err(CassieError::Unsupported(
                "transactional DDL cannot access multiple databases".into(),
            ));
        }
        let key = super::super::key_encoding::sequence_key(&canonical);
        let overlay = scope.overlay.lock();
        let Some(change) = overlay.changes.get(&key) else {
            return Ok(SequenceTarget::Native);
        };
        let Some(raw) = &change.value else {
            return Ok(SequenceTarget::Native);
        };
        let controls = controls()?;
        let (metadata, scratch) = decode(raw, &controls)?;
        if metadata.object_id == 0 {
            return Err(CassieError::Unsupported(
                "staged sequence has no stable identity".into(),
            ));
        }
        let mut original = None;
        let prefix = super::super::key_encoding::sequence_prefix();
        for (index, (other_key, other)) in overlay.changes.iter().enumerate() {
            if index % 128 == 0 {
                controls.check_interruption()?;
            }
            if !other_key.starts_with(&prefix) {
                continue;
            }
            match &other.expected {
                ExpectedStorageValue::Untracked => {
                    return Err(CassieError::Unsupported(
                        "untracked sequence preimage".into(),
                    ))
                }
                ExpectedStorageValue::Missing => {}
                ExpectedStorageValue::Present(bytes) => {
                    let (previous, _scratch) = decode(bytes, &controls)?;
                    if previous.object_id == 0 {
                        return Err(CassieError::Unsupported(
                            "sequence preimage has no stable identity".into(),
                        ));
                    }
                    if previous.object_id == metadata.object_id
                        && original.replace(other_key).is_some()
                    {
                        return Err(CassieError::Unsupported(
                            "ambiguous sequence identity".into(),
                        ));
                    }
                }
            }
        }
        if let Some(original) = original {
            if original == &key {
                return Ok(SequenceTarget::Native);
            }
            return Err(CassieError::Unsupported(
                "advancing a renamed published sequence in transactional DDL is not supported"
                    .into(),
            ));
        }
        let epoch = overlay.sequence_epoch(metadata.object_id);
        Ok(SequenceTarget::Private {
            metadata,
            owner: Arc::clone(&overlay.sequence_cursors),
            overlay: Arc::clone(&scope.overlay),
            epoch,
            scratch,
        })
    }

    pub(crate) fn advance_private_sequence<T>(
        &self,
        original: &SequenceMeta,
        owner: &SequenceOwner,
        overlay: &Mutex<StorageOverlay>,
        epoch: u64,
        update: impl FnOnce(&mut SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, SequenceMeta), CassieError> {
        let overlay = overlay.lock();
        let key = super::super::key_encoding::sequence_key(
            &self.canonical_collection_name(&original.name),
        );
        let raw = overlay
            .changes
            .get(&key)
            .and_then(|change| change.value.as_ref())
            .ok_or_else(|| {
                CassieError::Unsupported("transaction-owned sequence disappeared".into())
            })?;
        let (metadata, _scratch) = decode(raw, &controls()?)?;
        if metadata.object_id != original.object_id
            || overlay.sequence_epoch(metadata.object_id) != epoch
        {
            return Err(CassieError::Unsupported(
                "transaction-owned sequence changed before allocation".into(),
            ));
        }
        owner.advance(metadata, epoch, overlay.sequence_fork.as_deref(), update)
    }

    pub(crate) fn is_private_sequence(&self, metadata: &SequenceMeta) -> Result<bool, CassieError> {
        Ok(matches!(
            self.private_sequence_target(&metadata.name)?,
            SequenceTarget::Private { .. }
        ))
    }
}

impl StorageOverlay {
    pub(crate) fn attach_sequence_fork_lease(
        &mut self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), CassieError> {
        let owner = Arc::clone(&self.sequence_cursors);
        let fork = Arc::new(SequenceForkLease {
            owner: Arc::clone(&owner),
            pairs: Mutex::new(BTreeMap::new()),
        });
        let mut state = owner.state.lock();
        let result = if state.cursors.is_empty() {
            Ok(())
        } else {
            let controls = controls.ok_or_else(|| {
                CassieError::Unsupported("sequence fork requires query controls".into())
            });
            controls.and_then(|controls| {
                let base = controls.reserve_query_memory(0)?;
                let (mut keys, _memory) =
                    crate::runtime::owned_memory::try_reserve_vec(state.cursors.len(), &base)?;
                keys.extend(state.cursors.keys().copied());
                for key in keys {
                    fork.protect(&mut state, key, controls)?;
                }
                Ok(())
            })
        };
        drop(state);
        result?;
        self.sequence_fork = Some(fork);
        Ok(())
    }

    pub(crate) fn inherit_sequence_fork_lease(
        &mut self,
        parent: &StorageOverlay,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), CassieError> {
        let Some(fork) = &parent.sequence_fork else {
            self.sequence_fork = None;
            return Ok(());
        };
        let mut state = self.sequence_cursors.state.lock();
        if !state.cursors.is_empty() {
            let controls = controls.ok_or_else(|| {
                CassieError::Unsupported("nested sequence fork requires query controls".into())
            })?;
            let base = controls.reserve_query_memory(0)?;
            let (mut keys, _memory) =
                crate::runtime::owned_memory::try_reserve_vec(state.cursors.len(), &base)?;
            keys.extend(state.cursors.keys().copied());
            for key in keys {
                fork.protect(&mut state, key, controls)?;
            }
        }
        drop(state);
        self.sequence_fork = Some(Arc::clone(fork));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn clear_sequence_fork_lease(&mut self) {
        self.sequence_fork = None;
    }

    #[cfg(test)]
    fn advance_private_for_test(
        &self,
        metadata: SequenceMeta,
        epoch: u64,
        _controls: &QueryExecutionControls,
    ) -> Result<(i64, SequenceMeta), CassieError> {
        self.sequence_cursors
            .advance(metadata, epoch, self.sequence_fork.as_deref(), |sequence| {
                sequence.advance().map_err(Into::into)
            })
    }

    pub(crate) fn has_sequence_cursors(&self) -> bool {
        !self.sequence_cursors.state.lock().cursors.is_empty()
    }

    /// Retires only identities absent from both the current view and retained savepoints.
    pub(crate) fn prune_sequence_cursors_with<'a>(
        &'a self,
        protected: impl Iterator<Item = &'a StorageOverlay>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        // A fork cannot enumerate identities added to the parent after the
        // fork was captured. Only the transaction's owning overlay may prune.
        if self.sequence_fork.is_some() {
            return Ok(());
        }
        let mut state = self.sequence_cursors.state.lock();
        if state.cursors.is_empty() {
            return Ok(());
        }
        let base = controls.reserve_query_memory(0)?;
        let (mut live, _memory) =
            crate::runtime::owned_memory::try_reserve_vec(state.cursors.len(), &base)?;
        let prefix = super::super::key_encoding::sequence_prefix();
        for overlay in std::iter::once(self).chain(protected) {
            for (index, (key, change)) in overlay.changes.iter().enumerate() {
                if index % 128 == 0 {
                    controls.check_interruption()?;
                }
                if !key.starts_with(&prefix) {
                    continue;
                }
                let Some(raw) = &change.value else {
                    continue;
                };
                let (metadata, _scratch) = decode(raw, controls)?;
                let key = (
                    metadata.object_id,
                    overlay.sequence_epoch(metadata.object_id),
                );
                if state.cursors.contains_key(&key) && !live.contains(&key) {
                    live.push(key);
                }
            }
        }
        for key in state.inflight.keys() {
            controls.check_interruption()?;
            if state.cursors.contains_key(key) && !live.contains(key) {
                live.push(*key);
            }
        }
        live.sort_unstable();
        state.cursors.retain(|id, _| live.binary_search(id).is_ok());
        let retained = state.cursors.len() * CURSOR_BYTES;
        if let Some(admission) = &mut state.admission {
            admission.shrink_to(retained);
        }
        Ok(())
    }

    pub(super) fn merge_sequence_cursors(
        &mut self,
        controls: &QueryExecutionControls,
    ) -> Result<(Vec<QueryMemoryReservation>, QueryMemoryReservation), CassieError> {
        let state = self.sequence_cursors.state.lock();
        let base = controls.reserve_query_memory(0)?;
        let (mut leases, directory) =
            crate::runtime::owned_memory::try_reserve_vec(state.cursors.len(), &base)?;
        if state.cursors.is_empty() {
            return Ok((leases, directory));
        }
        let prefix = super::super::key_encoding::sequence_prefix();
        let selections = self.sequence_epochs.clone();
        for (index, (key, change)) in self.changes.iter_mut().enumerate() {
            if index % 128 == 0 {
                controls.check_interruption()?;
            }
            if !key.starts_with(&prefix) {
                continue;
            }
            let Some(raw) = &mut change.value else {
                continue;
            };
            let (mut metadata, _scratch) = decode(raw, controls)?;
            if let Some(cursor) = state.cursors.get(&(
                metadata.object_id,
                selections
                    .as_ref()
                    .and_then(|owner| owner.epochs.get(&metadata.object_id))
                    .copied()
                    .unwrap_or(0),
            )) {
                metadata.set_cursor(*cursor);
                let mut counter = Counter {
                    position: 0,
                    controls,
                };
                encode_sequence_metadata(&mut counter, &metadata, controls)?;
                let (mut encoded, memory) =
                    crate::runtime::owned_memory::try_reserve_vec(counter.position, &base)?;
                {
                    let mut writer = CheckingWriter {
                        output: &mut encoded,
                        position: 0,
                        controls,
                    };
                    encode_sequence_metadata(&mut writer, &metadata, controls)?;
                }
                if encoded.len() != counter.position {
                    return Err(CassieError::Execution(
                        "sequence serialization changed size".into(),
                    ));
                }
                if leases.len() == leases.capacity() {
                    return Err(CassieError::Unsupported(
                        "duplicate staged sequence identity".into(),
                    ));
                }
                *raw = encoded;
                leases.push(memory);
            }
        }
        Ok((leases, directory))
    }

    pub(crate) fn apply_sequence_cursors_to_catalog(
        &self,
        catalog: &crate::catalog::Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let state = self.sequence_cursors.state.lock();
        if state.cursors.is_empty() {
            return Ok(());
        }
        let base = controls.reserve_query_memory(0)?;
        let (mut prepared, _directory) =
            crate::runtime::owned_memory::try_reserve_vec(state.cursors.len(), &base)?;
        let prefix = super::super::key_encoding::sequence_prefix();
        for (index, (key, change)) in self.changes.iter().enumerate() {
            if index % 128 == 0 {
                controls.check_interruption()?;
            }
            if !key.starts_with(&prefix) {
                continue;
            }
            let Some(raw) = &change.value else {
                continue;
            };
            let (mut metadata, scratch) = decode(raw, controls)?;
            if let Some(cursor) = state
                .cursors
                .get(&(metadata.object_id, self.sequence_epoch(metadata.object_id)))
            {
                if prepared.len() == prepared.capacity() {
                    return Err(CassieError::Unsupported(
                        "duplicate staged sequence identity".into(),
                    ));
                }
                metadata.set_cursor(*cursor);
                prepared.push((metadata, scratch));
            }
        }
        controls.check_interruption()?;
        // All fallible decode and admission work completes before touching the stage.
        for (metadata, _scratch) in prepared {
            catalog.register_sequence(metadata);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "sequence_owner_tests.rs"]
mod tests;
