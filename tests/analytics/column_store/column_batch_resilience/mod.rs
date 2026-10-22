use cassie::app::{Cassie, CassieSession};
use cassie::catalog::ColumnBatchMetadata;
use cassie::midge::adapter::{
    decode_column_batch_manifest_for_test, encode_column_batch_manifest_for_test,
    set_column_batch_maintenance_failure_point, StorageFamily,
};
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};
use sha2::{Digest, Sha256};

use super::support_sql as support;
use support::{canonical_test_collection, canonical_test_index, data_dir};

struct AmountFixture {
    path: String,
    cassie: Cassie,
    session: CassieSession,
    collection: String,
    index: String,
}

fn amount_fixture(label: &str, table: &str, values: &[Value]) -> AmountFixture {
    amount_fixture_with_storage(label, table, values, false)
}

fn persistent_amount_fixture(label: &str, table: &str, values: &[Value]) -> AmountFixture {
    amount_fixture_with_storage(label, table, values, true)
}

fn amount_fixture_with_storage(
    label: &str,
    table: &str,
    values: &[Value],
    persistent: bool,
) -> AmountFixture {
    let path = data_dir(label);
    let cassie = if persistent {
        Cassie::new_with_data_dir(&path).expect("create Cassie")
    } else {
        crate::support_sql::new_in_memory_cassie().expect("create Cassie")
    };
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (amount BIGINT)"),
            vec![],
        )
        .expect("create table");
    for value in values {
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO {table} (amount) VALUES ($1)"),
                vec![value.clone()],
            )
            .expect("insert amount");
    }
    let index_name = format!("{table}_column_idx");
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = 1)"
            ),
            vec![],
        )
        .expect("create column index");
    let collection = canonical_test_collection(&cassie, table);
    let index = canonical_test_index(&cassie, &collection, &index_name);
    AmountFixture {
        path,
        cassie,
        session,
        collection,
        index,
    }
}

fn ordered_numeric_fixture(
    label: &str,
    table: &str,
    column_type: &str,
    values: &[serde_json::Value],
    segment_size: usize,
) -> AmountFixture {
    ordered_numeric_fixture_with_storage(label, table, column_type, values, segment_size, false)
}

fn persistent_ordered_numeric_fixture(
    label: &str,
    table: &str,
    column_type: &str,
    values: &[serde_json::Value],
    segment_size: usize,
) -> AmountFixture {
    ordered_numeric_fixture_with_storage(label, table, column_type, values, segment_size, true)
}

fn ordered_numeric_fixture_with_storage(
    label: &str,
    table: &str,
    column_type: &str,
    values: &[serde_json::Value],
    segment_size: usize,
    persistent: bool,
) -> AmountFixture {
    let path = data_dir(label);
    let cassie = if persistent {
        Cassie::new_with_data_dir(&path).expect("create Cassie")
    } else {
        crate::support_sql::new_in_memory_cassie().expect("create Cassie")
    };
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (amount {column_type})"),
            vec![],
        )
        .expect("create table");
    let collection = canonical_test_collection(&cassie, table);
    for (position, value) in values.iter().enumerate() {
        cassie
            .midge
            .put_document(
                &collection,
                Some(format!("row-{position:04}")),
                serde_json::json!({ "amount": value }),
            )
            .expect("insert ordered amount");
    }
    let index_name = format!("{table}_column_idx");
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE INDEX {index_name} ON {table} USING column (amount) WITH (segment_size = {segment_size})"
            ),
            vec![],
        )
        .expect("create column index");
    let index = canonical_test_index(&cassie, &collection, &index_name);
    AmountFixture {
        path,
        cassie,
        session,
        collection,
        index,
    }
}

fn metadata_entry(fixture: &AmountFixture) -> (Vec<u8>, ColumnBatchMetadata) {
    fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan data")
        .into_iter()
        .find_map(|(key, raw)| {
            raw.starts_with(b"CBM2")
                .then(|| decode_column_batch_manifest_for_test(&raw).ok())
                .flatten()
                .map(|metadata| (key, metadata))
        })
        .expect("column batch metadata")
}

fn write_metadata(fixture: &AmountFixture, key: Vec<u8>, metadata: &ColumnBatchMetadata) {
    write_raw_value(
        fixture,
        key,
        encode_column_batch_manifest_for_test(metadata).expect("encode metadata"),
    );
}

fn write_raw_value(fixture: &AmountFixture, key: Vec<u8>, value: Vec<u8>) {
    let mut tx = fixture
        .cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("open data transaction");
    tx.put(key, value, None).expect("write raw value");
    tx.commit(WriteOptions::sync())
        .expect("commit metadata mutation");
}

fn resign_manifest(mut raw: Vec<u8>) -> Vec<u8> {
    raw.truncate(raw.len().saturating_sub(32));
    let checksum = Sha256::digest(raw.as_slice());
    raw.extend_from_slice(checksum.as_slice());
    raw
}

fn alp_codec_rows(fixture: &AmountFixture, predicate: &str) -> Vec<Vec<Value>> {
    fixture
            .cassie
            .execute_sql(
                &fixture.session,
                &format!(
                    "SELECT amount FROM column_batch_alp_codec_version WHERE {predicate} ORDER BY amount"
                ),
                vec![],
            )
            .expect("query ALP codec-version fixture")
            .rows().to_vec()
}

fn fsst_codec_rows(fixture: &AmountFixture, table: &str, predicate: &str) -> Vec<Vec<Value>> {
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            &format!("SELECT amount FROM {table} WHERE {predicate} ORDER BY amount"),
            vec![],
        )
        .expect("query FSST fixture")
        .rows()
        .to_vec()
}

fn sum_amount(fixture: &AmountFixture, table: &str) -> Result<Vec<Vec<Value>>, String> {
    sum_amount_as(fixture, table, "total")
}

fn sum_amount_as(
    fixture: &AmountFixture,
    table: &str,
    alias: &str,
) -> Result<Vec<Vec<Value>>, String> {
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            &format!("SELECT SUM(amount) AS {alias} FROM {table}"),
            vec![],
        )
        .map(|result| result.rows().to_vec())
        .map_err(|error| error.to_string())
}

fn restart_fixture(fixture: AmountFixture) -> AmountFixture {
    let AmountFixture {
        path,
        cassie,
        session,
        collection,
        index,
    } = fixture;
    drop(session);
    drop(cassie);
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    cassie.startup().expect("reconcile column batches");
    let session = cassie.create_session("tester", None);
    AmountFixture {
        path,
        cassie,
        session,
        collection,
        index,
    }
}

include!("semantics.rs");

include!("restart.rs");

include!("parameters.rs");

include!("errors.rs");

include!("nulls.rs");
