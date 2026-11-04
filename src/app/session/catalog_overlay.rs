//! Session-owned catalog and storage changes for transactional DDL.

use super::{CassieSession, Catalog, Mutex, StorageOverlay};

impl CassieSession {
    pub(crate) fn transaction_catalog_overlay(&self) -> Option<Catalog> {
        self.transaction.lock().catalog_overlay.clone()
    }

    pub(crate) fn transaction_catalog_version(&self) -> Option<u64> {
        self.transaction
            .lock()
            .catalog_overlay
            .as_ref()
            .map(Catalog::version)
    }

    pub(crate) fn begin_transaction_catalog_overlay(&self, catalog: &Catalog) -> Catalog {
        let mut transaction = self.transaction.lock();
        if transaction.catalog_overlay.is_none() {
            transaction.catalog_base_version = Some(catalog.version());
            transaction.catalog_overlay = Some(catalog.detached_transaction_snapshot());
        }
        transaction
            .catalog_overlay
            .as_ref()
            .expect("catalog overlay initialized above")
            .clone()
    }

    pub(crate) fn transaction_catalog_base_version(&self) -> Option<u64> {
        self.transaction.lock().catalog_base_version
    }

    pub(crate) fn transaction_storage_overlay(&self) -> std::sync::Arc<Mutex<StorageOverlay>> {
        std::sync::Arc::clone(&self.transaction.lock().storage_overlay)
    }

    pub(crate) fn transaction_storage_overlay_snapshot(&self) -> StorageOverlay {
        self.transaction.lock().storage_overlay.lock().clone()
    }
}
