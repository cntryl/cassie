//! PostgreSQL 18 exact NUMERIC behavior tracked by #812.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

struct NumericDirectory(String);

impl Drop for NumericDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bool_rows(result: &cassie::executor::QueryResult) -> Vec<Vec<Value>> {
    result.rows().to_vec()
}

#[test]
fn should_distinguish_exact_values_beyond_float8_precision() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-float8-boundary", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CAST('9007199254740993' AS NUMERIC) <> CAST('9007199254740992' AS NUMERIC), CAST('12345678901234567890.123456789' AS NUMERIC) <> CAST('12345678901234567890.123456788' AS DECIMAL)",
    );

    // Assert
    assert_eq!(
        bool_rows(&result.expect("exact decimal comparisons")),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_round_numeric_typmods_with_signed_scales() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-typmod-rounding", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CAST('1.245' AS NUMERIC(5, 2)) = CAST('1.25' AS NUMERIC(5, 2)), CAST('-1.245' AS NUMERIC(5, 2)) = CAST('-1.25' AS NUMERIC(5, 2)), CAST('12500' AS NUMERIC(2, -3)) = CAST('13000' AS NUMERIC)",
    );

    // Assert
    assert_eq!(
        bool_rows(&result.expect("numeric scale rounding")),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_round_numeric_columns_on_insert_without_losing_exactness() {
    // Arrange
    let fixture = sql_fixture(
        "issue-812-numeric-typmod-storage",
        &[
            "CREATE TABLE issue_812_amounts (amount NUMERIC(5, 2))",
            "INSERT INTO issue_812_amounts VALUES ('1.245'), ('-1.245')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT amount = CAST('1.25' AS NUMERIC(5, 2)) OR amount = CAST('-1.25' AS NUMERIC(5, 2)) FROM issue_812_amounts ORDER BY amount",
    );

    // Assert
    assert_eq!(
        bool_rows(&result.expect("stored numeric typmod values")),
        &[vec![Value::Bool(true)], vec![Value::Bool(true)]]
    );
}

#[test]
fn should_accept_exponent_notation_without_rounding_to_float8() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-exponent-input", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CAST('1.234567890123456789e30' AS NUMERIC) <> CAST('1234567890123456789000000000001' AS NUMERIC)",
    );

    // Assert
    assert_eq!(
        result.expect("exact numeric exponent input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_keep_numeric_arithmetic_exact() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-arithmetic", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CAST('9007199254740993' AS NUMERIC) - CAST('9007199254740992' AS NUMERIC) = CAST('1' AS NUMERIC), CAST('1.25' AS NUMERIC) / CAST('0.5' AS NUMERIC) = CAST('2.5' AS NUMERIC)",
    );

    // Assert
    assert_eq!(
        bool_rows(&result.expect("exact numeric arithmetic")),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_compare_numeric_special_values() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-special-values", &[]);

    // Act
    let result = fixture.execute(
        "SELECT CAST('NaN' AS NUMERIC) = CAST('NaN' AS NUMERIC), CAST('NaN' AS NUMERIC) > CAST('Infinity' AS NUMERIC), CAST('Infinity' AS NUMERIC) > CAST('999999999999999999999999999999' AS NUMERIC)",
    );

    // Assert
    assert_eq!(
        bool_rows(&result.expect("numeric special-value ordering")),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_sum_bigint_values_beyond_the_int64_range_as_numeric() {
    // Arrange
    let fixture = sql_fixture(
        "issue-812-bigint-sum-range",
        &[
            "CREATE TABLE issue_812_sum_values (value BIGINT)",
            "INSERT INTO issue_812_sum_values VALUES (9223372036854775807), (1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(value) = CAST('9223372036854775808' AS NUMERIC) FROM issue_812_sum_values",
    );

    // Assert
    assert_eq!(
        result.expect("BIGINT sum uses exact numeric range").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_find_exact_numeric_index_keys_that_differ_beyond_float8_precision() {
    // Arrange
    let fixture = sql_fixture(
        "issue-812-numeric-index-precision",
        &[
            "CREATE TABLE issue_812_index_values (amount NUMERIC(30, 0), label TEXT)",
            "INSERT INTO issue_812_index_values VALUES ('9007199254740992', 'lower'), ('9007199254740993', 'upper')",
            "CREATE INDEX issue_812_index_values_amount_idx ON issue_812_index_values USING btree (amount)",
        ],
    );

    // Act
    let indexed = fixture.execute(
        "SELECT label FROM issue_812_index_values WHERE amount = CAST('9007199254740993' AS NUMERIC(30, 0))",
    );
    fixture
        .execute("DROP INDEX issue_812_index_values_amount_idx")
        .expect("drop exact numeric index for scan comparison");
    let scanned = fixture.execute(
        "SELECT label FROM issue_812_index_values WHERE amount = CAST('9007199254740993' AS NUMERIC(30, 0))",
    );

    // Assert
    let expected = &[vec![Value::String("upper".to_owned())]];
    assert_eq!(
        indexed.expect("indexed exact numeric equality").rows(),
        expected
    );
    assert_eq!(
        scanned.expect("scanned exact numeric equality").rows(),
        expected
    );
}

#[test]
fn should_bind_numeric_text_parameters_without_float8_coercion() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT CAST($1 AS NUMERIC) <> CAST('9007199254740992' AS NUMERIC)",
        vec![Value::String("9007199254740993".to_owned())],
    );

    // Assert
    assert_eq!(
        result.expect("exact numeric text parameter").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_keep_unconstrained_numeric_values_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-812-numeric-restart");
    let _directory = NumericDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create numeric fixture");
        cassie.startup().expect("start numeric fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_812_restart_values (amount NUMERIC)",
                vec![],
            )
            .expect("create unconstrained numeric column");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO issue_812_restart_values VALUES ('12345678901234567890.123456789')",
                vec![],
            )
            .expect("store exact numeric value");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen numeric fixture");
    restarted.startup().expect("restart numeric fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        "SELECT amount = CAST('12345678901234567890.123456789' AS NUMERIC) FROM issue_812_restart_values",
        vec![],
    );

    // Assert
    assert_eq!(
        result.expect("numeric value survives restart").rows(),
        &[vec![Value::Bool(true)]]
    );
    drop(session);
    restarted.shutdown();
    drop(restarted);
}
