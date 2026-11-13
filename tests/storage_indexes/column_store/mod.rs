//! Column-authoritative storage: no-row-blob invariant, corruption detection
//! and cross-mode transaction atomicity (issues #1107 and #1108).
//!
//! Every fixture is in-memory except the restart test, which needs disk.

use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::StorageFamily;

mod corruption;
mod no_row_blob;

pub struct Env {
    pub cassie: Cassie,
    pub session: CassieSession,
}

pub fn env() -> Env {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    Env { cassie, session }
}

impl Env {
    pub fn run(&self, sql: &str) {
        if let Err(error) = self.cassie.execute_sql(&self.session, sql, vec![]) {
            panic!("statement failed: {sql}: {error}");
        }
    }

    pub fn rows(&self, sql: &str) -> Vec<Vec<cassie::types::Value>> {
        match self.cassie.execute_sql(&self.session, sql, vec![]) {
            Ok(result) => result.rows().to_vec(),
            Err(error) => panic!("query failed: {sql}: {error}"),
        }
    }

    /// Every raw `\x18` entry of a column table, in key order.
    pub fn column_entries(&self, table: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
        let prefix = self
            .cassie
            .midge
            .column_store_prefix_for_diagnostics(table)
            .expect("column-store prefix");
        self.cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("raw column-store scan")
    }

    /// Raw keys under the row-blob and legacy-document prefixes of a table.
    pub fn row_store_keys(&self, table: &str) -> Vec<Vec<u8>> {
        self.cassie
            .midge
            .row_store_prefixes_for_diagnostics(table)
            .expect("row-store prefixes")
            .into_iter()
            .flat_map(|prefix| {
                self.cassie
                    .midge
                    .raw_scan_prefix(StorageFamily::Data, &prefix)
                    .expect("raw row-store scan")
            })
            .map(|(key, _)| key)
            .collect()
    }

    /// The raw key of `field_id` for `row_id`, built from the table's prefix.
    pub fn field_key(&self, table: &str, field_id: u32, row_id: &str) -> Vec<u8> {
        let mut key = self.column_prefix(table);
        key.extend_from_slice(b"f\0");
        key.extend_from_slice(&field_id.to_be_bytes());
        key.push(0);
        key.extend_from_slice(row_id.as_bytes());
        key
    }

    /// The raw live-marker key for `row_id`.
    pub fn marker_key(&self, table: &str, row_id: &str) -> Vec<u8> {
        let mut key = self.column_prefix(table);
        key.extend_from_slice(b"r\0");
        key.extend_from_slice(row_id.as_bytes());
        key
    }

    fn column_prefix(&self, table: &str) -> Vec<u8> {
        self.cassie
            .midge
            .column_store_prefix_for_diagnostics(table)
            .expect("column-store prefix")
    }

    /// Asserts the table holds no row blob, a marker per live row and one key
    /// per field per live row, and that the verifier agrees.
    pub fn assert_column_authority(
        &self,
        table: &str,
        live_rows: usize,
        fields: usize,
        step: &str,
    ) {
        let foreign = self.row_store_keys(table);
        let entries = self.column_entries(table);
        let markers = entries
            .iter()
            .filter(|(key, _)| key_has_discriminator(key, b'r'))
            .count();
        let field_keys = entries
            .iter()
            .filter(|(key, _)| key_has_discriminator(key, b'f'))
            .count();
        let violations = self
            .cassie
            .midge
            .verify_column_store_for_diagnostics(table)
            .expect("verifier");
        assert!(foreign.is_empty(), "{step}: row-blob or legacy keys exist");
        assert_eq!(markers, live_rows, "{step}: live markers");
        assert_eq!(field_keys, live_rows * fields, "{step}: field keys");
        assert!(violations.is_empty(), "{step}: {violations:?}");
    }
}

/// Whether the key's discriminator component (after the relation id) is `tag`.
fn key_has_discriminator(key: &[u8], tag: u8) -> bool {
    // cassie \0 \x01 \0 \x18 \0 <8-byte relation> \0 <tag> \0 ...
    let header = b"cassie\0\x01\0\x18\0".len();
    key.get(header + 8..header + 11)
        .is_some_and(|bytes| bytes == [0, tag, 0])
}
