//! Owned COPY input authority and its query-memory reservation.

use std::sync::Arc;

pub(super) struct CopyFromBinding {
    pub(super) cassie: Arc<crate::app::Cassie>,
    pub(super) owner: Option<crate::app::CopyCommandOwner>,
    pub(super) controls: crate::runtime::QueryExecutionControls,
    pub(super) authority: crate::app::FailureAuthority,
    pub(super) _authority_reservation: crate::runtime::QueryMemoryReservation,
}

pub(super) fn charged_bytes(owner_present: bool) -> Result<usize, crate::app::CassieError> {
    let owner_bytes = if owner_present {
        std::mem::size_of::<crate::app::CopyCommandOwner>()
    } else {
        0
    };
    std::mem::size_of::<CopyFromBinding>()
        .checked_sub(owner_bytes)
        .ok_or_else(|| crate::app::CassieError::ResourceLimit("COPY binding size underflow".into()))
}

#[cfg(test)]
impl CopyFromBinding {
    pub(super) fn failure_authority_owner_for_test(&self) -> Option<uuid::Uuid> {
        self.authority.owner_for_test().map(|(owner, _)| owner)
    }

    pub(super) fn authority_reservation_for_test(&self) -> &crate::runtime::QueryMemoryReservation {
        let Self {
            _authority_reservation: reservation,
            ..
        } = self;
        reservation
    }
}
