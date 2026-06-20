use cassie::app::Cassie;
use cassie::rest::{collections, documents};
use uuid::Uuid;

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
        let create =
            collections::create(&cassie, body.to_string().as_bytes()).expect("create collection");
        let list = collections::list(&cassie);
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"title": "hello", "payload": {"k": 1}, "embedding": [1.0, 2.0]})
                .to_string()
                .as_bytes(),
        )
        .expect("create document");
        let doc_id = doc["id"].as_str().expect("id present");
        let got = documents::get(&cassie, collection, doc_id).expect("get document");
        let removed = documents::delete(&cassie, collection, doc_id).expect("delete document");

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
        );

        // Act
        let insert = documents::create(
            &cassie,
            collection,
            serde_json::json!({"embedding": [1.0, 2.0, 3.0]})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(insert.is_err(), "dimension mismatch should fail");
    });
}

#[test]
fn should_reject_missing_document_lookup_through_rest() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let cassie = Cassie::new().unwrap();
    let collection = "rest_missing_doc";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let _ = collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "title", "type": "text"}],
            })
            .to_string()
            .as_bytes(),
        );

        // Act
        let missing = documents::get(&cassie, collection, "missing-id");
        // Assert
        assert!(missing.is_err(), "missing document should fail");
        let error = format!("{:?}", missing.unwrap_err());
        assert!(
            error.contains("document not found"),
            "unexpected error: {error}"
        );
    });
}

#[test]
fn should_apply_default_values_for_rest_ingest() {
    // Arrange
    let path = format!("/tmp/cassie-rest-default-{}", Uuid::new_v4());
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "rest_constraint_defaults";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("postgres", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

.unwrap();

        // Act
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1}).to_string().as_bytes(),
        )
        .expect("create rest document");
        let id = doc["id"].as_str().expect("id present");
        let stored = cassie
            .midge
            .get_document(collection, id)

            .expect("document read");

        // Assert
        let stored = stored.expect("document should be stored").payload;
        assert_eq!(stored["status"], "pending");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_ingest_when_not_null_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-not-null-{}", Uuid::new_v4());
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "rest_constraint_not_null";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("postgres", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_not_null (id INT PRIMARY KEY, email TEXT NOT NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let missing = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1}).to_string().as_bytes(),
        );

        // Assert
        assert!(
            missing.is_err(),
            "missing required field should be rejected"
        );
        let error = format!("{:?}", missing.unwrap_err());
        assert!(
            error.contains("cannot be null"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_ingest_when_unique_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-unique-{}", Uuid::new_v4());
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "rest_constraint_unique";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("postgres", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_unique (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
                vec![],
            )

.unwrap();

        documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        )
        .expect("first insert");

        // Act
        let duplicate = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 2, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(duplicate.is_err(), "duplicate unique field should be rejected");
        let error = format!("{:?}", duplicate.unwrap_err());
        assert!(
            error.contains("unique constraint failed"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_ingest_when_check_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-check-{}", Uuid::new_v4());
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "rest_constraint_check";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("postgres", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_check (id INT PRIMARY KEY, score INT CHECK (score >= 18))",
                vec![],
            )

.unwrap();

        // Act
        let invalid = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "score": 17})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(invalid.is_err(), "check constraint failure should be rejected");
        let error = format!("{:?}", invalid.unwrap_err());
        assert!(
            error.contains("check constraint failed"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
