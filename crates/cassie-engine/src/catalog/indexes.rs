pub use cassie_core::catalog::declarations::IndexKind;
pub use cassie_core::catalog::storage_metadata::{
    ColumnBatchChunkMeta, ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum,
    ColumnBatchRow, ColumnBatchSegmentMeta,
};

#[cfg(test)]
#[path = "indexes_identity_tests.rs"]
mod identity_tests;

pub use cassie_sql::catalog_declarations::{IndexMeta, ReservationIdentity, UniqueViolationDetails};

/// Engine error and catalog identity operations kept outside the SQL crate.
pub(crate) trait EngineIndexMetaExt {
    fn reservation_identity(&self) -> Result<ReservationIdentity<'_>, crate::app::CassieError>;
    fn unique_violation(&self, collection: &str) -> crate::app::CassieError;
    fn preserve_identity_before_name_change(&mut self);
    fn references_field_checked(
        &self,
        field: &str,
    ) -> Result<bool, crate::app::CassieError>;
}

impl EngineIndexMetaExt for IndexMeta {
    fn reservation_identity(&self) -> Result<ReservationIdentity<'_>, crate::app::CassieError> {
        self.reservation_identity_details()
            .map_err(crate::app::CassieError::Parse)
    }

    fn unique_violation(&self, collection: &str) -> crate::app::CassieError {
        let details = self.unique_violation_details(collection);
        crate::app::CassieError::UniqueViolation {
            table: details.table,
            column: details.column,
            constraint: details.constraint,
        }
    }

    fn preserve_identity_before_name_change(&mut self) {
        let name = self.name.clone();
        self.set_catalog_oid_if_missing(crate::catalog::object_identity::stable_index_oid(&name));
        if self.unique {
            self.set_unique_reservation_name_if_missing(&name);
        }
    }

    fn references_field_checked(
        &self,
        field: &str,
    ) -> Result<bool, crate::app::CassieError> {
        cassie_sql::catalog_declarations::IndexMeta::references_field_checked(self, field)
            .map_err(crate::app::CassieError::Parse)
    }
}


