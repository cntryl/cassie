use cassie::app::Cassie;

#[path = "support/executor.rs"]
mod support;
use support::{
    cassie_temp, create_text_collection, data_dir, put_document, put_fulltext_index, with_fallback,
};

#[test]
fn should_persist_generation_bound_fulltext_state() {
    // Arrange
    let cassie = cassie_temp("fulltext_persisted_retrieval");
    let collection = "fulltext_persisted_retrieval";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha alpha beta"}),
    );
    put_document(
        &cassie,
        collection,
        "d2",
        serde_json::json!({"body": "beta gamma"}),
    );

    // Act
    put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
    let state = cassie
        .midge
        .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
        .expect("read persisted fulltext state")
        .expect("state after index publication");

    // Assert
    assert_eq!(
        state.built_generation,
        cassie.midge.collection_generation(collection).unwrap()
    );
    assert_eq!(state.total_documents, 2);
    assert_eq!(state.documents_with_text, 2);
    assert_eq!(state.document_stats.get("d1").unwrap().doc_length, 3);
    assert_eq!(state.postings.get("alpha").unwrap()[0].term_frequency, 2);
}

#[test]
fn should_refresh_fulltext_postings_after_mutation() {
    // Arrange
    let cassie = cassie_temp("fulltext_persisted_mutation");
    let collection = "fulltext_persisted_mutation";
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha alpha beta"}),
    );
    put_document(
        &cassie,
        collection,
        "d2",
        serde_json::json!({"body": "beta gamma"}),
    );
    put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);

    // Act
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "delta"}),
    );
    cassie.midge.delete_document(collection, "d2").unwrap();
    let state = cassie
        .midge
        .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
        .unwrap()
        .unwrap();

    // Assert
    assert!(!state.postings.contains_key("alpha"));
    assert_eq!(state.document_stats.get("d1").unwrap().doc_length, 1);
    assert_eq!(state.total_documents, 1);
    assert!(!state.document_stats.contains_key("d2"));
}

#[test]
fn should_reload_persisted_fulltext_state_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_persisted_restart");
    let collection = "fulltext_persisted_restart";
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    create_text_collection(&cassie, collection, &["id", "body"]);
    put_document(
        &cassie,
        collection,
        "d1",
        serde_json::json!({"body": "alpha beta"}),
    );
    put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state(collection, "fulltext_body_idx")
        .unwrap()
        .unwrap();

    // Assert
    assert_eq!(state.total_documents, 1);
    assert_eq!(state.postings["alpha"][0].document_id, "d1");
}

#[test]
fn should_bound_persisted_term_candidate_reads() {
    // Arrange
    let cassie = cassie_temp("fulltext_bounded_candidates");
    let collection = "fulltext_bounded_candidates";
    create_text_collection(&cassie, collection, &["id", "body"]);
    for index in 0..64 {
        let body = if index < 2 {
            "alpha marker"
        } else {
            "unrelated"
        };
        put_document(
            &cassie,
            collection,
            &format!("d{index}"),
            serde_json::json!({"body": body}),
        );
    }
    put_fulltext_index(&cassie, collection, "fulltext_body_idx", "body", &[]);
    let before = cassie.metrics();

    // Act
    let candidates = cassie
        .midge
        .fulltext_candidate_stats(collection, "fulltext_body_idx", &["alpha".to_string()])
        .expect("candidate stats");
    let after = cassie.metrics();

    // Assert
    assert_eq!(candidates.len(), 2);
    assert!(candidates.contains_key("d0"));
    assert!(candidates.contains_key("d1"));
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(
        reads < 64,
        "expected bounded persisted reads, observed {reads}"
    );
}
