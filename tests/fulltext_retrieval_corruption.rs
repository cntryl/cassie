use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/executor.rs"]
mod support;
use support::{cassie_temp, create_text_collection, put_document, put_fulltext_index};

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
}

fn corrupt_one_posting(cassie: &Cassie) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let (key, _) = entries
        .into_iter()
        .find(|(_, value)| {
            serde_json::from_slice::<Vec<serde_json::Value>>(value)
                .is_ok_and(|postings| !postings.is_empty())
        })
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

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM fulltext_retrieval_corruption WHERE search(body, 'alpha')",
            vec![],
        )
        .expect("query must use deterministic row fallback");

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0][0],
        cassie::types::Value::String("d1".to_string())
    );
}
