//! Direct scalar-entry corruption fixtures for REINDEX recovery assertions.
use cassie::app::Cassie;
use cassie::midge::adapter::{Midge, StorageFamily};

pub fn entries(cassie: &Cassie, collection: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let id = cassie
        .midge
        .collection_metadata(collection)
        .unwrap()
        .unwrap()
        .storage_id;
    let prefix = Midge::scalar_index_collection_prefix_for_diagnostics(id);
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .unwrap()
}

pub fn remove_entries(cassie: &Cassie, collection: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let before = entries(cassie, collection);
    assert_ne!(before, Vec::new());
    for (key, _) in &before {
        cassie.midge.raw_delete(StorageFamily::Data, key).unwrap();
    }
    before
}
