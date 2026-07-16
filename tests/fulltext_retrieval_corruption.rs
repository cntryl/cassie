use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/executor.rs"]
mod support;
use support::{
    cassie_temp, create_text_collection, fulltext_index, put_document, put_fulltext_index,
};

fn seed_corruptible_index() -> Cassie {
    let cassie = cassie_temp("fulltext_retrieval_corruption");
    let collection = "fulltext_retrieval_corruption";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha beta"}),
    );
    put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
    cassie
        .catalog
        .register_index(fulltext_index(collection, "fulltext_body_idx", "body", &[]));
    cassie
}

fn corrupt_one_posting(cassie: &Cassie) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let (key, _) = entries
        .into_iter()
        .find(|(_, value)| value.starts_with(b"FTB1"))
        .expect("persisted posting");
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(key, b"corrupt-posting".to_vec(), None).unwrap();
    tx.commit(WriteOptions::sync()).unwrap();
}

#[test]
fn should_reject_corrupt_persisted_fulltext_postings() {
    // Arrange
    let cassie = seed_corruptible_index();
    corrupt_one_posting(&cassie);

    // Act
    let error = cassie
        .midge
        .get_persisted_fulltext_index_state("fulltext_retrieval_corruption", "fulltext_body_idx")
        .expect_err("corrupt posting must be rejected");

    // Assert
    assert!(error.to_string().contains("invalid fulltext posting"));
}

#[test]
fn should_fallback_to_rows_when_persisted_fulltext_postings_are_corrupt() {
    // Arrange
    let cassie = seed_corruptible_index();
    corrupt_one_posting(&cassie);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, search_score(body, $1) AS score FROM fulltext_retrieval_corruption WHERE search(body, $1)",
            vec![Value::String("alpha".to_string())],
        )
        .expect("query must use deterministic row fallback");

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0][0],
        cassie::types::Value::String("d1".to_string())
    );
    let after = cassie.metrics();
    assert!(
        after["search"]["row_scan_fallback_total"].as_u64().unwrap()
            > before["search"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap()
    );
    assert!(
        after["search"]["retrieval_fallback_reasons"]["invalid_persisted_artifact"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
}
