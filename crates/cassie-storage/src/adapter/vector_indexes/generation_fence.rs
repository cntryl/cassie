//! Generation fence for normalized-vector sidecars.
//!
//! A document write restamps only the normalized vectors it changed, so an
//! unchanged record legitimately carries an older generation than the
//! collection. The fence therefore accepts a record stamped between the
//! generation of the last full publication (the floor, persisted next to the
//! index state) and the generation a reader observes. Without a floor (an
//! unpublished or mid-rebuild index) it demands exact equality, so every record
//! written before the rebuild is rejected.

use super::{CassieError, Midge};

/// Whether a record stamped `record` is current for a reader at `current`.
pub(super) fn normalized_generation_is_current(
    floor: Option<u64>,
    record: u64,
    current: u64,
) -> bool {
    floor.map_or(record == current, |floor| {
        floor <= record && record <= current
    })
}

impl Midge {
    /// Reads the generation of the last full normalized-vector publication.
    pub(super) fn normalized_generation_floor(
        tx: &crate::adapter::AdapterTransaction,
        relation_id: u64,
        field_id: u32,
    ) -> Result<Option<u64>, CassieError> {
        Self::decode_normalized_generation_floor(
            tx.get(&super::super::key_encoding::normalized_vector_floor_key(
                relation_id,
                field_id,
            ))
            .map_err(CassieError::from)?,
        )
    }

    /// Decodes a persisted floor record, when one exists.
    pub(super) fn decode_normalized_generation_floor(
        raw: Option<impl AsRef<[u8]>>,
    ) -> Result<Option<u64>, CassieError> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        let bytes: [u8; 8] = raw
            .as_ref()
            .try_into()
            .map_err(|_| CassieError::Parse("invalid normalized vector floor".to_string()))?;
        Ok(Some(u64::from_be_bytes(bytes)))
    }

    pub(super) fn normalized_generation_floor_record(
        relation_id: u64,
        field_id: u32,
        generation: u64,
    ) -> (Vec<u8>, Vec<u8>) {
        (
            super::super::key_encoding::normalized_vector_floor_key(relation_id, field_id),
            generation.to_be_bytes().to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_generation_is_current;

    #[test]
    fn should_require_exact_generation_without_a_floor() {
        // Arrange
        let floor = None;

        // Act
        let exact = normalized_generation_is_current(floor, 4, 4);
        let older = normalized_generation_is_current(floor, 3, 4);

        // Assert
        assert!(exact);
        assert!(!older);
    }

    #[test]
    fn should_accept_records_within_the_floor_to_observed_range() {
        // Arrange
        let floor = Some(3);

        // Act
        let at_floor = normalized_generation_is_current(floor, 3, 7);
        let restamped = normalized_generation_is_current(floor, 6, 7);
        let before_floor = normalized_generation_is_current(floor, 2, 7);
        let from_the_future = normalized_generation_is_current(floor, 8, 7);

        // Assert
        assert!(at_floor);
        assert!(restamped);
        assert!(!before_floor);
        assert!(!from_the_future);
    }
}
