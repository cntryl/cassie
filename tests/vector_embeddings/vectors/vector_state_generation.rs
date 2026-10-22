use cassie::embeddings::VectorIndexState;

use super::support_sql as support;
use support::canonical_test_collection;

#[test]
fn should_reject_vector_state_from_an_older_collection_generation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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
}
