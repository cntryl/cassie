use cassie::app::Cassie;
use cassie::rest;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_reject_family_specific_rest_vector_index_options() {
    // Arrange
    with_fallback();
    let path = data_dir("rest_vector_index_family_options");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_vector_index_family_options (content TEXT, embedding VECTOR(1536))",
            vec![],
        )
        .unwrap();
    let cases = [
        (
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "index_type": "bruteforce",
                    "m": "12"
                }
            }),
            "vector index option 'm' requires index_type 'hnsw'",
        ),
        (
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "index_type": "hnsw",
                    "lists": "2"
                }
            }),
            "vector index option 'lists' requires index_type 'ivfflat'",
        ),
        (
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "index_type": "ivfflat",
                    "ef_search": "64"
                }
            }),
            "vector index option 'ef_search' requires index_type 'hnsw'",
        ),
    ];

    for (body, expected) in cases {
        // Act
        let error = rest::indexes::create(
            &cassie,
            "rest_vector_index_family_options",
            body.to_string().as_bytes(),
        )
        .expect_err("wrong-family vector option should fail");

        // Assert
        assert!(
            error.to_string().contains(expected),
            "expected '{expected}' in {error}"
        );
    }

    let _ = std::fs::remove_dir_all(path);
}
