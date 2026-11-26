use crate::{
    adapter::{Midge, StorageFamily},
    key_codec,
};

#[test]
fn should_read_an_index_entry_written_in_one_adapter_batch() {
    // Arrange
    const ROWS: usize = 1_024;
    let midge = Midge::new_in_memory("postgres").expect("open in-memory Midge");
    let row = 512;
    let key = key_codec::encode_key(b"benchmark-index", &[format!("{row:04}").as_bytes()]);
    let value = format!("document-{row:04}").into_bytes();
    let mut tx = midge
        .begin_raw_family_adapter_tx(
            StorageFamily::Data,
            cntryl_midge::TransactionMode::ReadWrite,
        )
        .expect("begin adapter transaction");
    for current in 0..ROWS {
        tx.put(
            key_codec::encode_key(b"benchmark-index", &[format!("{current:04}").as_bytes()]),
            format!("document-{current:04}").into_bytes(),
            None,
        )
        .expect("stage index entry");
    }
    tx.commit(midge.write_options_sync())
        .expect("commit index entry");

    // Act
    let actual = midge
        .raw_get(StorageFamily::Data, &key)
        .expect("read index entry");

    // Assert
    assert_eq!(actual.as_deref(), Some(value.as_slice()));
}
