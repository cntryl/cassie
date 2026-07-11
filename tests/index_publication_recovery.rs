use std::collections::BTreeMap;

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::midge::adapter::set_index_publication_failure_point;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, data_dir, openai_runtime_for_vectors, with_fallback};

static INDEX_PUBLICATION_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn should_replay_prepared_scalar_index_publication_after_restart() {
    // Arrange
    with_fallback();
    let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("index_publication_recovery");
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
                "CREATE TABLE index_publication_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO index_publication_docs (title) VALUES ('alpha')",
                vec![],
            )
            .expect("seed row");
        let collection = canonical_test_collection(&cassie, "index_publication_docs");
        let index = IndexMeta {
            collection: collection.clone(),
            name: "index_publication_docs_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: vec![],
            include_fields: vec![],
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        };

        // Act
        set_index_publication_failure_point(true);
        assert!(cassie.midge.put_index(&index).is_err());
        assert!(cassie
            .midge
            .get_index(&collection, &index.name)
            .expect("read unpublished index")
            .is_none());
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay prepared index");
        let restarted_session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title FROM index_publication_docs WHERE title = 'alpha'",
                vec![],
            )
            .expect("query after replay");

        // Assert
        assert!(restarted
            .midge
            .get_index(&collection, &index.name)
            .expect("read published index")
            .is_some());
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_hide_vector_index_until_prepared_publication_replays() {
    // Arrange
    with_fallback();
    let _failpoint_guard = INDEX_PUBLICATION_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("vector_index_publication_recovery");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors())
        .expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE vector_index_publication_docs (content TEXT, embedding VECTOR(1536))",
            vec![],
        )
        .expect("create table");

    // Act
    set_index_publication_failure_point(true);
    assert!(cassie
        .execute_sql(
            &session,
            "CREATE INDEX vector_index_publication_idx ON vector_index_publication_docs USING vector (embedding) WITH (source_field = content, metric = l2)",
            vec![],
        )
        .is_err());

    // Assert
    assert!(cassie
        .catalog
        .list_vector_indexes("vector_index_publication_docs")
        .is_empty());
    assert!(cassie
        .catalog
        .get_index(
            "vector_index_publication_docs",
            "vector_index_publication_idx"
        )
        .is_none());
    assert!(cassie
        .midge
        .get_vector_index("vector_index_publication_docs", "embedding")
        .expect("read prepared vector metadata")
        .is_some());

    drop(cassie);
    let restarted = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors())
        .expect("reopen Cassie");
    restarted.startup().expect("replay prepared vector index");
    assert_eq!(
        restarted
            .catalog
            .list_vector_indexes("vector_index_publication_docs")
            .len(),
        1
    );
    assert!(restarted
        .catalog
        .get_index(
            "vector_index_publication_docs",
            "vector_index_publication_idx"
        )
        .is_some());

    let _ = std::fs::remove_dir_all(path);
}
