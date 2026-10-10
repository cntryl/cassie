use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};

pub type RawRecords = Vec<(Vec<u8>, Vec<u8>)>;

pub struct PendingBackfill {
    pub publication_id: String,
    pub rows: Vec<(String, serde_json::Value)>,
    pub vector_index: cassie::embeddings::VectorIndexRecord,
    pub sql_index: Option<cassie::catalog::IndexMeta>,
}

impl PendingBackfill {
    pub fn capture(cassie: &Cassie, source_generation: u64, publish_sql_index: bool) -> Self {
        let pending = records(cassie, StorageFamily::Schema, b"index-publication");
        assert_eq!(pending.len(), 1, "exactly one durable publication intent");
        let record: serde_json::Value =
            serde_json::from_slice(&pending[0].1).expect("pending publication JSON");
        assert_eq!(record["state"], "Prepared");
        assert_eq!(record["target_generation"], source_generation);
        let backfill = &record["vector_backfill"];
        assert_eq!(backfill["version"], 1);
        assert_eq!(backfill["row_count"], 2);
        assert_eq!(backfill["source_generation"], source_generation);
        assert_eq!(backfill["publish_sql_index"], publish_sql_index);
        let publication_id = backfill["publication_id"]
            .as_str()
            .expect("publication ID")
            .to_string();
        uuid::Uuid::parse_str(&publication_id).expect("valid publication identity");
        let staged = records(cassie, StorageFamily::Schema, b"vector-backfill-row");
        assert_eq!(staged.len(), 2, "exact staged payload count");
        let mut rows = staged
            .into_iter()
            .enumerate()
            .map(|(ordinal, (key, raw))| {
                assert_eq!(
                    key,
                    publication_key(
                        b"vector-backfill-row",
                        &[
                            publication_id.as_bytes(),
                            &u64::try_from(ordinal).unwrap().to_be_bytes()
                        ],
                    ),
                    "staging key belongs to the selected publication and ordinal"
                );
                let row: serde_json::Value = serde_json::from_slice(&raw).expect("staged row JSON");
                let mut payload = row["payload"].clone();
                // The declared VECTOR(3) carrier is f32. Compare complete decoded
                // payloads with those exact values, including f32 JSON round trips.
                let vector: Vec<f32> = serde_json::from_value(payload["embedding"].clone())
                    .expect("staged vector components");
                assert_eq!(vector.len(), 3);
                payload["embedding"] =
                    serde_json::json!(vector.into_iter().map(f64::from).collect::<Vec<_>>());
                (
                    row["id"].as_str().expect("staged row identity").to_string(),
                    payload,
                )
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        Self {
            publication_id,
            rows,
            vector_index: serde_json::from_value(backfill["vector_index"].clone())
                .expect("pending vector metadata"),
            sql_index: publish_sql_index.then(|| {
                serde_json::from_value(record["index"].clone()).expect("pending SQL metadata")
            }),
        }
    }

    pub fn assert_applied_marker(&self, cassie: &Cassie, applied: bool) {
        let markers = records(cassie, StorageFamily::Data, b"vector-backfill-applied");
        if applied {
            assert_eq!(markers.len(), 1, "exact applied marker count");
            assert_eq!(
                markers[0].0,
                publication_key(
                    b"vector-backfill-applied",
                    &[self.publication_id.as_bytes()]
                ),
                "applied marker key belongs to the selected publication"
            );
            assert_eq!(markers[0].1, self.publication_id.as_bytes());
        } else {
            assert!(
                markers.is_empty(),
                "unapplied publication has no Data marker"
            );
        }
    }
}

pub fn row_payloads(cassie: &Cassie, collection: &str) -> Vec<(String, serde_json::Value)> {
    let mut rows = cassie
        .midge
        .all_fields_json(collection)
        .expect("full stored payloads");
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

pub fn raw_rows(cassie: &Cassie) -> RawRecords {
    // Each private restart fixture owns one relation in its own local database.
    let rows = records(cassie, StorageFamily::Data, b"\x10");
    assert_eq!(rows.len(), 2, "exact authoritative row-blob count");
    rows
}

pub fn assert_cleanup(cassie: &Cassie) {
    for family in [b"index-publication".as_slice(), b"vector-backfill-row"] {
        assert!(
            records(cassie, StorageFamily::Schema, family).is_empty(),
            "startup must clean publication intent and staged payloads"
        );
    }
    assert!(
        records(cassie, StorageFamily::Data, b"vector-backfill-applied").is_empty(),
        "startup must clean applied publication markers"
    );
}

fn records(cassie: &Cassie, storage: StorageFamily, family: &[u8]) -> RawRecords {
    let mut encoder = cntryl_lexkey::Encoder::with_capacity(64);
    encoder.encode_composite_into_buf(&[b"cassie", b"\x01", family]);
    encoder.push_separator();
    let prefix = encoder.into_vec();
    cassie
        .midge
        .raw_scan_prefix(storage, &prefix)
        .expect("scan exact storage family")
}

fn publication_key(family: &[u8], components: &[&[u8]]) -> Vec<u8> {
    let mut encoder = cntryl_lexkey::Encoder::with_capacity(96);
    let mut parts = vec![b"cassie".as_slice(), b"\x01", family];
    parts.extend_from_slice(components);
    encoder.encode_composite_into_buf(&parts);
    encoder.into_vec()
}

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
