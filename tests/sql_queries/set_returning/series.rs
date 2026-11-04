//! User-selected finite PostgreSQL18.6 generate_series families.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_evaluate_scalar_subquery_series_bounds_with_native_result_identity() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-subquery-bounds", &[]);

    // Act
    let result = fixture.execute("SELECT n::TEXT FROM generate_series((SELECT 1),(SELECT 3)) s(n)");

    // Assert
    assert_eq!(
        result.expect("scalar subquery bounds").rows(),
        strings(&["1", "2", "3"])
    );
}

fn strings(values: &[&str]) -> Vec<Vec<Value>> {
    values
        .iter()
        .map(|value| vec![Value::String((*value).to_owned())])
        .collect()
}

#[test]
fn should_generate_inclusive_integer_series_with_checked_endpoint_progression() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-integers", &[]);
    let cases = [
        ("SELECT v::TEXT FROM generate_series(1,3) s(v)", vec!["1","2","3"]),
        ("SELECT v::TEXT FROM generate_series(3,1,-1) s(v)", vec!["3","2","1"]),
        ("SELECT v::TEXT FROM generate_series(3,1,1) s(v)", vec![]),
        ("SELECT v::TEXT FROM generate_series(-2147483648,-2147483648,-1) s(v)", vec!["-2147483648"]),
        ("SELECT v::TEXT FROM generate_series('-9223372036854775808'::BIGINT,'-9223372036854775808'::BIGINT,-1::BIGINT) s(v)", vec!["-9223372036854775808"]),
        ("SELECT v::TEXT FROM generate_series(2147483647,2147483647,1) s(v)", vec!["2147483647"]),
        ("SELECT v::TEXT FROM generate_series(9223372036854775807::BIGINT,9223372036854775807::BIGINT,1::BIGINT) s(v)", vec!["9223372036854775807"]),
    ];

    for (sql, expected) in cases {
        // Act
        assert_eq!(
            fixture.execute(sql).expect("finite integer series").rows(),
            // Assert
            strings(&expected),
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_exact_numeric_series_scale_during_advancement() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-numeric", &[]);

    // Act
    let result = fixture.execute(
        "SELECT v::TEXT FROM generate_series(1.00::NUMERIC,2.50::NUMERIC,0.50::NUMERIC) s(v)",
    );

    // Assert
    assert_eq!(
        result.expect("exact decimal series").rows(),
        strings(&["1.00", "1.50", "2.00", "2.50"])
    );
}

#[test]
fn should_advance_calendar_series_under_the_resolved_timezone() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-calendar", &["SET TIME ZONE 'UTC'"]);

    // Act
    let civil = fixture.execute("SELECT v::TEXT FROM generate_series('2026-01-01'::TIMESTAMP,'2026-01-03'::TIMESTAMP,INTERVAL '1 day') s(v)");
    let zoned = fixture.execute("SELECT v::TEXT FROM generate_series('2026-03-07 12:00:00-05'::TIMESTAMPTZ,'2026-03-09 12:00:00-04'::TIMESTAMPTZ,INTERVAL '1 day','America/New_York') s(v)");

    // Assert
    assert_eq!(
        civil.expect("civil calendar series").rows(),
        strings(&[
            "2026-01-01 00:00:00",
            "2026-01-02 00:00:00",
            "2026-01-03 00:00:00"
        ])
    );
    assert_eq!(
        zoned.expect("named-zone DST calendar series").rows(),
        strings(&[
            "2026-03-07 17:00:00+00",
            "2026-03-08 16:00:00+00",
            "2026-03-09 16:00:00+00"
        ])
    );
}

#[test]
fn should_treat_null_series_arguments_as_empty_before_validating_zero_steps() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-null-strictness", &[]);
    let queries = [
        "SELECT count(*) FROM generate_series(NULL::INT,3,0) s(v)",
        "SELECT count(*) FROM generate_series(1,3,NULL::INT) s(v)",
        "SELECT count(*) FROM generate_series(NULL::TIMESTAMPTZ,'2026-01-01'::TIMESTAMPTZ,INTERVAL '0','not/a/zone') s(v)",
    ];

    for sql in queries {
        // Act
        assert_eq!(
            fixture
                .execute(sql)
                // Assert
                .expect("strict NULL source")
                .rows(),
            &[vec![Value::Int64(0)]],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_numeric_scale_when_descending_past_the_inclusive_bound() {
    // Arrange
    let fixture = sql_fixture("issue-808-series-negative-numeric", &[]);

    // Act
    let result = fixture.execute(
        "SELECT v::TEXT FROM generate_series(1.0::NUMERIC,0.00::NUMERIC,-0.25::NUMERIC) s(v)",
    );

    // Assert
    assert_eq!(
        result.expect("descending exact source").rows(),
        strings(&["1.0", "0.75", "0.50", "0.25", "0.00"])
    );
}
