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
    cursors: BTreeMap<u64, SequenceCursor>,
    admission: Option<QueryMemoryReservation>,
}

pub(crate) enum SequenceTarget {
    Private {
        metadata: SequenceMeta,
        owner: Arc<SequenceOwner>,
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
        if self.position % 4096 == 0 {
            self.controls
                .check_interruption()
                .map_err(io::Error::other)?;
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

#[derive(Default)]
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("sequence serialization overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SequenceOwner {
    pub(crate) fn advance<T>(
        &self,
        mut metadata: SequenceMeta,
        update: impl FnOnce(&mut SequenceMeta) -> Result<T, CassieError>,
    ) -> Result<(T, SequenceMeta), CassieError> {
        let mut state = self.state.lock();
        if let Some(cursor) = state.cursors.get(&metadata.object_id) {
            metadata.set_cursor(*cursor);
        }
        let controls = super::super::publication::PublicationControlScope::current_query_controls()
            .ok_or_else(|| {
                CassieError::Unsupported("staged sequence requires query controls".into())
            })?;
        controls.check_interruption()?;
        let is_new = !state.cursors.contains_key(&metadata.object_id);
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
        state.cursors.insert(metadata.object_id, metadata.cursor());
        Ok((result, metadata))
    }
}

impl Midge {
    /// A catalog ALTER/RESTART cannot replace a private cursor until it has
    /// transaction-aware restart and savepoint ownership.
    pub(crate) fn ensure_no_private_cursor_for_catalog_update(
        &self,
        name: &str,
    ) -> Result<(), CassieError> {
        let active = ACTIVE_STORAGE_SCOPE.with(|current| current.borrow().clone());
        let Some(scope) = active.filter(|scope| {
            scope.midge_identity == std::ptr::from_ref(self) as usize && scope.stage_writes
        }) else {
            return Ok(());
        };
        let overlay = scope.overlay.lock();
        let state = overlay.sequence_cursors.state.lock();
        if state.cursors.is_empty() {
            return Ok(());
        }
        let key = super::super::key_encoding::sequence_key(&self.canonical_collection_name(name));
        let Some(raw) = overlay
            .changes
            .get(&key)
            .and_then(|change| change.value.as_ref())
        else {
            return Ok(());
        };
        let (metadata, _scratch) = decode(raw, &controls()?)?;
        if state.cursors.contains_key(&metadata.object_id) {
            return Err(CassieError::Unsupported("ALTER or RESTART of a transaction-owned sequence after advancement is not supported".into()));
        }
        Ok(())
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
                    if previous.object_id == metadata.object_id {
                        if original.replace(other_key).is_some() {
                            return Err(CassieError::Unsupported(
                                "ambiguous sequence identity".into(),
                            ));
                        }
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
        Ok(SequenceTarget::Private {
            metadata,
            owner: Arc::clone(&overlay.sequence_cursors),
            scratch,
        })
    }

    pub(crate) fn is_private_sequence(&self, metadata: &SequenceMeta) -> Result<bool, CassieError> {
        Ok(matches!(
            self.private_sequence_target(&metadata.name)?,
            SequenceTarget::Private { .. }
        ))
    }
}

impl StorageOverlay {
    pub(crate) fn has_sequence_cursors(&self) -> bool {
        !self.sequence_cursors.state.lock().cursors.is_empty()
    }

    /// Retires only identities absent from both the current view and retained savepoints.
    pub(crate) fn prune_sequence_cursors_with<'a>(
        &'a self,
        protected: impl Iterator<Item = &'a StorageOverlay>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
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
                if state.cursors.contains_key(&metadata.object_id)
                    && !live.contains(&metadata.object_id)
                {
                    live.push(metadata.object_id);
                }
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
            if let Some(cursor) = state.cursors.get(&metadata.object_id) {
                metadata.set_cursor(*cursor);
                let mut counter = Counter::default();
                serde_json::to_writer(&mut counter, &metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                let (mut encoded, memory) =
                    crate::runtime::owned_memory::try_reserve_vec(counter.0, &base)?;
                serde_json::to_writer(&mut encoded, &metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                if encoded.len() != counter.0 {
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
            if let Some(cursor) = state.cursors.get(&metadata.object_id) {
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
mod tests {
    use super::{decode, CheckingReader, StorageOverlay, CURSOR_BYTES};
    use crate::catalog::SequenceMeta;
    use crate::config::CassieRuntimeLimits;
    use crate::midge::adapter::PublicationControlScope;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::DataType;
    use std::time::Instant;

    fn controls() -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: CURSOR_BYTES,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_bound_and_retire_transaction_cursor_owners_across_statements() {
        // Arrange
        let overlay = StorageOverlay::default();
        let owner = &overlay.sequence_cursors;
        let mut first = SequenceMeta::new("first", DataType::BigInt);
        first.object_id = 11;
        let mut second = SequenceMeta::new("second", DataType::BigInt);
        second.object_id = 12;
        let statement_one = controls();
        let statement_two = controls();

        // Act
        let first_value = {
            let _scope = PublicationControlScope::enter(Some(&statement_one));
            owner.advance(first, |sequence| sequence.advance().map_err(Into::into))
        };
        let rejected = {
            let _scope = PublicationControlScope::enter(Some(&statement_two));
            owner.advance(second.clone(), |sequence| {
                sequence.advance().map_err(Into::into)
            })
        };
        {
            let _scope = PublicationControlScope::enter(Some(&statement_two));
            overlay
                .prune_sequence_cursors_with(std::iter::empty(), &statement_two)
                .expect("prune retired identity");
        }
        let second_value = {
            let _scope = PublicationControlScope::enter(Some(&statement_two));
            owner.advance(second, |sequence| sequence.advance().map_err(Into::into))
        };
        let retained = owner
            .state
            .lock()
            .admission
            .as_ref()
            .expect("transaction admission")
            .bytes();
        {
            let _scope = PublicationControlScope::enter(Some(&statement_two));
            overlay
                .prune_sequence_cursors_with(std::iter::empty(), &statement_two)
                .expect("retire final identity");
        }
        let released = owner
            .state
            .lock()
            .admission
            .as_ref()
            .expect("stable admission")
            .bytes();

        // Assert
        assert_eq!(first_value.expect("first cursor").0, 1);
        assert!(rejected.is_err());
        assert_eq!(second_value.expect("second cursor").0, 1);
        assert_eq!(retained, CURSOR_BYTES);
        assert_eq!(released, 0);
        assert_eq!(statement_one.current_query_memory_bytes(), 0);
        assert_eq!(statement_two.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_admit_and_interrupt_sequence_metadata_decode_at_its_boundary() {
        // Arrange
        let metadata = SequenceMeta::new("sequence", DataType::BigInt);
        let raw = serde_json::to_vec(&metadata).expect("sequence metadata");
        let bytes = crate::runtime::accounted::json::serialized_decode_bytes(raw.len())
            .expect("decode bound");
        let limits = |budget| CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        };
        let below = QueryExecutionControls::from_limits(&limits(bytes - 1), Instant::now());
        let exact = QueryExecutionControls::from_limits(&limits(bytes), Instant::now());
        let cancellation = QueryCancellationHandle::new();
        let interrupted = QueryExecutionControls::with_cancellation(
            &limits(bytes),
            Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();

        // Act
        let rejected = decode(&raw, &below);
        let admitted = decode(&raw, &exact);
        let cancelled = decode(&raw, &interrupted);
        let held = exact.current_query_memory_bytes();
        drop(admitted);

        // Assert
        assert!(rejected.is_err());
        assert!(matches!(
            cancelled,
            Err(crate::app::CassieError::QueryCancelled)
        ));
        assert_eq!(held, bytes);
        assert_eq!(exact.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_a_cursor_referenced_only_by_a_retained_savepoint() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 1024 * 1024,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut current = StorageOverlay::default();
        let mut sequence = SequenceMeta::new("saved", DataType::BigInt);
        sequence.object_id = 91;
        let _scope = PublicationControlScope::enter(Some(&controls));
        let first = current.sequence_cursors.advance(sequence.clone(), |value| {
            value.advance().map_err(Into::into)
        });
        let key = super::super::super::key_encoding::sequence_key("saved");
        current.changes.insert(
            key,
            super::super::StorageMutation {
                value: Some(serde_json::to_vec(&sequence).expect("sequence metadata")),
                ttl_seconds: None,
                expected: super::super::ExpectedStorageValue::Missing,
            },
        );
        let savepoint = current.clone();
        current.changes.clear();

        // Act
        current
            .prune_sequence_cursors_with(std::iter::once(&savepoint), &controls)
            .expect("preserve savepoint");
        let second = current
            .sequence_cursors
            .advance(sequence, |value| value.advance().map_err(Into::into));
        current
            .prune_sequence_cursors_with(std::iter::empty(), &controls)
            .expect("retire released owner");
        let retained = current
            .sequence_cursors
            .state
            .lock()
            .admission
            .as_ref()
            .expect("owner")
            .bytes();

        // Assert
        assert_eq!(first.expect("first").0, 1);
        assert_eq!(second.expect("savepoint cursor").0, 2);
        assert_eq!(retained, 0);
    }

    #[test]
    fn should_interrupt_metadata_reader_at_a_deterministic_chunk_boundary() {
        // Arrange
        use std::io::Read;
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let input = vec![b'x'; 4097];
        let mut reader = CheckingReader {
            bytes: &input,
            position: 0,
            controls: &controls,
        };
        let mut chunk = [0; 4096];
        reader.read_exact(&mut chunk).expect("first chunk");
        cancellation.cancel();

        // Act
        let result = reader.read(&mut [0]);

        // Assert
        assert!(result.is_err());
        assert_eq!(reader.position, 4096);
    }
}
