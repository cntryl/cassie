use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const UPPER_UUID: &str = "01234567-89AB-CDEF-0123-456789ABCDEF";

fn uuid_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (uid UUID, txt TEXT, blob BYTEA, n INT)",
            "INSERT INTO t (uid, txt, blob, n) VALUES \
                 ('01234567-89ab-cdef-0123-456789abcdef', \
                  '01234567-89AB-CDEF-0123-456789ABCDEF', '\\xdeadbeef', 7)",
        ],
    )
}

#[test]
fn should_type_renamed_derived_columns_by_their_own_source() {
    // Arrange
    let fixture = uuid_fixture("derived_renames");

    // Act
    let renamed = fixture.rows(&format!(
        "SELECT n FROM (SELECT txt AS uid, n FROM t) d WHERE uid = '{UPPER_UUID}'"
    ));
    let plain_text =
        fixture.execute("SELECT n FROM (SELECT txt AS uid, n FROM t) d WHERE uid = 'hello'");

    // Assert
    assert_eq!(renamed, vec![vec![Value::Int64(7)]]);
    assert!(plain_text.is_ok(), "a TEXT column accepts any literal");
}

#[test]
fn should_canonicalize_typed_literals_over_a_cte() {
    // Arrange
    let fixture = uuid_fixture("cte_typed_literals");

    // Act
    let uuid = fixture.rows(&format!(
        "WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE uid = '{UPPER_UUID}'"
    ));
    let bytea =
        fixture.rows("WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE blob = '\\xDEADBEEF'");
    let invalid = fixture.execute("WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE uid = 'nope'");
    let nested = fixture.rows(&format!(
        "WITH c AS (SELECT * FROM t) SELECT n FROM (SELECT * FROM c) d WHERE uid = '{UPPER_UUID}'"
    ));

    // Assert
    assert_eq!(uuid, vec![vec![Value::Int64(7)]]);
    assert_eq!(nested, vec![vec![Value::Int64(7)]]);
    assert_eq!(bytea, vec![vec![Value::Int64(7)]]);
    assert!(
        invalid.is_err(),
        "an invalid UUID literal must be rejected through a CTE"
    );
}

#[test]
fn should_validate_operand_families_over_a_cte() {
    // Arrange
    let fixture = sql_fixture(
        "cte_operand_families",
        &[
            "CREATE TABLE d (k INT, flag BOOLEAN, price FLOAT)",
            "INSERT INTO d (k, flag, price) VALUES (1, true, 19.99)",
        ],
    );

    // Act
    let base = fixture.execute("SELECT k FROM d WHERE flag = 1").is_err();
    let cte = fixture
        .execute("WITH c AS (SELECT * FROM d) SELECT k FROM c WHERE flag = 1")
        .is_err();

    let base_literal = fixture
        .execute("SELECT k FROM d WHERE k = '1'")
        .map(|result| result.rows().to_vec())
        .ok();
    let cte_literal = fixture
        .execute("WITH c AS (SELECT * FROM d) SELECT k FROM c WHERE k = '1'")
        .map(|result| result.rows().to_vec())
        .ok();

    // Assert
    assert_eq!(cte_literal, base_literal);
    assert!(base, "the base table rejects boolean = integer");
    assert!(cte, "the CTE spelling must reject it too");
}
