use cassie::app::Cassie;
use cassie::embeddings::VectorIndexState;

#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, data_dir, with_fallback};

#[test]
fn should_reject_vector_state_from_an_older_collection_generation() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_state_generation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_state_docs (title TEXT, embedding VECTOR(3))",
                vec![],
            )
            .expect("create table");
        let collection = canonical_test_collection(&cassie, "vector_state_docs");
        cassie
            .midge
            .put_vector_index_state(&collection, "embedding", VectorIndexState::default())
            .expect("store current state");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO vector_state_docs (title) VALUES ('alpha')",
                vec![],
            )
            .expect("advance collection generation");

        // Assert
        assert!(cassie
            .midge
            .get_vector_index_state(&collection, "embedding")
            .expect("read state")
            .is_none());
    });

    let _ = std::fs::remove_dir_all(path);
}
