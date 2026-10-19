use super::support_sql::{data_dir, use_local_storage};
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const OID_INT8: i64 = 20;
const OID_INT2: i64 = 21;
const OID_INT4: i64 = 23;
const OID_NUMERIC: i64 = 1700;

fn seeded(label: &str) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE numeric_rows (score INT, big BIGINT, label TEXT)",
        "INSERT INTO numeric_rows (score, big, label) VALUES (7, 1, 'a')",
        "INSERT INTO numeric_rows (score, big, label) VALUES (-7, 2, 'b')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    (cassie, session, path)
}

fn first_value(cassie: &Cassie, session: &CassieSession, sql: &str) -> (Value, i64) {
    let result = cassie.execute_sql(session, sql, vec![]).expect(sql);
    (result.rows()[0][0].clone(), result.columns()[0].type_oid)
}

fn mixed_width_fixture(label: &str) -> super::support_sql_fixture::SqlFixture {
    super::support_sql_fixture::sql_fixture(
        label,
        &[
            "CREATE TABLE mix (k TEXT, s SMALLINT, i INT, b BIGINT)",
            "INSERT INTO mix (k, s, i, b) VALUES ('empty', NULL, NULL, NULL)",
            "INSERT INTO mix (k, s, i, b) VALUES ('one', 1, 1, 1)",
        ],
    )
}

fn typed_first_value(fixture: &super::support_sql_fixture::SqlFixture, sql: &str) -> (Value, i64) {
    let result = fixture
        .execute(sql)
        .map(|result| (result.rows()[0][0].clone(), result.columns()[0].type_oid));
    result.unwrap_or((Value::Null, -1))
}

#[test]
fn should_type_coalesce_by_the_common_argument_type() {
    // Arrange
    let fixture = mixed_width_fixture("numeric_coalesce_common_type");

    // Act
    let widened = [
        "SELECT COALESCE(b, 1.5) FROM mix WHERE k = 'empty'",
        "SELECT COALESCE(i, 1.5) FROM mix WHERE k = 'empty'",
        "SELECT COALESCE(s, 1.5) FROM mix WHERE k = 'empty'",
    ]
    .map(|sql| typed_first_value(&fixture, sql));
    let integers = typed_first_value(&fixture, "SELECT COALESCE(s, i) FROM mix WHERE k = 'one'");

    // Assert
    // The decimal literal 1.5 is numeric, so COALESCE resolves to numeric.
    assert!(widened
        .iter()
        .all(|typed| *typed == (Value::String("1.5".to_owned()), OID_NUMERIC)));
    assert_eq!(integers, (Value::Int64(1), OID_INT4));
}

#[test]
fn should_type_abs_of_smallint_as_smallint() {
    // Arrange
    let fixture = mixed_width_fixture("numeric_abs_smallint");

    // Act
    let absolute = typed_first_value(&fixture, "SELECT abs(s) FROM mix WHERE k = 'one'");

    // Assert
    assert_eq!(absolute, (Value::Int64(1), OID_INT2));
}

