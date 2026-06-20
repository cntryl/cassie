use cassie::app::Cassie;
use cassie::types::{DataType, Value};
use std::env;
use uuid::Uuid;

fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-probes-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

#[test]
fn should_return_version_function() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("version");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT version()", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "version");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0][0],
            Value::String(env!("CARGO_PKG_VERSION").to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_current_schema_function() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("schema");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT current_schema()", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "current_schema");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("public".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_current_database_function() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("database");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT current_database()", vec![])
            .await
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "current_database");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("catalogdb".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_search_path_from_show_statement() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("show_search_path");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SHOW search_path", vec![])
            .await;

        // Assert
        let result = result.unwrap();
        assert_eq!(
            result.columns,
            vec![cassie::executor::ColumnMeta {
                name: "search_path".to_string(),
                data_type: "text".to_string(),
                type_oid: DataType::Text.type_oid(),
                typlen: DataType::Text.typlen(),
                atttypmod: DataType::Text.atttypmod(),
                format_code: 0,
                nullable: true,
            }]
        );
        assert_eq!(result.rows, vec![vec![Value::String("public".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_treat_supported_set_statement_as_noop() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("set_supported");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SET search_path = public", vec![])
            .await;

        // Assert
        let result = result.unwrap();
        assert_eq!(result.command, "SET");
        assert!(result.columns.is_empty());
        assert!(result.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsupported_show_variable() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("show_unsupported");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SHOW unsupported_metadata", vec![])
            .await;

        // Assert
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsupported_set_variable() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("set_unsupported");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));

        // Act
        let result = cassie
            .execute_sql(&session, "SET unsupported_variable = foo", vec![])
            .await;

        // Assert
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(path);
    });
}
