use cassie::app::Cassie;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::path::PathBuf;
use uuid::Uuid;

fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-view-{label}-{}", Uuid::new_v4()))
}

async fn seed_view_docs(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .await
        .unwrap();
    cassie
        .register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        )
        .await;
    cassie
        .midge
        .put_document(
            collection,
            None,
            serde_json::json!({
                "title": "alpha",
                "score": 7
            }),
        )
        .await
        .unwrap();
}

#[test]
fn should_create_select_drop_user_defined_view() {
    // Arrange
    with_fallback();
    let path = data_dir("create_select_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "view_docs";
        seed_view_docs(&cassie, collection).await;

        let session = cassie.create_session("tester", None).await;

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW view_docs_ready AS SELECT title, score FROM view_docs",
                vec![],
            )
            .await
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM view_docs_ready WHERE score = 7",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(&session, "DROP VIEW view_docs_ready", vec![])
            .await
            .unwrap();
        let dropped = cassie
            .execute_sql(
                &session,
                "SELECT title FROM view_docs_ready",
                vec![],
            )
            .await;

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("alpha".to_string()),
                Value::Int64(7),
            ]]
        );
        assert!(dropped.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_select_from_nested_user_defined_views() {
    // Arrange
    with_fallback();
    let path = data_dir("nested");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "view_nested_docs";
        seed_view_docs(&cassie, collection).await;
        let session = cassie.create_session("tester", None).await;

        cassie
            .execute_sql(
                &session,
                "CREATE VIEW view_nested_inner AS SELECT title FROM view_nested_docs",
                vec![],
            )
            .await
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW view_nested_outer AS SELECT title FROM view_nested_inner",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM view_nested_outer WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_user_defined_views_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "view_restart_docs";
        seed_view_docs(&cassie, collection).await;
        let session = cassie.create_session("tester", None).await;

        cassie
            .execute_sql(
                &session,
                "CREATE VIEW view_restart_ready AS SELECT title, score FROM view_restart_docs",
                vec![],
            )
            .await
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();
        let session = restarted.create_session("tester", None).await;

        // Act
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT title, score FROM view_restart_ready WHERE score = 7",
                vec![],
            )
            .await
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(7)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_dml_against_user_defined_view() {
    // Arrange
    with_fallback();
    let path = data_dir("read_only");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "view_read_only_docs";
        seed_view_docs(&cassie, collection).await;
        let session = cassie.create_session("tester", None).await;

        cassie
            .execute_sql(
                &session,
                "CREATE VIEW view_read_only AS SELECT title, score FROM view_read_only_docs",
                vec![],
            )
            .await
            .unwrap();

        // Act
        let insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO view_read_only (title, score) VALUES ('beta', 9)",
                vec![],
            )
            .await;
        let update = cassie
            .execute_sql(
                &session,
                "UPDATE view_read_only SET score = 9",
                vec![],
            )
            .await;
        let delete = cassie
            .execute_sql(&session, "DELETE FROM view_read_only", vec![])
            .await;

        // Assert
        assert!(matches!(insert, Err(error) if error.to_string().contains("read-only")));
        assert!(matches!(update, Err(error) if error.to_string().contains("read-only")));
        assert!(matches!(delete, Err(error) if error.to_string().contains("read-only")));

        let _ = std::fs::remove_dir_all(path);
    });
}
