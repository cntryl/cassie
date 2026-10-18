use cassie::types::Value;

use super::support_sql_fixture::sql_fixture;

#[test]
fn should_round_float_to_nearest_integer_when_casting() {
    // Arrange
    let fixture = sql_fixture(
        "cast_float_rounds",
        &[
            "CREATE TABLE d (id INT, price FLOAT)",
            "INSERT INTO d (id, price) VALUES (1, 19.99)",
            "INSERT INTO d (id, price) VALUES (2, 5.0)",
            "INSERT INTO d (id, price) VALUES (3, 2.5)",
            "INSERT INTO d (id, price) VALUES (4, -3.5)",
        ],
    );

    // Act
    let ints = fixture.rows("SELECT CAST(price AS INT) FROM d ORDER BY id");
    let bigints = fixture.rows("SELECT CAST(price AS BIGINT) FROM d WHERE id = 1");
    let smallints = fixture.rows("SELECT CAST(price AS SMALLINT) FROM d WHERE id = 4");

    // Assert
    assert_eq!(
        ints,
        vec![
            vec![Value::Int64(20)],
            vec![Value::Int64(5)],
            vec![Value::Int64(2)],
            vec![Value::Int64(-4)],
        ]
    );
    assert_eq!(bigints, vec![vec![Value::Int64(20)]]);
    assert_eq!(smallints, vec![vec![Value::Int64(-4)]]);
}

#[test]
fn should_reject_float_cast_outside_integer_range() {
    // Arrange
    let fixture = sql_fixture(
        "cast_float_out_of_range",
        &[
            "CREATE TABLE r (price FLOAT)",
            "INSERT INTO r (price) VALUES (1.0e10)",
        ],
    );

    // Act
    let failed = fixture.execute("SELECT CAST(price AS INT) FROM r").is_err();

    // Assert
    assert!(failed, "a float beyond int4 must not cast to INT");
}
