//! PostgreSQL 18 temporal type and expression grammar tracked by #814.

#[test]
fn should_parse_timestamptz_alias_with_fractional_precision() {
    // Arrange
    let sql = "SELECT CAST('2024-01-02 03:04:05.123456+00' AS TIMESTAMPTZ(3))";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "TIMESTAMPTZ precision: {parsed:?}");
}

#[test]
fn should_parse_timestamp_with_time_zone_field_order() {
    // Arrange
    let sql = "SELECT CAST('2024-01-02 03:04:05+00' AS TIMESTAMP(3) WITH TIME ZONE)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "timestamp with time zone: {parsed:?}");
}

#[test]
fn should_parse_timetz_precision_alias() {
    // Arrange
    let sql = "SELECT CAST('04:05:06.789-08:00' AS TIMETZ(2))";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "TIMETZ precision: {parsed:?}");
}

#[test]
fn should_parse_time_with_time_zone_precision() {
    // Arrange
    let sql = "SELECT CAST('04:05:06.789-08:00' AS TIME(2) WITH TIME ZONE)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "time with time zone precision: {parsed:?}");
}

#[test]
fn should_parse_timestamp_time_without_zone_typmods() {
    // Arrange
    let statements = [
        "SELECT CAST('2001-02-16 20:38:40.123456' AS TIMESTAMP(6) WITHOUT TIME ZONE)",
        "SELECT CAST('04:05:06.123456' AS TIME(6) WITHOUT TIME ZONE)",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(
            result.is_ok(),
            "temporal type declaration {sql}: {result:?}"
        );
    }
}

#[test]
fn should_parse_interval_qualifier_precision_matrix() {
    // Arrange
    let statements = [
        "SELECT CAST('1 year 2 months' AS INTERVAL YEAR TO MONTH)",
        "SELECT CAST('1' AS INTERVAL YEAR)",
        "SELECT CAST('1' AS INTERVAL MONTH)",
        "SELECT CAST('1' AS INTERVAL DAY)",
        "SELECT CAST('1' AS INTERVAL HOUR)",
        "SELECT CAST('1' AS INTERVAL MINUTE)",
        "SELECT CAST('1.25' AS INTERVAL SECOND(2))",
        "SELECT CAST('1 2:03:04' AS INTERVAL DAY TO HOUR)",
        "SELECT CAST('1 2:03:04' AS INTERVAL DAY TO MINUTE)",
        "SELECT CAST('3 days 4:05:06.789' AS INTERVAL DAY TO SECOND(3))",
        "SELECT CAST('4:05:06' AS INTERVAL HOUR TO MINUTE)",
        "SELECT CAST('4:05:06.789' AS INTERVAL HOUR TO SECOND(3))",
        "SELECT CAST('5:06.789' AS INTERVAL MINUTE TO SECOND(3))",
        "SELECT CAST('1 second' AS INTERVAL(3))",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(result.is_ok(), "interval declaration {sql}: {result:?}");
    }
}

#[test]
fn should_parse_timestamp_at_time_zone_for_each_timestamp_direction() {
    // Arrange
    let statements = [
        "SELECT TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE 'America/Denver'",
        "SELECT TIMESTAMPTZ '2001-02-16 20:38:40-05' AT TIME ZONE 'America/Denver'",
        "SELECT TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE INTERVAL '-08:00'",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(result.is_ok(), "AT TIME ZONE expression {sql}: {result:?}");
    }
}

#[test]
fn should_parse_at_local_for_timestamp_values() {
    // Arrange
    let statements = [
        "SELECT TIMESTAMP '2001-02-16 20:38:40' AT LOCAL",
        "SELECT TIMESTAMPTZ '2001-02-16 20:38:40-05' AT LOCAL",
        "SELECT TIME WITH TIME ZONE '20:38:40-05' AT LOCAL",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(result.is_ok(), "AT LOCAL expression {sql}: {result:?}");
    }
}

#[test]
fn should_parse_temporal_arithmetic_operator_forms() {
    // Arrange
    let statements = [
        "SELECT DATE '2001-09-28' + 7",
        "SELECT DATE '2001-09-28' - 7",
        "SELECT DATE '2001-09-28' - DATE '2001-09-27'",
        "SELECT DATE '2001-09-28' + TIME '03:00'",
        "SELECT DATE '2001-09-28' + INTERVAL '1 month'",
        "SELECT DATE '2001-09-28' - INTERVAL '1 day'",
        "SELECT TIME '05:00' + INTERVAL '1 hour'",
        "SELECT TIME '05:00' - INTERVAL '1 hour'",
        "SELECT TIME '05:00' - TIME '03:00'",
        "SELECT TIMESTAMP '2001-09-28 23:00' - TIMESTAMP '2001-09-28 01:00'",
        "SELECT INTERVAL '1 day' * 2.5",
        "SELECT INTERVAL '1 hour' / 1.5",
        "SELECT - INTERVAL '23 hours'",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(result.is_ok(), "temporal arithmetic {sql}: {result:?}");
    }
}

#[test]
fn should_parse_timezone_function_overloads() {
    // Arrange
    let statements = [
        "SELECT timezone('America/Denver', TIMESTAMP '2001-02-16 20:38:40')",
        "SELECT timezone('UTC', TIME WITH TIME ZONE '05:34:17-05')",
        "SELECT timezone(TIMESTAMP '2001-02-16 20:38:40')",
        "SELECT timezone(TIME WITH TIME ZONE '05:34:17-05')",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in statements.iter().zip(parsed) {
        assert!(result.is_ok(), "timezone overload {sql}: {result:?}");
    }
}
