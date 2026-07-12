#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::app::CassieSession;
use cassie::midge::adapter::StorageFamily;
use cntryl_lexkey::LexKey;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_pack_internal_key_segments_with_compact_markers() {
    // Arrange
    with_fallback();
    let path = data_dir("key_segment_compaction");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        // Act
        populate_compaction_fixtures(&cassie, &session);
        // Assert
        assert_key_encoding_marker_compaction(
            &cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"")
                .unwrap(),
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

fn populate_compaction_fixtures(cassie: &Cassie, session: &CassieSession) {
    execute_all_queries(cassie, session, SCALAR_INDEX_QUERIES);
    execute_all_queries(cassie, session, GRAPH_QUERIES);
}

fn execute_all_queries(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for statement in statements {
        let _ = cassie.execute_sql(session, statement, vec![]).unwrap();
    }
}

fn assert_key_encoding_marker_compaction(data_keys: &[(Vec<u8>, Vec<u8>)]) {
    assert!(data_keys
        .iter()
        .any(|(key, _)| key_family(key) == Some("scalar-index")));
    assert!(data_keys
        .iter()
        .any(|(key, _)| key_family(key) == Some("time-series-index")));
    assert!(data_keys
        .iter()
        .any(|(key, _)| key_family(key) == Some("column-batch")));
    assert!(data_keys
        .iter()
        .any(|(key, _)| key_family(key) == Some("column-store")));
    assert!(data_keys
        .iter()
        .any(|(key, _)| key_family(key) == Some("graph-adjacency")));

    let scalar_checks = data_keys
        .iter()
        .filter(|(key, _)| key_family(key) == Some("scalar-index"))
        .all(|(key, _)| !has_key_component(key, b"data"));
    let time_series_checks = data_keys
        .iter()
        .filter(|(key, _)| key_family(key) == Some("time-series-index"))
        .all(|(key, _)| !has_key_component(key, b"data"));
    let batch_checks = data_keys
        .iter()
        .filter(|(key, _)| key_family(key) == Some("column-batch"))
        .all(|(key, _)| {
            !has_key_component(key, b"metadata") && !has_key_component(key, b"segment")
        });
    let store_checks = data_keys
        .iter()
        .filter(|(key, _)| key_family(key) == Some("column-store"))
        .all(|(key, _)| {
            !has_key_component(key, b"row")
                && !has_key_component(key, b"deleted")
                && !has_key_component(key, b"field")
        });
    let graph_checks = data_keys
        .iter()
        .filter(|(key, _)| key_family(key) == Some("graph-adjacency"))
        .all(|(key, _)| !has_key_component(key, b"out") && !has_key_component(key, b"in"));
    assert!(scalar_checks);
    assert!(time_series_checks);
    assert!(batch_checks);
    assert!(store_checks);
    assert!(graph_checks);
}

fn key_family(raw: &[u8]) -> Option<&str> {
    raw.split(|byte| *byte == LexKey::SEPARATOR)
        .nth(3)
        .and_then(|component| std::str::from_utf8(component).ok())
}

fn has_key_component(raw: &[u8], target: &[u8]) -> bool {
    raw.split(|byte| *byte == LexKey::SEPARATOR)
        .any(|part| part == target)
}

const SCALAR_INDEX_QUERIES: &[&str] = &[
    "CREATE TABLE keyseg_row (tenant TEXT, event_at TIMESTAMP, title TEXT, email TEXT)",
    "INSERT INTO keyseg_row (tenant, event_at, title, email) VALUES ('acme', '2026-01-01T00:00:00Z', 'alpha', 'a@example.com')",
    "CREATE INDEX keyseg_scalar_idx ON keyseg_row USING btree (email)",
    "CREATE UNIQUE INDEX keyseg_unique_tenant_idx ON keyseg_row USING btree (tenant)",
    "CREATE INDEX keyseg_time_series_idx ON keyseg_row USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
    "CREATE INDEX keyseg_column_idx ON keyseg_row USING column (title) WITH (segment_size = 1)",
    "SELECT title FROM keyseg_row WHERE title = 'alpha'",
    "CREATE TABLE keyseg_column_store (k TEXT, payload TEXT) WITH (storage = column_store)",
    "INSERT INTO keyseg_column_store (k, payload) VALUES ('r1', 'keep'), ('r2', 'delete')",
    "SELECT payload FROM keyseg_column_store WHERE k = 'r1'",
    "DELETE FROM keyseg_column_store WHERE k = 'r2'",
];

const GRAPH_QUERIES: &[&str] = &[
    "CREATE GRAPH keyseg_graph (NODES (label TEXT), EDGES (source TEXT))",
    "INSERT INTO keyseg_graph_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice')",
    "INSERT INTO keyseg_graph_nodes (node_type, node_id, label) VALUES ('person', 'bob', 'Bob')",
    "INSERT INTO keyseg_graph_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
];
