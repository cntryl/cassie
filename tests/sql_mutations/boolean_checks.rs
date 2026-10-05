//! Exercises only the existing persisted field/operator/JSON-value CHECK shape.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::catalog::{canonical_relation_name, ConstraintCheck};
use cassie::types::Value;

fn metadata_fixture(label: &str, setup: &[&str]) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    for sql in setup {
        fixture.execute(sql).expect("metadata fixture setup");
    }
    fixture
}

fn stored_check(fixture: &SqlFixture, table: &str) -> ConstraintCheck {
    fixture
        .cassie
        .catalog
        .get_constraint(
            &canonical_relation_name("postgres", "public", table),
            "flag",
        )
        .expect("declared Boolean constraint")
        .check
        .expect("simple comparison CHECK")
}

#[test]
fn should_canonicalize_boolean_comparison_literals_in_create_checks() {
    // Arrange
    let fixture = metadata_fixture("bool-create-check-comparison", &[]);

    // Act
    let created = fixture
        .execute("CREATE TABLE bool_check_create (id INT, flag BOOLEAN CHECK (flag = 'no'))");

    // Assert
    created.expect("existing CHECK comparison syntax remains supported");
    assert_eq!(
        serde_json::to_value(stored_check(&fixture, "bool_check_create")).expect("CHECK metadata"),
        serde_json::json!({"field": "flag", "operator": "eq", "value": false})
    );
    assert_eq!(
        fixture.rows("INSERT INTO bool_check_create VALUES (1, FALSE), (2, NULL) RETURNING flag"),
        vec![vec![Value::Bool(false)], vec![Value::Null]]
    );
    assert!(matches!(
        fixture.execute("INSERT INTO bool_check_create VALUES (3, TRUE)"),
        Err(CassieError::CheckViolation { .. })
    ));
}

#[test]
fn should_canonicalize_boolean_comparison_literals_before_alter_check_validation() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-alter-check-comparison",
        &[
            "CREATE TABLE bool_check_alter (id INT, flag BOOLEAN)",
            "INSERT INTO bool_check_alter VALUES (1, FALSE), (2, NULL)",
        ],
    );

    // Act
    let altered = fixture
        .execute("ALTER TABLE bool_check_alter ADD CONSTRAINT false_flag CHECK (flag = '  OF  ')");

    // Assert
    altered.expect("canonicalize before scanning existing rows");
    assert_eq!(stored_check(&fixture, "bool_check_alter").value, false);
    assert_eq!(
        fixture.rows("SELECT flag FROM bool_check_alter ORDER BY id"),
        vec![vec![Value::Bool(false)], vec![Value::Null]]
    );
    assert!(matches!(
        fixture.execute("INSERT INTO bool_check_alter VALUES (3, TRUE)"),
        Err(CassieError::CheckViolation { .. })
    ));
}

#[test]
fn should_reject_invalid_boolean_comparison_inputs_before_create_check_publication() {
    // Arrange
    let fixture = metadata_fixture("bool-invalid-create-check", &[]);
    let cases = [
        ("bool_check_invalid_ambiguous", "'o'"),
        ("bool_check_invalid_word", "'maybe'"),
        ("bool_check_invalid_numeric", "1"),
    ];

    // Act
    let results = cases.map(|(table, input)| {
        fixture.execute(&format!(
            "CREATE TABLE {table} (flag BOOLEAN CHECK (flag = {input}))"
        ))
    });

    // Assert
    for ((table, input), result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "CHECK input {input}: {result:?}"
        );
        assert!(
            !fixture.cassie.catalog.exists(table),
            "invalid CHECK published {table}"
        );
    }
}

#[test]
fn should_reject_invalid_boolean_comparison_inputs_before_empty_alter_check_publication() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-invalid-empty-alter-check",
        &[
            "CREATE TABLE bool_check_alter_ambiguous (flag BOOLEAN)",
            "CREATE TABLE bool_check_alter_word (flag BOOLEAN)",
            "CREATE TABLE bool_check_alter_numeric (flag BOOLEAN)",
        ],
    );
    let cases = [
        ("bool_check_alter_ambiguous", "'o'"),
        ("bool_check_alter_word", "'maybe'"),
        ("bool_check_alter_numeric", "1"),
    ];

    // Act
    let results = cases.map(|(table, input)| {
        fixture.execute(&format!(
            "ALTER TABLE {table} ADD CONSTRAINT invalid_flag CHECK (flag = {input})"
        ))
    });

    // Assert
    for ((table, input), result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "CHECK input {input}: {result:?}"
        );
        assert!(fixture
            .cassie
            .catalog
            .get_constraints(table)
            .iter()
            .all(|constraint| constraint.check.is_none()));
        assert_eq!(
            fixture.rows(&format!("INSERT INTO {table} VALUES (TRUE) RETURNING flag")),
            vec![vec![Value::Bool(true)]]
        );
    }
}
