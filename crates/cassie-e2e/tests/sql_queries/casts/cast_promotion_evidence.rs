use super::support_sql as support;

use cassie::types::Value;

use support::data_dir;

#[test]
fn should_cast_values_at_every_documented_type_boundary() {
    // Arrange
    let path = data_dir("type_cast_boundary_matrix_ok");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    let ok_cases: &[(&str, Value)] = &[
        ("CAST('32767' AS SMALLINT)", Value::Int64(32767)),
        ("CAST('-32768' AS SMALLINT)", Value::Int64(-32768)),
        ("CAST('2147483647' AS INT)", Value::Int64(2_147_483_647)),
        ("CAST('-2147483648' AS INT)", Value::Int64(-2_147_483_648)),
        (
            "CAST('9223372036854775807' AS BIGINT)",
            Value::Int64(9_223_372_036_854_775_807),
        ),
        (
            "CAST('-9223372036854775808' AS BIGINT)",
            Value::Int64(-9_223_372_036_854_775_808),
        ),
        ("CAST('3.5' AS FLOAT)", Value::Float64(3.5)),
        ("CAST('true' AS BOOLEAN)", Value::Bool(true)),
        ("CAST('F' AS BOOLEAN)", Value::Bool(false)),
        ("CAST('abc' AS CHAR(3))", Value::String("abc".to_string())),
        ("CAST('abcd' AS CHAR(3))", Value::String("abc".to_string())),
        ("CAST('abc ' AS CHAR(3))", Value::String("abc".to_string())),
        (
            "CAST('abc' AS VARCHAR(3))",
            Value::String("abc".to_string()),
        ),
        (
            "CAST('abcd' AS VARCHAR(3))",
            Value::String("abc".to_string()),
        ),
        (
            "CAST('550e8400-e29b-41d4-a716-446655440000' AS UUID)",
            Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
        ),
        (
            "CAST('\\x0a0b' AS BYTEA)",
            Value::String("\\x0a0b".to_string()),
        ),
        (
            // PostgreSQL 18.6 renders timestamp output in ISO DateStyle.
            "CAST('2024-01-01T09:00:00' AS TIMESTAMP)",
            Value::String("2024-01-01 09:00:00".to_string()),
        ),
        // DATE and TIME appeared only in the rejection matrix, so nothing
        // proved a valid value of either type casts at all.
        (
            "CAST('2024-01-01' AS DATE)",
            Value::String("2024-01-01".to_string()),
        ),
        (
            "CAST('23:59:59' AS TIME)",
            Value::String("23:59:59".to_string()),
        ),
        ("CAST(NULL AS INT)", Value::Null),
    ];

    // Act
    let selected = ok_cases
        .iter()
        .map(|(expr, _)| {
            cassie
                .execute_sql(&session, &format!("SELECT {expr}"), vec![])
                .unwrap_or_else(|error| panic!("{expr} should cast: {error}"))
                .rows()
                .to_vec()
        })
        .collect::<Vec<_>>();

    // Assert
    for ((expr, expected), rows) in ok_cases.iter().zip(selected) {
        assert_eq!(rows, vec![vec![expected.clone()]], "{expr}");
    }

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_casts_beyond_every_documented_type_boundary() {
    // Arrange
    let path = data_dir("type_cast_boundary_matrix_rejected");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    let rejected_cases: &[&str] = &[
        "CAST('32768' AS SMALLINT)",
        "CAST('-32769' AS SMALLINT)",
        "CAST('2147483648' AS INT)",
        "CAST('9223372036854775808' AS BIGINT)",
        "CAST('not-a-number' AS INT)",
        "CAST('maybe' AS BOOLEAN)",
        "CAST('not-a-uuid' AS UUID)",
        // Plain text is valid BYTEA input; malformed hex is the rejection case.
        "CAST('\\xg1' AS BYTEA)",
        "CAST('not-a-timestamp' AS TIMESTAMP)",
        "CAST('not-a-date' AS DATE)",
        "CAST('25:00:00' AS TIME)",
    ];

    // Act
    let results = rejected_cases
        .iter()
        .map(|expr| cassie.execute_sql(&session, &format!("SELECT {expr}"), vec![]))
        .collect::<Vec<_>>();

    // Assert
    for (expr, result) in rejected_cases.iter().zip(results) {
        let error = result
            .expect_err(&format!("{expr} should be rejected"))
            .to_string();
        // A range error is a successful cast-validation path too: for
        // example, PostgreSQL reports an out-of-range value for SMALLINT
        // rather than a generic cast failure. Keep syntax and unsupported
        // feature errors distinct from recognized conversion failures.
        assert!(
            error.contains("cannot cast value to")
                || error.contains("out of range")
                || error.contains("invalid input syntax for type")
                || error.contains("invalid hexadecimal digit"),
            "{expr} must be rejected by cast/input validation, got: {error}"
        );
        assert!(
            !error.contains("unsupported feature"),
            "{expr} must be validated, not reported as unimplemented: {error}"
        );
    }

    let _ = std::fs::remove_dir_all(path);
}
