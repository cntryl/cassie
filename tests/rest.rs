use cassie::app::Cassie;
use cassie::rest::{collections, documents};

#[test]
fn should_crud_collection_documents_through_rest() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let cassie = Cassie::new().unwrap();
    let collection = "rest_docs";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let body = serde_json::json!({
            "name": collection,
            "fields": [
                {"name": "title", "type": "text"},
                {"name": "payload", "type": "json"},
                {"name": "embedding", "type": "vector(2)"},
            ]
        });

        // Act
        let create = collections::create(&cassie, body.to_string().as_bytes())
            .await
            .expect("create collection");
        let list = collections::list(&cassie).await;
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"title": "hello", "payload": {"k": 1}, "embedding": [1.0, 2.0]})
                .to_string()
                .as_bytes(),
        )
        .await
        .expect("create document");
        let doc_id = doc["id"].as_str().expect("id present");
        let got = documents::get(&cassie, collection, doc_id)
            .await
            .expect("get document");
        let removed = documents::delete(&cassie, collection, doc_id)
            .await
            .expect("delete document");

        // Assert
        assert_eq!(create["collection"], collection);
        assert!(list.contains(&collection.to_string()));
        assert_eq!(got["title"], "hello");
        assert_eq!(removed["deleted"], serde_json::Value::Bool(true));
    });
}

#[test]
fn should_reject_invalid_vector_dimensions_through_rest() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let cassie = Cassie::new().unwrap();
    let collection = "rest_bad_vector";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let _ = collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "embedding", "type": "vector(2)"}],
            })
            .to_string()
            .as_bytes(),
        )
        .await;

        // Act
        let insert = documents::create(
            &cassie,
            collection,
            serde_json::json!({"embedding": [1.0, 2.0, 3.0]})
                .to_string()
                .as_bytes(),
        )
        .await;

        // Assert
        assert!(insert.is_err(), "dimension mismatch should fail");
    });
}
