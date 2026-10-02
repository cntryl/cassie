use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};

pub fn pending_record(cassie: &Cassie) -> (Vec<u8>, serde_json::Value) {
    schema_record(cassie, |record| record.get("vector_backfill").is_some())
}

pub fn staged_record(cassie: &Cassie) -> (Vec<u8>, serde_json::Value) {
    schema_record(cassie, |record| {
        record.get("id").is_some() && record.get("payload").is_some()
    })
}

fn schema_record(
    cassie: &Cassie,
    matches: impl Fn(&serde_json::Value) -> bool,
) -> (Vec<u8>, serde_json::Value) {
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, &[])
        .expect("scan schema")
        .into_iter()
        .filter_map(|(key, raw)| {
            serde_json::from_slice::<serde_json::Value>(&raw)
                .ok()
                .map(|record| (key, record))
        })
        .find(|(_, record)| matches(record))
        .expect("expected publication record")
}

pub fn write_schema_record(cassie: &Cassie, key: Vec<u8>, record: Option<&serde_json::Value>) {
    let mut tx = cassie
        .midge
        .schema_tx(TransactionMode::ReadWrite)
        .expect("schema transaction");
    if let Some(record) = record {
        tx.put(
            key,
            serde_json::to_vec(record).expect("encode record"),
            None,
        )
        .expect("write record");
    } else {
        tx.delete(key).expect("delete record");
    }
    tx.commit(WriteOptions::sync()).expect("commit record");
}