#[test]
fn should_type_integer_case_branches_as_integer() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_case_integer");

    // Act
    let summed = first_value(
        &cassie,
        &session,
        "SELECT SUM(CASE WHEN score > 1 THEN score ELSE 0 END) AS total FROM numeric_rows",
    );
    let literal_branches = first_value(
        &cassie,
        &session,
        "SELECT CASE WHEN true THEN 1 ELSE 0 END AS flag",
    );
    let mixed = first_value(
            &cassie,
            &session,
            "SELECT CASE WHEN score > 1 THEN score ELSE 0.5 END AS mixed FROM numeric_rows WHERE label = 'a'",
        );

    // Assert
    assert_eq!(summed, (Value::Int64(7), OID_INT8));
    assert_eq!(literal_branches, (Value::Int64(1), OID_INT4));
    // An integer branch beside the numeric literal 0.5 yields numeric.
    assert_eq!(mixed, (Value::String("7".to_owned()), OID_NUMERIC));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_coerce_case_results_in_window_function_arguments() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_case_window");

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "SELECT lag(CASE WHEN score > 0 THEN score ELSE 0.5 END) OVER (ORDER BY big) AS previous FROM numeric_rows ORDER BY big",
                vec![],
            )
            .expect("execute CASE in window argument");

    // Assert
    // CASE over an integer and the numeric literal 0.5 is numeric.
    assert_eq!(result.columns()[0].type_oid, OID_NUMERIC);
    assert_eq!(result.rows()[0][0], Value::Null);
    assert_eq!(result.rows()[1][0], Value::String("7".to_owned()));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_case_with_incompatible_branch_families() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_case_text_int");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT CASE WHEN score > 1 THEN label ELSE 0 END FROM numeric_rows",
            vec![],
        )
        .expect_err("reject text and integer CASE branches");

    // Assert
    assert!(error.to_string().contains("CASE result"), "{error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_count_as_int8_in_every_context() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_count_int8");

    // Act
    let top_level = first_value(
        &cassie,
        &session,
        "SELECT COUNT(score) AS n FROM numeric_rows",
    );
    let coalesced = first_value(
        &cassie,
        &session,
        "SELECT COALESCE(COUNT(score), 0) AS n FROM numeric_rows",
    );
    let cased = first_value(
        &cassie,
        &session,
        "SELECT CASE WHEN COUNT(score) > 0 THEN COUNT(score) ELSE 0 END AS n FROM numeric_rows",
    );
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW counted AS SELECT COUNT(score) AS n FROM numeric_rows",
            vec![],
        )
        .expect("create view");
    let derived = first_value(&cassie, &session, "SELECT n FROM counted");

    // Assert
    assert_eq!(top_level, (Value::Int64(2), OID_INT8));
    assert_eq!(coalesced, (Value::Int64(2), OID_INT8));
    assert_eq!(cased, (Value::Int64(2), OID_INT8));
    assert_eq!(derived.1, OID_INT8);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_truncate_integer_division_toward_zero() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_integer_division");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT score / 2 AS half, score / 2.0 AS exact FROM numeric_rows ORDER BY big",
            vec![],
        )
        .expect("integer division");
    let literal = first_value(&cassie, &session, "SELECT 7 / 2 AS half");
    let by_zero = cassie
        .execute_sql(&session, "SELECT score / 0 FROM numeric_rows", vec![])
        .expect_err("reject integer division by zero");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        // PostgreSQL 18.6: int4 / int4 is int4; int4 / 2.0 is numeric.
        vec![OID_INT4, OID_NUMERIC]
    );
    assert_eq!(
        result.rows(),
        vec![
            vec![
                Value::Int64(3),
                Value::String("3.5000000000000000".to_owned())
            ],
            vec![
                Value::Int64(-3),
                Value::String("-3.5000000000000000".to_owned())
            ],
        ]
    );
    assert_eq!(literal, (Value::Int64(3), OID_INT4));
    assert!(
        by_zero.to_string().contains("division by zero"),
        "{by_zero}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_bigint_division_overflow() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_division_overflow");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO numeric_rows (score, big, label) VALUES (0, $1, 'min')",
            vec![Value::Int64(i64::MIN)],
        )
        .expect("insert minimum bigint");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT big / -1 FROM numeric_rows WHERE label = 'min'",
            vec![],
        )
        .expect_err("reject i64::MIN / -1");

    // Assert
    assert!(error.to_string().contains("bigint out of range"), "{error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_type_integer_column_with_decimal_literal_as_numeric() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_decimal_literal");

    // Act
    let decimal = first_value(
        &cassie,
        &session,
        "SELECT score + 1.0 AS value FROM numeric_rows WHERE label = 'a'",
    );
    let integer = first_value(
        &cassie,
        &session,
        "SELECT score + 1 AS value FROM numeric_rows WHERE label = 'a'",
    );

    // Assert
    // PostgreSQL 18.6: int4 + 1.0 is numeric and int4 + 1 is int4.
    assert_eq!(decimal, (Value::String("8.0".to_owned()), OID_NUMERIC));
    assert_eq!(integer, (Value::Int64(8), OID_INT4));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_bigint_out_of_range_for_scalar_overflow() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_scalar_overflow");

    // Act
    let error = cassie
        .execute_sql(&session, "SELECT 9223372036854775807 + 1", vec![])
        .expect_err("reject bigint overflow");

    // Assert
    assert!(error.to_string().contains("bigint out of range"), "{error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_group_by_case_expression_regardless_of_identifier_case() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_group_by_case");

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "SELECT CASE WHEN SCORE > 0 THEN 'positive' ELSE 'negative' END AS sign, COUNT(score) AS n FROM numeric_rows GROUP BY CASE WHEN score > 0 THEN 'positive' ELSE 'negative' END ORDER BY sign",
                vec![],
            )
            .expect("GROUP BY CASE");

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![Value::String("negative".into()), Value::Int64(1)],
            vec![Value::String("positive".into()), Value::Int64(1)],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allow_case_wrapping_grouped_aggregates() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_case_over_aggregates");

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "SELECT label, CASE WHEN SUM(score) > 0 THEN label ELSE 'none' END AS kept FROM numeric_rows GROUP BY label ORDER BY label",
                vec![],
            )
            .expect("CASE wrapping aggregates");
    let ungrouped = cassie
        .execute_sql(
            &session,
            "SELECT label, CASE WHEN score > 0 THEN 1 ELSE 0 END FROM numeric_rows GROUP BY label",
            vec![],
        )
        .expect_err("reject ungrouped column inside CASE");

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![Value::String("a".into()), Value::String("a".into())],
            vec![Value::String("b".into()), Value::String("none".into())],
        ]
    );
    assert!(ungrouped.to_string().contains("GROUP BY"), "{ungrouped}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_groups_with_case_in_having() {
    // Arrange
    let (cassie, session, path) = seeded("numeric_having_case");

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "SELECT label, SUM(CASE WHEN score > 0 THEN 1 ELSE 0 END) AS positives FROM numeric_rows GROUP BY label HAVING SUM(CASE WHEN score > 0 THEN 1 ELSE 0 END) > 0",
                vec![],
            )
            .expect("HAVING with CASE");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("a".into()), Value::Int64(1)]]
    );
    assert_eq!(result.columns()[1].type_oid, OID_INT8);
    let _ = std::fs::remove_dir_all(path);
}
