use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn data_dir(name: &str) -> PathBuf {
    crate::support_temp_dirs::sweep_stale_once();
    std::env::temp_dir().join(format!("cassie-idempotent-ddl-{name}-{}", Uuid::new_v4()))
}

#[test]
fn should_preserve_table_given_mismatched_if_not_exists_definition() {
    // Arrange
    let path = data_dir("table");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE stable_docs (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO stable_docs VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        let epoch = cassie.midge.schema_epoch().unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "CREATE TABLE IF NOT EXISTS stable_docs (different VECTOR(7) NOT NULL)",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(&session, "SELECT title FROM stable_docs", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command(), "CREATE TABLE");
        assert_eq!(cassie.midge.schema_epoch().unwrap(), epoch);
        assert_eq!(rows.rows(), vec![vec![Value::String("alpha".into())]]);
        assert!(cassie
            .execute_sql(&session, "CREATE TABLE stable_docs (id INT)", vec![])
            .is_err());
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_idempotent_ddl_given_repeated_create_index_if_not_exists() {
    // Arrange
    let path = data_dir("index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE indexed_docs (id INT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX stable_idx ON indexed_docs (title)",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("indexed_docs")
            .unwrap()
            .collection;
        let before = cassie.catalog.get_index(&collection, "stable_idx").unwrap();
        let epoch = cassie.midge.schema_epoch().unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_docs (score)",
                vec![],
            )
            .unwrap();
        let after = cassie.catalog.get_index(&collection, "stable_idx").unwrap();

        // Assert
        assert_eq!(result.command(), "CREATE INDEX");
        assert_eq!(cassie.midge.schema_epoch().unwrap(), epoch);
        assert_eq!(after.fields, before.fields);
        assert_eq!(after.unique, before.unique);
        assert!(cassie
            .execute_sql(
                &session,
                "CREATE INDEX stable_idx ON indexed_docs (score)",
                vec![]
            )
            .is_err());
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_validate_columns_before_skipping_an_existing_index() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "index-if-not-exists-validation",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE INDEX stable_idx ON indexed_rows (v)",
        ],
    );
    let before = fixture
        .cassie
        .catalog
        .get_index("indexed_rows", "stable_idx")
        .expect("index");
    let epoch = fixture.cassie.midge.schema_epoch().expect("schema epoch");

    // Act
    let key = fixture.execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing)");
    let include = fixture
        .execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) INCLUDE (missing)");
    let predicate = fixture
        .execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) WHERE missing = 1");
    let valid = fixture.execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v)");
    let valid_repeated_include =
        fixture.execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) INCLUDE (v)");

    // Assert
    for result in [key, include, predicate] {
        assert!(matches!(
            result.expect_err("missing column is validated"),
            cassie::app::CassieError::UndefinedColumn(_)
        ));
    }
    assert_eq!(
        valid.expect("valid duplicate is skipped").command(),
        "CREATE INDEX"
    );
    assert_eq!(
        valid_repeated_include
            .expect("duplicate key/include is skipped")
            .command(),
        "CREATE INDEX"
    );
    assert_eq!(
        fixture.cassie.midge.schema_epoch().expect("schema epoch"),
        epoch
    );
    let after = fixture
        .cassie
        .catalog
        .get_index("indexed_rows", "stable_idx")
        .expect("index survives");
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.unique, before.unique);
}

#[test]
fn should_preserve_missing_column_diagnostics_for_nonduplicate_indexes() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "index-nonduplicate-validation",
        &["CREATE TABLE indexed_rows (v INT)"],
    );

    // Act
    let error = fixture
        .execute("CREATE INDEX new_idx ON indexed_rows (missing)")
        .expect_err("missing key");

    // Assert
    assert!(
        matches!(&error, cassie::app::CassieError::UndefinedColumn(column)
        if column == "missing")
    );
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_report_predicate_then_key_then_include_for_invalid_duplicate_indexes() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "index-duplicate-validation-order",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE INDEX stable_idx ON indexed_rows (v)",
        ],
    );

    // Act
    let predicate = fixture.execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing_key) INCLUDE (missing_include) WHERE missing_predicate = 1").expect_err("missing predicate");
    let key = fixture.execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing_key) INCLUDE (missing_include)").expect_err("missing key");

    // Assert
    assert!(
        matches!(predicate, cassie::app::CassieError::UndefinedColumn(name) if name == "missing_predicate")
    );
    assert!(
        matches!(key, cassie::app::CassieError::UndefinedColumn(name) if name == "missing_key")
    );
}

#[test]
fn should_preserve_duplicate_index_skip_validation_order() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "index-duplicate-validation-fallback",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE INDEX stable_idx ON indexed_rows (v)",
        ],
    );

    // Act
    let options = fixture.execute(
        "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing) WITH (unknown_option = 1)",
    );
    let function = fixture.execute(
        "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing) WHERE abs(v) > 0",
    );
    let ill_typed = fixture
        .execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing) WHERE 1 + true");
    let present_predicate = fixture
        .execute("CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (missing) WHERE v = 1");
    let qualified_predicate = fixture.execute(
        "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) WHERE other.missing = 1",
    );

    // Assert
    options.expect("options remain outside the bounded increment");
    function.expect("function predicates remain outside the bounded increment");
    ill_typed.expect("predicate type checking remains outside the bounded increment");
    present_predicate.expect("key validation after a predicate remains pending");
    qualified_predicate.expect("qualified predicate resolution remains pending");
}

#[test]
fn should_report_missing_unqualified_predicate_columns_on_either_side() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "index-duplicate-validation-predicate-columns",
        &[
            "CREATE TABLE indexed_rows (v INT)",
            "CREATE INDEX stable_idx ON indexed_rows (v)",
        ],
    );

    // Act
    for sql in [
        "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) WHERE 1 = missing_right",
        "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_rows (v) WHERE \"MissingQuoted\" = 1",
    ] {
        let error = fixture.execute(sql).expect_err("missing predicate column");
        // Assert
        assert!(matches!(
            error,
            cassie::app::CassieError::UndefinedColumn(_)
        ));
    }
}
