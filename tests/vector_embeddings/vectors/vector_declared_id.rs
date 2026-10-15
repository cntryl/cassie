use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

use support::{data_dir, use_local_storage};

#[test]
fn should_return_declared_id_from_vector_distance_top_k() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_declared_id_topk");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE vector_declared_docs (id INT, embedding VECTOR(2))",
            vec![],
        )
        .expect("create table");
    for (key, id, embedding) in [("a", 7, [1.0, 0.0]), ("b", 8, [0.0, 1.0])] {
        cassie
            .midge
            .put_document(
                "vector_declared_docs",
                Some(key.to_string()),
                serde_json::json!({"id": id, "embedding": embedding}),
            )
            .expect("put vector row");
    }

    // Act
    let declared = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM vector_declared_docs ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .expect("vector top-k with declared id");
    let identity = cassie
            .execute_sql(
                &session,
                "SELECT _id, vector_distance(embedding, '[1,0]') AS distance FROM vector_declared_docs ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .expect("vector top-k with row identity");

    // Assert
    assert_eq!(declared.rows()[0][0], Value::Int64(7));
    assert_eq!(identity.rows()[0][0], Value::String("a".to_string()));

    let _ = std::fs::remove_dir_all(path);
}
