use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

use support::data_dir;

fn cassie_for(label: &str) -> (Cassie, cassie::app::CassieSession, String) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

#[test]
fn should_parse_keyword_prefix_identifiers_as_columns() {
    // Arrange
    let (cassie, session, path) = cassie_for("keyword_prefix_identifiers");
    cassie
            .execute_sql(
                &session,
                "CREATE TABLE keyword_prefix_columns (id INT, _deleted BOOLEAN, not_deleted BOOLEAN, case_id INT, exists_flag BOOLEAN, cast_value INT, unique_key TEXT, primary_contact TEXT, check_sum INT, foreign_id TEXT, constraint_name TEXT)",
                vec![],
            )
            .expect("keyword-prefixed column names should be valid identifiers");
    cassie
            .execute_sql(
                &session,
                "INSERT INTO keyword_prefix_columns VALUES (1, true, true, 11, true, 12, 'unique', 'primary', 13, 'foreign', 'constraint'), (2, false, false, 22, false, 23, 'key', 'contact', 24, 'id', 'name')",
                vec![],
            )
            .expect("insert keyword-prefixed values");

    // Act
    let selected = cassie
            .execute_sql(
                &session,
                "SELECT id, _deleted, not_deleted, case_id, exists_flag, cast_value, unique_key, primary_contact, check_sum, foreign_id, constraint_name FROM keyword_prefix_columns ORDER BY id",
                vec![],
            )
            .expect("select keyword-prefixed columns");
    let matching = cassie
        .execute_sql(
            &session,
            "SELECT id FROM keyword_prefix_columns WHERE not_deleted = true",
            vec![],
        )
        .expect("filter by keyword-prefixed identifier");

    // Assert
    assert_eq!(selected.rows().len(), 2);
    assert_eq!(selected.rows()[0][0], Value::Int64(1));
    assert_eq!(selected.rows()[0][1], Value::Bool(true));
    assert_eq!(selected.rows()[0][2], Value::Bool(true));
    assert_eq!(selected.rows()[0][3], Value::Int64(11));
    assert_eq!(selected.rows()[0][4], Value::Bool(true));
    assert_eq!(selected.rows()[0][5], Value::Int64(12));
    assert_eq!(selected.rows()[0][6], Value::String("unique".to_string()));
    assert_eq!(selected.rows()[0][7], Value::String("primary".to_string()));
    assert_eq!(selected.rows()[0][8], Value::Int64(13));
    assert_eq!(selected.rows()[0][9], Value::String("foreign".to_string()));
    assert_eq!(
        selected.rows()[0][10],
        Value::String("constraint".to_string())
    );
    assert_eq!(selected.rows()[1][2], Value::Bool(false));
    assert_eq!(matching.rows(), vec![vec![Value::Int64(1)]]);
    let _ = std::fs::remove_dir_all(path);
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
    assert_eq!(result.rows(), vec![vec![Value::Int64(5)]]);
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
    // 100.0 is numeric, so the chain divides with numeric result scales.
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("5.0000000000000000".to_owned())]]
    );
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
    assert_eq!(result.rows(), vec![vec![Value::Int64(9), Value::Int64(20)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_bigint_scalar_arithmetic_precision() {
    // Arrange
    let (cassie, session, path) = cassie_for("bigint_scalar_arithmetic");

    // Act
    let exact = cassie
            .execute_sql(
                &session,
                "SELECT 9007199254740993 + 9007199254740993 AS added, 9007199254740995 - 9007199254740993 AS subtracted, CAST(3002399751580331 AS BIGINT) * CAST(3 AS BIGINT) AS multiplied, 1 + 1.5 AS mixed",
                vec![],
            )
            .expect("execute exact bigint arithmetic");
    // Assert
    assert_eq!(
        exact.rows(),
        vec![vec![
            Value::Int64(18_014_398_509_481_986),
            Value::Int64(2),
            Value::Int64(9_007_199_254_740_993),
            // 1 + 1.5 is numeric in PostgreSQL 18.6.
            Value::String("2.5".to_owned()),
        ]]
    );
    assert_eq!(
        exact
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20, 20, 1700]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_integer_column_arithmetic_exact_when_combined_with_a_bare_literal() {
    // Arrange
    let (cassie, session, path) = cassie_for("integer_literal_arithmetic");
    cassie
        .execute_sql(&session, "CREATE TABLE t (n INT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(&session, "INSERT INTO t (n) VALUES (1)", vec![])
        .expect("insert row");

    // Act
    let column_plus_literal = cassie
        .execute_sql(&session, "SELECT n + 1 FROM t", vec![])
        .expect("execute column plus literal");
    let literal_plus_column = cassie
        .execute_sql(&session, "SELECT 1 + n FROM t", vec![])
        .expect("execute literal plus column");
    let column_plus_column = cassie
        .execute_sql(&session, "SELECT n + n FROM t", vec![])
        .expect("execute column plus column");
    let pure_literal = cassie
        .execute_sql(&session, "SELECT 1 + 2 AS total", vec![])
        .expect("execute pure literal arithmetic");

    // Assert
    assert_eq!(column_plus_literal.rows(), vec![vec![Value::Int64(2)]]);
    assert_eq!(column_plus_literal.columns()[0].type_oid, 20);
    assert_eq!(literal_plus_column.rows(), vec![vec![Value::Int64(2)]]);
    assert_eq!(literal_plus_column.columns()[0].type_oid, 20);
    assert_eq!(column_plus_column.rows(), vec![vec![Value::Int64(2)]]);
    assert_eq!(column_plus_column.columns()[0].type_oid, 20);
    // Integer literals are integers in PostgreSQL, so literal-only
    // arithmetic stays exact (see
    // should_execute_table_free_literal_alias_cast_projection).
    assert_eq!(pure_literal.rows(), vec![vec![Value::Int64(3)]]);
    assert_eq!(pure_literal.columns()[0].type_oid, 20);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_bigint_scalar_arithmetic_overflow() {
    // Arrange
    let (cassie, session, path) = cassie_for("bigint_scalar_overflow");

    // Act
    let overflow_errors = [
        "SELECT 9223372036854775807 + 9007199254740993",
        "SELECT -9223372036854775808 - 9007199254740993",
        "SELECT 9007199254740993 * 9007199254740993",
    ]
    .map(|sql| {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect_err("reject bigint overflow")
    });

    // Assert
    for error in overflow_errors {
        assert!(error.to_string().contains("bigint out of range"), "{error}");
    }
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
        above_f64_boundary.rows(),
        vec![vec![Value::Int64(9_007_199_254_740_993)]]
    );
    assert_eq!(maximum.rows(), vec![vec![Value::Int64(i64::MAX)]]);
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
    assert_eq!(result.rows(), vec![vec![Value::Int64(i64::MIN)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_numeric_literals_given_exponents_beyond_float8_range_when_selected() {
    // Arrange
    let expected = format!("1{}", "0".repeat(400));

    // Act
    let batches = crate::support_type_metadata_contract::run_cycles(
        "numeric-exponent-literal",
        &[],
        vec![vec![crate::support_pgwire::simple_query_frame(
            "SELECT 1e400",
        )]],
    );
    let frames = &batches[0];

    // Assert
    crate::support_type_metadata_contract::assert_success(frames);
    let descriptor = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        crate::support_pgwire::parse_row_description(&descriptor.1)[0].type_oid,
        1700
    );
    assert_eq!(
        crate::support_pgwire::data_rows(frames),
        vec![vec![Some(expected)]]
    );
}

#[test]
fn should_infer_numeric_given_integer_literals_above_bigint_range_when_selected() {
    // Arrange
    let sql = "SELECT 9223372036854775808";

    // Act
    let batches = crate::support_type_metadata_contract::run_cycles(
        "numeric-wide-integer-literal",
        &[],
        vec![vec![crate::support_pgwire::simple_query_frame(sql)]],
    );
    let frames = &batches[0];

    // Assert
    crate::support_type_metadata_contract::assert_success(frames);
    let descriptor = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        crate::support_pgwire::parse_row_description(&descriptor.1)[0].type_oid,
        1700
    );
    assert_eq!(
        crate::support_pgwire::data_rows(frames),
        vec![vec![Some("9223372036854775808".to_string())]]
    );
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
        result.rows(),
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
    assert_eq!(result.rows(), vec![vec![Value::String("left".to_string())]]);
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
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("row-1".to_string())]]
    );
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
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("row-1".to_string())]]
    );
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
