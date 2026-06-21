#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_execute_distinct_query() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE distinct_docs (category TEXT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO distinct_docs (category) VALUES ('b')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category FROM distinct_docs ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("a".to_string())],
                vec![Value::String("b".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_union_all_query() {
    // Arrange
    with_fallback();
    let path = data_dir("union_all_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_all_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE union_all_right (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_left (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_all_left UNION ALL SELECT title FROM union_all_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
                vec![Value::String("beta".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_union_query_with_deduplication() {
    // Arrange
    with_fallback();
    let path = data_dir("union_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_right (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_left (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_left UNION SELECT title FROM union_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_intersect_query() {
    // Arrange
    with_fallback();
    let path = data_dir("intersect_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE intersect_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE intersect_right (title TEXT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO intersect_left (title) VALUES ('alpha')",
            "INSERT INTO intersect_left (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM intersect_left INTERSECT SELECT title FROM intersect_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_except_query() {
    // Arrange
    with_fallback();
    let path = data_dir("except_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE except_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE except_right (title TEXT)", vec![])
            .unwrap();
        for sql in [
            "INSERT INTO except_left (title) VALUES ('alpha')",
            "INSERT INTO except_left (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM except_left EXCEPT SELECT title FROM except_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_distinct_on_query_with_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_on_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE distinct_on_docs (tenant_id TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'low', 1)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'high', 9)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('b', 'only', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM distinct_on_docs ORDER BY tenant_id ASC, score DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("high".to_string())
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("only".to_string())
                ]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_order_limit_offset_after_union_all() {
    // Arrange
    with_fallback();
    let path = data_dir("union_global_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_right (title TEXT)", vec![])
            .unwrap();
        for sql in [
            "INSERT INTO union_order_left (title) VALUES ('beta')",
            "INSERT INTO union_order_right (title) VALUES ('alpha')",
            "INSERT INTO union_order_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_order_left UNION ALL SELECT title FROM union_order_right ORDER BY title LIMIT 1 OFFSET 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_chained_union_all_query() {
    // Arrange
    with_fallback();
    let path = data_dir("union_all_chained");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for table in ["union_chain_a", "union_chain_b", "union_chain_c"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (title TEXT)"), vec![])
                .unwrap();
        }
        for sql in [
            "INSERT INTO union_chain_a (title) VALUES ('alpha')",
            "INSERT INTO union_chain_b (title) VALUES ('beta')",
            "INSERT INTO union_chain_c (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_chain_a UNION ALL SELECT title FROM union_chain_b UNION ALL SELECT title FROM union_chain_c ORDER BY title DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("gamma".to_string())],
                vec![Value::String("beta".to_string())],
                vec![Value::String("alpha".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
