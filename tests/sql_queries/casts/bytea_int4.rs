//! Native BYTEA to signed integer round trips.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_negative_32768_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_32768_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-32768)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-32_768)]]
    );
}

#[test]
fn should_decode_negative_32768_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_32768_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-32768)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-32_768)]]
    );
}

#[test]
fn should_decode_negative_32768_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_32768_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-32768)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-32_768)]]
    );
}

#[test]
fn should_decode_negative_257_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_257_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-257)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-257)]]
    );
}

#[test]
fn should_decode_negative_257_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_257_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-257)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-257)]]
    );
}

#[test]
fn should_decode_negative_257_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_257_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-257)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-257)]]
    );
}

#[test]
fn should_decode_negative_256_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_256_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-256)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-256)]]
    );
}

#[test]
fn should_decode_negative_256_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_256_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-256)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-256)]]
    );
}

#[test]
fn should_decode_negative_256_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_256_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-256)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-256)]]
    );
}

#[test]
fn should_decode_negative_129_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_129_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-129)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-129)]]
    );
}

#[test]
fn should_decode_negative_129_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_129_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-129)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-129)]]
    );
}

#[test]
fn should_decode_negative_129_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_129_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-129)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-129)]]
    );
}

#[test]
fn should_decode_negative_128_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_128_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-128)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-128)]]
    );
}

#[test]
fn should_decode_negative_128_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_128_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-128)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-128)]]
    );
}

#[test]
fn should_decode_negative_128_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_128_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-128)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-128)]]
    );
}

#[test]
fn should_decode_negative_1_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_1_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (-1)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-1)]]
    );
}

#[test]
fn should_decode_negative_1_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_1_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((-1)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-1)]]
    );
}

#[test]
fn should_decode_negative_1_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_1_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-1)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-1)]]
    );
}

#[test]
fn should_decode_positive_0_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_0_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (0)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_positive_0_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_0_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((0)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_positive_0_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_0_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((0)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_positive_1_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_1_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (1)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_decode_positive_1_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_1_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((1)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_decode_positive_1_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_1_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((1)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_decode_positive_127_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_127_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (127)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(127)]]
    );
}

#[test]
fn should_decode_positive_127_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_127_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((127)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(127)]]
    );
}

#[test]
fn should_decode_positive_127_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_127_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((127)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(127)]]
    );
}

#[test]
fn should_decode_positive_128_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_128_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (128)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(128)]]
    );
}

#[test]
fn should_decode_positive_128_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_128_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((128)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(128)]]
    );
}

#[test]
fn should_decode_positive_128_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_128_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((128)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(128)]]
    );
}

#[test]
fn should_decode_positive_255_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_255_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (255)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(255)]]
    );
}

#[test]
fn should_decode_positive_255_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_255_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((255)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(255)]]
    );
}

#[test]
fn should_decode_positive_255_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_255_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((255)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(255)]]
    );
}

#[test]
fn should_decode_positive_256_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_256_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (256)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(256)]]
    );
}

#[test]
fn should_decode_positive_256_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_256_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((256)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(256)]]
    );
}

#[test]
fn should_decode_positive_256_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_256_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((256)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(256)]]
    );
}

#[test]
fn should_decode_positive_32767_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_32767_scalar", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (SELECT (32767)::INT4::BYTEA AS output) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(32_767)]]
    );
}

#[test]
fn should_decode_positive_32767_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_32767_values", &[]);
    // Act
    let result =
        fixture.execute("SELECT v::INT4 AS output FROM (VALUES ((32767)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(32_767)]]
    );
}

#[test]
fn should_decode_positive_32767_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_32767_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((32767)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(32_767)]]
    );
}

#[test]
fn should_decode_negative_2147483648_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_2147483648_scalar", &[]);
    // Act
    let result = fixture.execute(
        "SELECT v::INT4 AS output FROM (SELECT (-2147483648)::INT4::BYTEA AS output) AS q(v)",
    );
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-2_147_483_648)]]
    );
}

#[test]
fn should_decode_negative_2147483648_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_2147483648_values", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (VALUES ((-2147483648)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-2_147_483_648)]]
    );
}

#[test]
fn should_decode_negative_2147483648_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_negative_2147483648_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((-2147483648)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(-2_147_483_648)]]
    );
}

#[test]
fn should_decode_positive_2147483647_given_int4_bytea_when_scalar_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_2147483647_scalar", &[]);
    // Act
    let result = fixture.execute(
        "SELECT v::INT4 AS output FROM (SELECT (2147483647)::INT4::BYTEA AS output) AS q(v)",
    );
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(2_147_483_647)]]
    );
}

#[test]
fn should_decode_positive_2147483647_given_int4_bytea_when_values_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_2147483647_values", &[]);
    // Act
    let result = fixture
        .execute("SELECT v::INT4 AS output FROM (VALUES ((2147483647)::INT4::BYTEA)) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(2_147_483_647)]]
    );
}

#[test]
fn should_decode_positive_2147483647_given_int4_bytea_when_cte_round_trips() {
    // Arrange
    let fixture = sql_fixture("byteacasts-reverse-int4_positive_2147483647_cte", &[]);
    // Act
    let result = fixture.execute("SELECT v::INT4 AS output FROM (WITH q(v) AS (VALUES ((2147483647)::INT4)) SELECT v::BYTEA AS output FROM q) AS q(v)");
    // Assert
    assert_eq!(
        result.expect("native integer BYTEA round trip").rows(),
        &[vec![Value::Int64(2_147_483_647)]]
    );
}
