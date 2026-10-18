use cassie::types::Value;

use super::ddl_check_literals::{open, rows};

#[test]
fn should_evaluate_volatile_function_defaults_per_row() {
    // Arrange
    let (cassie, session, path) = open(
            "default_volatile_functions",
            &[
                "CREATE TABLE default_functions (id INT NOT NULL, created_at TIMESTAMP DEFAULT now(), updated_at TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, day DATE DEFAULT CURRENT_DATE, uid UUID DEFAULT gen_random_uuid())",
                "INSERT INTO default_functions (id) VALUES (1)",
                "INSERT INTO default_functions (id) VALUES (2)",
            ],
        );

    // Act
    let stored = rows(
        &cassie,
        &session,
        "SELECT created_at, updated_at, day, uid FROM default_functions ORDER BY id",
    );
    let past_rows = rows(
            &cassie,
            &session,
            "SELECT id FROM default_functions WHERE created_at > '2020-01-01T00:00:00Z' AND day > '2020-01-01' ORDER BY id",
        );

    // Assert
    assert_eq!(stored.len(), 2);
    for row in &stored {
        assert!(
            row.iter().all(|value| !matches!(value, Value::Null)),
            "every function default must produce a value"
        );
        assert!(
            !row.iter().any(|value| matches!(
                value,
                Value::String(text) if text.contains("()") || text.contains("CURRENT_")
            )),
            "a function default must not be stored as its own source text"
        );
    }
    assert_ne!(stored[0][3], stored[1][3], "gen_random_uuid() runs per row");
    assert_eq!(
        past_rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_default_expressions_the_engine_cannot_evaluate() {
    // Arrange
    let (cassie, session, path) = open(
        "default_unsupported_expressions",
        &["CREATE TABLE default_alter_target (a INT, n INT)"],
    );

    // Act
    let unsupported_function = cassie.execute_sql(
        &session,
        "CREATE TABLE default_abs (a INT, n INT DEFAULT abs(-3))",
        vec![],
    );
    let wrong_type = cassie.execute_sql(
        &session,
        "CREATE TABLE default_wrong_type (a INT, n INT DEFAULT now())",
        vec![],
    );
    let text_function = cassie.execute_sql(
        &session,
        "CREATE TABLE default_text_function (a INT, label TEXT DEFAULT upper('x'))",
        vec![],
    );
    let set_default = cassie.execute_sql(
        &session,
        "ALTER TABLE default_alter_target ALTER COLUMN n SET DEFAULT abs(-3)",
        vec![],
    );
    let add_column = cassie.execute_sql(
        &session,
        "ALTER TABLE default_alter_target ADD COLUMN m INT DEFAULT now()",
        vec![],
    );
    cassie
        .execute_sql(
            &session,
            "INSERT INTO default_alter_target (a) VALUES (1)",
            vec![],
        )
        .expect("table stays insertable");

    // Assert
    assert!(unsupported_function.is_err(), "abs(-3) must be rejected");
    assert!(wrong_type.is_err(), "now() cannot default an INT column");
    assert!(text_function.is_err(), "upper('x') must be rejected");
    assert!(set_default.is_err(), "SET DEFAULT abs(-3) must be rejected");
    assert!(add_column.is_err(), "ADD COLUMN ... DEFAULT now() on INT");
    assert!(!cassie.catalog.exists("default_abs"));
    assert!(!cassie.catalog.exists("default_wrong_type"));
    assert!(!cassie.catalog.exists("default_text_function"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_wrapped_default_constants() {
    // Arrange
    let (cassie, session, path) = open(
            "default_cast_constants",
            &[
                "CREATE TABLE default_casts (id INT NOT NULL, n INT DEFAULT (0), s VARCHAR(10) DEFAULT 'draft'::varchar, doc JSONB DEFAULT '{}'::jsonb)",
                "INSERT INTO default_casts (id) VALUES (1)",
                "INSERT INTO default_casts (id, n, s, doc) VALUES (2, 0, 'draft', '{}')",
            ],
        );

    // Act
    let stored = rows(
        &cassie,
        &session,
        "SELECT n, s, doc FROM default_casts ORDER BY id",
    );

    // Assert
    assert_eq!(stored.len(), 2);
    assert_eq!(
        stored[0], stored[1],
        "defaults must equal the explicit constants"
    );
    assert_eq!(stored[0][1], Value::String("draft".to_string()));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_coerce_quoted_boolean_defaults_to_boolean() {
    // Arrange
    let (cassie, session, path) = open(
            "default_boolean_literals",
            &[
                "CREATE TABLE default_flags (id INT NOT NULL, a BOOLEAN DEFAULT 'true', b BOOLEAN NOT NULL DEFAULT 'f', c BOOLEAN DEFAULT true)",
                "ALTER TABLE default_flags ALTER COLUMN c SET DEFAULT 'no'",
                "INSERT INTO default_flags (id) VALUES (1)",
            ],
        );

    // Act
    let stored = rows(&cassie, &session, "SELECT a, b, c FROM default_flags");
    let numeric_default = cassie.execute_sql(
        &session,
        "CREATE TABLE default_flag_number (id INT NOT NULL, flag BOOLEAN DEFAULT 1)",
        vec![],
    );
    let invalid_spelling = cassie.execute_sql(
        &session,
        "CREATE TABLE default_flag_word (id INT NOT NULL, flag BOOLEAN DEFAULT 'maybe')",
        vec![],
    );

    // Assert
    assert_eq!(
        stored,
        vec![vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]]
    );
    assert!(
        numeric_default.is_err(),
        "an integer default for a boolean column must be rejected"
    );
    assert!(
        invalid_spelling.is_err(),
        "'maybe' is not a boolean literal"
    );
    let _ = std::fs::remove_dir_all(path);
}
