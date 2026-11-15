//! Admitted immutable storage overlay for a COPY command owner.

use super::{Arc, CassieError, ExpectedStorageValue, StorageMutation, StorageOverlay};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::collections::BTreeMap;
use std::mem::size_of;

// A full internal B-tree node per entry bounds sparse trees and node splits.
const NODE_BYTES: usize = 2048;

#[derive(Debug)]
pub(crate) struct CopyStorageSnapshot {
    overlay: StorageOverlay,
    _memory: QueryMemoryReservation,
}

/// Checkpoint-visible subset of one command's immutable storage overlay.
#[derive(Debug)]
pub(crate) struct CheckpointStorageSnapshot {
    changes: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    _memory: QueryMemoryReservation,
}

impl CheckpointStorageSnapshot {
    pub(crate) fn get(&self, key: &[u8]) -> Option<Option<&[u8]>> {
        self.changes.get(key).map(|value| value.as_deref())
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (&[u8], Option<&[u8]>)> {
        self.changes.iter().map(|(key, value)| (key.as_slice(), value.as_deref()))
    }
}

fn copy_bytes(bytes: &[u8], controls: &QueryExecutionControls) -> Result<Vec<u8>, CassieError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if copy.capacity() > bytes.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY snapshot capacity".into(),
        ));
    }
    for chunk in bytes.chunks(4096) {
        controls.check_interruption()?;
        copy.extend_from_slice(chunk);
    }
    Ok(copy)
}

fn bytes_equal(
    left: &[u8],
    right: &[u8],
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.chunks(4096).zip(right.chunks(4096)) {
        controls.check_interruption()?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

impl StorageOverlay {
    pub(crate) fn copy_checkpoint_snapshot(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Arc<CheckpointStorageSnapshot>>, CassieError> {
        let projection_prefix = super::super::Midge::projection_prefix();
        let collections_key = super::super::Midge::collections_key();
        let relevant = |key: &[u8]| key == collections_key.as_slice() || key.starts_with(&projection_prefix);
        let mut bytes = size_of::<CheckpointStorageSnapshot>() + 2 * size_of::<usize>() + NODE_BYTES;
        let mut any = false;
        for (key, change) in &self.changes {
            controls.check_interruption()?;
            if relevant(key) {
                any = true;
                bytes = add(bytes, add(NODE_BYTES, key.len())?)?;
                if let Some(value) = &change.value {
                    bytes = add(bytes, value.len())?;
                }
            }
        }
        if !any {
            return Ok(None);
        }
        let memory = controls.reserve_query_memory(bytes)?;
        let mut changes = BTreeMap::new();
        for (key, change) in &self.changes {
            controls.check_interruption()?;
            if relevant(key) {
                let value = change.value.as_ref().map(|value| copy_bytes(value, controls)).transpose()?;
                changes.insert(copy_bytes(key, controls)?, value);
            }
        }
        Ok(Some(Arc::new(CheckpointStorageSnapshot { changes, _memory: memory })))
    }

    pub(crate) fn copy_snapshot(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<CopyStorageSnapshot>, CassieError> {
        let mut bytes = add(
            size_of::<CopyStorageSnapshot>() + 2 * size_of::<usize>(),
            NODE_BYTES,
        )?;
        for (key, change) in &self.changes {
            controls.check_interruption()?;
            bytes = add(bytes, add(NODE_BYTES, key.len())?)?;
            if let Some(value) = &change.value {
                bytes = add(bytes, value.len())?;
            }
            if let ExpectedStorageValue::Present(value) = &change.expected {
                bytes = add(bytes, value.len())?;
            }
        }
        // Arc headers for the snapshot and retained guard are bounded separately.
        bytes = add(bytes, mul(4, size_of::<usize>())?)?;
        let memory = controls.reserve_query_memory(bytes)?;
        let mut changes = BTreeMap::new();
        for (key, change) in &self.changes {
            controls.check_interruption()?;
            let value = change
                .value
                .as_ref()
                .map(|value| copy_bytes(value, controls))
                .transpose()?;
            let expected = match &change.expected {
                ExpectedStorageValue::Untracked => ExpectedStorageValue::Untracked,
                ExpectedStorageValue::Missing => ExpectedStorageValue::Missing,
                ExpectedStorageValue::Present(value) => {
                    ExpectedStorageValue::Present(copy_bytes(value, controls)?)
                }
            };
            changes.insert(
                copy_bytes(key, controls)?,
                StorageMutation {
                    value,
                    ttl_seconds: change.ttl_seconds,
                    expected,
                },
            );
        }
        let mut overlay = StorageOverlay {
            changes,
            sequence_cursors: Arc::clone(&self.sequence_cursors),
            sequence_epochs: self.sequence_epochs.clone(),
            sequence_fork: None,
        };
        overlay.attach_sequence_fork_lease(Some(controls))?;
        controls.check_interruption()?;
        Ok(Arc::new(CopyStorageSnapshot {
            overlay,
            _memory: memory,
        }))
    }
}

impl CopyStorageSnapshot {
    pub(crate) fn matches(
        &self,
        other: &StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<bool, CassieError> {
        if !Arc::ptr_eq(&self.overlay.sequence_cursors, &other.sequence_cursors)
            || self.overlay.changes.len() != other.changes.len()
        {
            return Ok(false);
        }
        let left_epochs = self
            .overlay
            .sequence_epochs
            .as_ref()
            .map(|owner| &owner.epochs);
        let right_epochs = other.sequence_epochs.as_ref().map(|owner| &owner.epochs);
        match (left_epochs, right_epochs) {
            (None, None) => {}
            (Some(left), Some(right)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (left, right) in left.iter().zip(right.iter()) {
                    controls.check_interruption()?;
                    if left != right {
                        return Ok(false);
                    }
                }
            }
            _ => return Ok(false),
        }
        for ((left_key, left), (right_key, right)) in
            self.overlay.changes.iter().zip(other.changes.iter())
        {
            controls.check_interruption()?;
            if !bytes_equal(left_key, right_key, controls)? || left.ttl_seconds != right.ttl_seconds
            {
                return Ok(false);
            }
            let same_value = match (&left.value, &right.value) {
                (None, None) => true,
                (Some(left), Some(right)) => bytes_equal(left, right, controls)?,
                _ => false,
            };
            let same_expected = match (&left.expected, &right.expected) {
                (ExpectedStorageValue::Untracked, ExpectedStorageValue::Untracked)
                | (ExpectedStorageValue::Missing, ExpectedStorageValue::Missing) => true,
                (ExpectedStorageValue::Present(left), ExpectedStorageValue::Present(right)) => {
                    bytes_equal(left, right, controls)?
                }
                _ => false,
            };
            if !same_value || !same_expected {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
