//! Read-only Midge family inspection after Cassie admission rejects a directory.
use std::collections::BTreeMap;
use std::path::Path;

pub type FamilyContents = BTreeMap<String, Vec<(Vec<u8>, Vec<u8>)>>;

/// Reads existing families directly through the sole Midge storage owner.
///
/// # Panics
///
/// Panics if opening, scanning or closing the local storage owner fails.
pub fn read(path: &Path) -> FamilyContents {
    let options = cntryl_midge::OpenOptions::local(path)
        .build()
        .expect("options");
    let mut engine = cntryl_midge::Engine::open(options).expect("inspection owner");
    let contents = engine
        .list_column_families()
        .expect("families")
        .into_iter()
        .map(|family| {
            let tx = engine
                .begin_tx(family.id(), cntryl_midge::TransactionMode::ReadOnly)
                .expect("read transaction");
            let rows = tx
                .scan(&cntryl_midge::Query::new())
                .expect("scan")
                .try_collect()
                .expect("entries");
            (
                family.name().to_owned(),
                rows.into_iter()
                    .map(|(key, value)| (key.to_vec(), value.to_vec()))
                    .collect(),
            )
        })
        .collect();
    engine
        .shutdown(std::time::Duration::from_secs(5))
        .expect("close inspection owner");
    contents
}

/// Removes only an empty temporary family to model an interrupted bootstrap inventory.
///
/// # Panics
///
/// Panics if the family is nonempty or Midge cannot safely remove it.
pub fn remove_empty_temp_family(path: &Path) {
    let options = cntryl_midge::OpenOptions::local(path)
        .build()
        .expect("options");
    let mut engine = cntryl_midge::Engine::open(options).expect("inventory owner");
    let family = engine.get_column_family("cf1").expect("temporary family");
    let read = engine
        .begin_tx(family.id(), cntryl_midge::TransactionMode::ReadOnly)
        .expect("read transaction");
    assert_eq!(
        read.scan(&cntryl_midge::Query::new())
            .expect("scan")
            .try_collect()
            .expect("entries"),
        Vec::new()
    );
    drop(read);
    engine
        .drop_column_family(family.id())
        .expect("safe empty-family drop");
    engine
        .shutdown(std::time::Duration::from_secs(5))
        .expect("close owner");
}

/// Creates an unpublished empty inventory through Midge's native family API.
///
/// # Panics
///
/// Panics if creating or closing the fixture storage owner fails.
pub fn create_empty_families(path: &Path, names: &[&str]) {
    let options = cntryl_midge::OpenOptions::local(path)
        .build()
        .expect("options");
    let mut engine = cntryl_midge::Engine::open(options).expect("inventory owner");
    for name in names {
        engine.create_column_family(name).expect("empty family");
    }
    engine
        .shutdown(std::time::Duration::from_secs(5))
        .expect("close owner");
}
