//! Owned COPY input authority and its query-memory reservation.

pub(super) struct CopyFromBinding {
    pub(super) operation: crate::app::transport::copy::CopyFromOperation,
    #[cfg(test)]
    pub(super) cassie: std::sync::Arc<crate::app::Cassie>,
    #[cfg(test)]
    pub(super) controls: crate::runtime::QueryExecutionControls,
    #[cfg(test)]
    pub(super) authority: crate::app::FailureAuthority,
}

impl CopyFromBinding {
    pub(super) fn controls(&self) -> &crate::runtime::QueryExecutionControls {
        self.operation.controls()
    }
}

#[cfg(test)]
impl CopyFromBinding {
    pub(super) fn failure_authority_owner_for_test(&self) -> Option<uuid::Uuid> {
        self.authority
            .identity()
            .and_then(|identity| identity.transaction)
    }
}
