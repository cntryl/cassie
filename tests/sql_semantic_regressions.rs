use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

fn cassie_for(label: &str) -> (Cassie, cassie::app::CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

#[test]
fn should_evaluate_subtraction_chains_left_to_right() {
    // Arrange
    let (cassie, session, path) = cassie_for("left_associative_subtraction");

    // Act
    let result = cassie
        .execute_sql(&session, "SELECT 10 - 3 - 2 AS value", vec![])
        .expect("execute subtraction chain");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_evaluate_division_chains_left_to_right() {
    // Arrange
    let (cassie, session, path) = cassie_for("left_associative_division");

    // Act
    let result = cassie
        .execute_sql(&session, "SELECT 100.0 / 10 / 2 AS value", vec![])
        .expect("execute division chain");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_mixed_same_precedence_evaluation_order() {
    // Arrange
    let (cassie, session, path) = cassie_for("left_associative_mixed");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT 10 - 3 + 2 AS additive, 100 / 10 * 2 AS multiplicative",
            vec![],
        )
        .expect("execute mixed arithmetic chains");

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![Value::Float64(9.0), Value::Float64(20.0)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_round_trip_bigint_literals_exactly() {
    // Arrange
    let (cassie, session, path) = cassie_for("bigint_literal_round_trip");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bigint_literal_probe (wide BIGINT)",
            vec![],
        )
        .expect("create bigint probe");

    // Act
    cassie
        .execute_sql(
            &session,
            "INSERT INTO bigint_literal_probe VALUES (9007199254740993), (9223372036854775807)",
            vec![],
        )
        .expect("insert exact wide bigints");
    let above_f64_boundary = cassie
        .execute_sql(
            &session,
            "SELECT wide FROM bigint_literal_probe WHERE wide = 9007199254740993",
            vec![],
        )
        .expect("select bigint above exact float boundary");
    let maximum = cassie
        .execute_sql(
            &session,
            "SELECT wide FROM bigint_literal_probe WHERE wide = 9223372036854775807",
            vec![],
        )
        .expect("select maximum bigint");

    // Assert
    assert_eq!(
        above_f64_boundary.rows,
        vec![vec![Value::Int64(9_007_199_254_740_993)]]
    );
    assert_eq!(maximum.rows, vec![vec![Value::Int64(i64::MAX)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_round_trip_minimum_bigint_literal_exactly() {
    // Arrange
    let (cassie, session, path) = cassie_for("minimum_bigint_literal");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE minimum_bigint_probe (wide BIGINT)",
            vec![],
        )
        .expect("create bigint probe");

    // Act
    cassie
        .execute_sql(
            &session,
            "INSERT INTO minimum_bigint_probe VALUES (-9223372036854775808)",
            vec![],
        )
        .expect("insert minimum bigint");
    let result = cassie
        .execute_sql(
            &session,
            "SELECT wide FROM minimum_bigint_probe WHERE wide = -9223372036854775808",
            vec![],
        )
        .expect("select minimum bigint");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::Int64(i64::MIN)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_non_finite_float_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("non_finite_float_literal");

    // Act
    let literal_error = cassie
        .execute_sql(&session, "SELECT 1e400", vec![])
        .expect_err("reject non-finite float literal");
    let cast_error = cassie
        .execute_sql(&session, "SELECT CAST('1e400' AS FLOAT)", vec![])
        .expect_err("reject non-finite float cast");

    // Assert
    assert!(literal_error
        .to_string()
        .contains("numeric literal out of range"));
    assert!(cast_error
        .to_string()
        .contains("cannot cast value to FLOAT"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_out_of_range_integer_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("out_of_range_integer_literal");

    // Act
    let error = cassie
        .execute_sql(&session, "SELECT 9223372036854775808", vec![])
        .expect_err("reject integer outside bigint range");

    // Assert
    assert!(error.to_string().contains("numeric literal out of range"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_ungrouped_projection_column() {
    // Arrange
    let (cassie, session, path) = cassie_for("ungrouped_projection");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE grouped_projection_probe (grp TEXT, name TEXT, amount INT)",
            vec![],
        )
        .expect("create grouped projection probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT grp, name, SUM(amount) FROM grouped_projection_probe GROUP BY grp",
            vec![],
        )
        .expect_err("reject ungrouped projection");

    // Assert
    assert!(error.to_string().contains("must appear in the GROUP BY"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_ungrouped_column_inside_expression() {
    // Arrange
    let (cassie, session, path) = cassie_for("ungrouped_expression");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE grouped_expression_probe (grp TEXT, name TEXT, amount INT)",
            vec![],
        )
        .expect("create grouped expression probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT grp, upper(name), SUM(amount) FROM grouped_expression_probe GROUP BY grp",
            vec![],
        )
        .expect_err("reject ungrouped expression");

    // Assert
    assert!(error.to_string().contains("must appear in the GROUP BY"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_grouped_projection_columns() {
    // Arrange
    let (cassie, session, path) = cassie_for("valid_grouped_projection");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE valid_grouped_probe (grp TEXT, amount INT)",
            vec![],
        )
        .expect("create grouped projection probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO valid_grouped_probe VALUES ('a', 2), ('a', 3)",
            vec![],
        )
        .expect("seed grouped projection probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT grp, SUM(amount) FROM valid_grouped_probe GROUP BY grp",
            vec![],
        )
        .expect("execute valid grouped projection");

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![Value::String("a".to_string()), Value::Int64(5)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_ambiguous_unqualified_join_column() {
    // Arrange
    let (cassie, session, path) = cassie_for("ambiguous_join_column");
    cassie
        .execute_sql(&session, "CREATE TABLE amb_left (label TEXT)", vec![])
        .expect("create left relation");
    cassie
        .execute_sql(&session, "CREATE TABLE amb_right (label TEXT)", vec![])
        .expect("create right relation");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT label FROM amb_left JOIN amb_right ON true",
            vec![],
        )
        .expect_err("reject ambiguous column");

    // Assert
    assert!(error
        .to_string()
        .contains("column reference 'label' is ambiguous"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_qualified_join_column() {
    // Arrange
    let (cassie, session, path) = cassie_for("qualified_join_column");
    cassie
        .execute_sql(&session, "CREATE TABLE qual_left (label TEXT)", vec![])
        .expect("create left relation");
    cassie
        .execute_sql(&session, "CREATE TABLE qual_right (label TEXT)", vec![])
        .expect("create right relation");
    cassie
        .execute_sql(&session, "INSERT INTO qual_left VALUES ('left')", vec![])
        .expect("seed left relation");
    cassie
        .execute_sql(&session, "INSERT INTO qual_right VALUES ('right')", vec![])
        .expect("seed right relation");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT qual_left.label FROM qual_left JOIN qual_right ON true",
            vec![],
        )
        .expect("execute qualified projection");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("left".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compare_uuid_column_to_valid_text_literal() {
    // Arrange
    let (cassie, session, path) = cassie_for("uuid_text_comparison");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE uuid_probe (item_id TEXT, item_uuid UUID)",
            vec![],
        )
        .expect("create uuid probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO uuid_probe VALUES ('row-1', '550e8400-e29b-41d4-a716-446655440000')",
            vec![],
        )
        .expect("seed uuid probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM uuid_probe WHERE item_uuid = '550e8400-e29b-41d4-a716-446655440000'",
            vec![],
        )
        .expect("compare uuid to text literal");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("row-1".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_uuid_comparison_literal() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_uuid_comparison");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_uuid_probe (item_uuid UUID)",
            vec![],
        )
        .expect("create uuid probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT item_uuid FROM malformed_uuid_probe WHERE item_uuid = 'not-a-uuid'",
            vec![],
        )
        .expect_err("reject malformed uuid literal");

    // Assert
    assert!(error.to_string().contains("invalid UUID literal"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compare_bytea_column_to_valid_text_literal() {
    // Arrange
    let (cassie, session, path) = cassie_for("bytea_text_comparison");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bytea_probe (item_id TEXT, payload BYTEA)",
            vec![],
        )
        .expect("create bytea probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO bytea_probe VALUES ('row-1', '\\x01020a')",
            vec![],
        )
        .expect("seed bytea probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM bytea_probe WHERE payload = '\\x01020a'",
            vec![],
        )
        .expect("compare bytea to text literal");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("row-1".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_bytea_comparison_literal() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_bytea_comparison");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_bytea_probe (payload BYTEA)",
            vec![],
        )
        .expect("create bytea probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM malformed_bytea_probe WHERE payload = '\\xzz'",
            vec![],
        )
        .expect_err("reject malformed bytea literal");

    // Assert
    assert!(error.to_string().contains("invalid BYTEA literal"));
    let _ = std::fs::remove_dir_all(path);
}
