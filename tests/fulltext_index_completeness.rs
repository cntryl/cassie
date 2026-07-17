use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, DEFAULT_SCHEMA};
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/executor.rs"]
mod support;
use support::{
    create_text_collection, data_dir, fulltext_index, put_document, put_fulltext_index,
    with_fallback,
};

const COLLECTION: &str = "fulltext_index_completeness";
const INDEX: &str = "fulltext_body_idx";

fn seed_index(path: &str, documents: impl IntoIterator<Item = (String, String)>) -> Cassie {
    let cassie = Cassie::new_with_data_dir(path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = canonical_relation_name("postgres", DEFAULT_SCHEMA, COLLECTION);
    create_text_collection(&cassie, &collection, &["id", "body"]);
    for (id, body) in documents {
        put_document(&cassie, &collection, &id, serde_json::json!({"body": body}));
    }
    put_fulltext_index(&cassie, &collection, INDEX, "body", &[]);
    cassie
        .catalog
        .register_index(fulltext_index(&collection, INDEX, "body", &[]));
    cassie
}

fn artifacts_with_magic(cassie: &Cassie, magic: &[u8; 4]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let prefix = cassie
        .midge
        .fulltext_artifact_prefix_for_diagnostics(COLLECTION, INDEX)
        .expect("fulltext artifact prefix");
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("fulltext artifacts")
        .into_iter()
        .filter(|(_, value)| value.starts_with(magic))
        .collect()
}

fn delete_data_key(cassie: &Cassie, key: Vec<u8>) {
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    tx.delete(key).expect("delete data key");
    tx.commit(WriteOptions::sync()).expect("commit deletion");
}

fn replace_artifact(cassie: &Cassie, magic: &[u8; 4], replacement: Vec<u8>) {
    let (key, _) = artifacts_with_magic(cassie, magic)
        .into_iter()
        .next()
        .expect("artifact record");
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    tx.put(key, replacement, None).expect("replace artifact");
    tx.commit(WriteOptions::sync())
        .expect("commit artifact replacement");
}

fn search(cassie: &Cassie, sql: &str) -> cassie::executor::QueryResult {
    cassie
        .execute_sql(&cassie.create_session("tester", None), sql, vec![])
        .expect("fulltext query")
}

fn fallback_count(cassie: &Cassie, reason: &str) -> u64 {
    cassie.metrics()["search"]["retrieval_fallback_reasons"][reason]
        .as_u64()
        .unwrap_or_default()
}

fn long_document_id(index: usize) -> String {
    format!("{index:03}-{}", format!("{index:03}").repeat(300))
}

#[test]
fn should_fallback_when_one_posting_block_is_missing_among_valid_blocks() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_missing_posting_block");
    let documents = (0..96).map(|index| (long_document_id(index), "alpha".to_string()));
    let cassie = seed_index(&path, documents);
    let blocks = artifacts_with_magic(&cassie, b"FTB1");
    assert!(blocks.len() > 1, "fixture must span posting blocks");
    delete_data_key(&cassie, blocks[1].0.clone());
    let before = fallback_count(&cassie, "invalid_persisted_artifact");

    // Act
    let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 100",
    );

    // Assert
    assert_eq!(result.rows.len(), 96);
    assert!(fallback_count(&cassie, "invalid_persisted_artifact") > before);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_when_a_candidate_is_missing_document_statistics() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_missing_document_stats");
    let cassie = seed_index(
        &path,
        [
            ("d1".to_string(), "alpha".to_string()),
            ("d2".to_string(), "alpha".to_string()),
        ],
    );
    let stats = artifacts_with_magic(&cassie, b"FTD1");
    delete_data_key(&cassie, stats[0].0.clone());
    let before = fallback_count(&cassie, "invalid_persisted_artifact");

    // Act
    let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10",
    );

    // Assert
    assert_eq!(result.rows.len(), 2);
    assert!(fallback_count(&cassie, "invalid_persisted_artifact") > before);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_before_top_k_publishes_a_dangling_candidate() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_dangling_candidate");
    let cassie = seed_index(
        &path,
        [
            ("d1".to_string(), "alpha".to_string()),
            ("d2".to_string(), "alpha".to_string()),
        ],
    );
    let source_key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("data records")
        .into_iter()
        .find(|(_, value)| value.starts_with(b"CRB2"))
        .map(|(key, _)| key)
        .expect("source row");
    delete_data_key(&cassie, source_key);
    let before = fallback_count(&cassie, "missing_candidate_row");

    // Act
    let result = search(
        &cassie,
        "SELECT id, search_score(body, 'alpha') AS score FROM fulltext_index_completeness WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 10",
    );

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert!(fallback_count(&cassie, "missing_candidate_row") > before);
    assert!(matches!(result.rows[0][0], Value::String(_)));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_a_malformed_manifest_during_startup() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_malformed_manifest_restart");
    let cassie = seed_index(&path, [("d1".to_string(), "alpha beta".to_string())]);
    replace_artifact(&cassie, b"FTG1", b"malformed-manifest".to_vec());
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("reconcile fulltext sidecar");
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("read reconciled fulltext state")
        .expect("fulltext state");

    // Assert
    assert_eq!(state.total_documents, 1);
    assert_eq!(state.postings["alpha"].len(), 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_old_fulltext_metadata_during_startup() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_old_metadata_restart");
    let cassie = seed_index(&path, [("d1".to_string(), "alpha beta".to_string())]);
    let (_, mut metadata) = artifacts_with_magic(&cassie, b"FTM1")
        .into_iter()
        .next()
        .expect("fulltext metadata");
    metadata[4] = 1;
    replace_artifact(&cassie, b"FTM1", metadata);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("upgrade fulltext sidecar");
    let metadata = artifacts_with_magic(&restarted, b"FTM1")
        .into_iter()
        .next()
        .map(|(_, value)| value)
        .expect("upgraded fulltext metadata");

    // Assert
    assert_eq!(metadata[4], 2, "startup must publish only v2 metadata");
    assert!(restarted
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("read upgraded fulltext state")
        .is_some());
    let _ = std::fs::remove_dir_all(path);
}
