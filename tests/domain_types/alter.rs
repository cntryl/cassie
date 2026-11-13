//! ALTER DOMAIN actions, validation of existing rows and failure atomicity.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn with_small(label: &str) -> SqlFixture {
    let fixture = fixture(label);
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();
    fixture
}

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN qty AS int",
            "CREATE TABLE items (id int, q qty)",
            "INSERT INTO items VALUES (1, 5), (2, NULL)",
        ],
    )
}

#[test]
fn should_reject_add_check_when_existing_rows_violate_it_and_publish_nothing() {
    // Arrange
    let fixture = fixture("domain_alter_add_check_fails");

    // Act
    let error = fixture.error("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 3)");
    let accepted = fixture.execute("INSERT INTO items VALUES (3, 10)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
    assert!(
        accepted.is_ok(),
        "failed ADD CHECK left no constraint behind"
    );
}

#[test]
fn should_enforce_an_added_check_on_later_writes() {
    // Arrange
    let fixture = fixture("domain_alter_add_check");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();

    // Act
    let error = fixture.error("INSERT INTO items VALUES (3, 500)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_set_not_null_when_existing_rows_hold_null_and_publish_nothing() {
    // Arrange
    let fixture = fixture("domain_alter_set_not_null_fails");

    // Act
    let error = fixture.error("ALTER DOMAIN qty SET NOT NULL");
    let accepted = fixture.execute("INSERT INTO items VALUES (3, NULL)");

    // Assert
    assert_eq!(error.sql_state(), "23502");
    assert!(accepted.is_ok(), "failed SET NOT NULL left no flag behind");
}

#[test]
fn should_lift_not_null_with_drop_not_null() {
    // Arrange
    let fixture = sql_fixture(
        "domain_alter_drop_not_null",
        &["CREATE DOMAIN req AS text NOT NULL"],
    );

    // Act
    fixture.execute("ALTER DOMAIN req DROP NOT NULL").unwrap();
    let rows = fixture.rows("SELECT NULL::req");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_apply_a_new_domain_default_to_later_inserts() {
    // Arrange
    let fixture = fixture("domain_alter_set_default");
    fixture.execute("ALTER DOMAIN qty SET DEFAULT 42").unwrap();

    // Act
    fixture
        .execute("INSERT INTO items (id) VALUES (9)")
        .unwrap();
    let rows = fixture.rows("SELECT q FROM items WHERE id = 9");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(42)]]);
}

#[test]
fn should_stop_applying_a_dropped_domain_default() {
    // Arrange
    let fixture = fixture("domain_alter_drop_default");
    fixture.execute("ALTER DOMAIN qty SET DEFAULT 42").unwrap();
    fixture.execute("ALTER DOMAIN qty DROP DEFAULT").unwrap();

    // Act
    fixture
        .execute("INSERT INTO items (id) VALUES (9)")
        .unwrap();
    let rows = fixture.rows("SELECT q FROM items WHERE id = 9");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_stop_enforcing_a_dropped_constraint() {
    // Arrange
    let fixture = with_small("domain_alter_drop_constraint");

    // Act
    fixture
        .execute("ALTER DOMAIN qty DROP CONSTRAINT small")
        .unwrap();
    let accepted = fixture.execute("INSERT INTO items VALUES (3, 500)");

    // Assert
    assert!(accepted.is_ok(), "dropped constraint no longer enforces");
}

#[test]
fn should_tolerate_drop_constraint_if_exists_for_a_missing_constraint() {
    // Arrange
    let fixture = with_small("domain_alter_drop_constraint_if_exists");

    // Act
    let dropped = fixture.execute("ALTER DOMAIN qty DROP CONSTRAINT IF EXISTS nothing");

    // Assert
    assert!(dropped.is_ok(), "IF EXISTS tolerates a missing constraint");
}

#[test]
fn should_reject_dropping_a_missing_constraint_with_sqlstate_42704() {
    // Arrange
    let fixture = with_small("domain_alter_drop_missing_constraint");

    // Act
    let error = fixture.error("ALTER DOMAIN qty DROP CONSTRAINT nothing");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_enforce_a_not_valid_check_on_insert() {
    // Arrange
    let fixture = fixture("domain_alter_not_valid_insert");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 3) NOT VALID")
        .unwrap();

    // Act
    let insert = fixture.error("INSERT INTO items VALUES (3, 10)");

    // Assert
    assert_eq!(insert.sql_state(), "23514");
}

#[test]
fn should_reject_validating_a_check_that_an_existing_row_violates() {
    // Arrange
    let fixture = fixture("domain_alter_not_valid_validate");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 3) NOT VALID")
        .unwrap();

    // Act
    let validate = fixture.error("ALTER DOMAIN qty VALIDATE CONSTRAINT small");

    // Assert
    assert_eq!(
        validate.sql_state(),
        "23514",
        "row 1 still violates the check"
    );
}

#[test]
fn should_reject_an_update_assigning_a_value_that_violates_a_not_valid_check() {
    // Arrange
    let fixture = fixture("domain_alter_not_valid_update_assigned");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT lt3 CHECK (VALUE < 3) NOT VALID")
        .unwrap();

    // Act
    let error = fixture.error("UPDATE items SET q = 7 WHERE id = 1");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_update_another_column_when_a_not_valid_check_is_violated_by_an_unchanged_value() {
    // Arrange
    let fixture = fixture("domain_alter_not_valid_update_unchanged");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT lt3 CHECK (VALUE < 3) NOT VALID")
        .unwrap();

    // Act
    let updated = fixture.execute("UPDATE items SET id = 10 WHERE id = 1");

    // Assert
    assert!(updated.is_ok(), "an unchanged legacy value stays updatable");
}

#[test]
fn should_reject_an_upsert_update_assigning_a_value_that_violates_a_not_valid_check() {
    // Arrange
    let fixture = sql_fixture(
        "domain_alter_not_valid_upsert",
        &[
            "CREATE DOMAIN qty AS int",
            "CREATE TABLE items (id int PRIMARY KEY, q qty)",
            "INSERT INTO items VALUES (1, 5)",
            "ALTER DOMAIN qty ADD CONSTRAINT lt3 CHECK (VALUE < 3) NOT VALID",
        ],
    );

    // Act
    let error =
        fixture.error("INSERT INTO items VALUES (1, 1) ON CONFLICT (id) DO UPDATE SET q = 7");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_validate_a_not_valid_check_once_rows_comply() {
    // Arrange
    let fixture = fixture("domain_alter_validate");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 3) NOT VALID")
        .unwrap();
    fixture
        .execute("UPDATE items SET q = 1 WHERE id = 1")
        .unwrap();

    // Act
    let validated = fixture.execute("ALTER DOMAIN qty VALIDATE CONSTRAINT small");

    // Assert
    assert!(
        validated.is_ok(),
        "VALIDATE succeeds when every row complies"
    );
}

#[test]
fn should_report_the_new_name_after_a_constraint_rename() {
    // Arrange
    let fixture = fixture("domain_alter_rename_constraint");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();
    fixture
        .execute("ALTER DOMAIN qty RENAME CONSTRAINT small TO tiny")
        .unwrap();

    // Act
    let message = fixture.error("SELECT (500)::qty").to_string();

    // Assert
    assert!(message.contains("tiny"), "violation reports the new name");
}

#[test]
fn should_stop_resolving_the_old_name_after_a_domain_rename() {
    // Arrange
    let fixture = fixture("domain_alter_rename_old_name");
    fixture
        .execute("ALTER DOMAIN qty RENAME TO amount")
        .unwrap();

    // Act
    let old_name = fixture.execute("SELECT (1)::qty");

    // Assert
    assert!(old_name.is_err(), "the old name no longer resolves");
}

#[test]
fn should_keep_enforcing_column_rules_after_a_domain_rename() {
    // Arrange
    let fixture = fixture("domain_alter_rename_rules");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();
    fixture
        .execute("ALTER DOMAIN qty RENAME TO amount")
        .unwrap();

    // Act
    let error = fixture.error("INSERT INTO items VALUES (3, 500)");

    // Assert
    assert_eq!(error.sql_state(), "23514", "the column keeps its rules");
}

#[test]
fn should_reject_a_duplicate_constraint_name_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture("domain_alter_duplicate_constraint");
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();

    // Act
    let error = fixture.error("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE > -5)");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_alter_domain_owner_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("domain_alter_owner");

    // Act
    let error = fixture.error("ALTER DOMAIN qty OWNER TO root");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_roll_back_an_alter_domain_in_a_transaction() {
    // Arrange
    let fixture = fixture("domain_alter_rollback");

    // Act
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 100)")
        .unwrap();
    fixture.execute("ROLLBACK").unwrap();
    let accepted = fixture.execute("INSERT INTO items VALUES (3, 500)");

    // Assert
    assert!(
        accepted.is_ok(),
        "rolled-back ALTER DOMAIN left no constraint"
    );
}

#[test]
fn should_validate_staged_rows_of_the_same_transaction_when_adding_a_check() {
    // Arrange
    let fixture = fixture("domain_alter_staged_rows");
    fixture.execute("BEGIN").unwrap();
    fixture.execute("INSERT INTO items VALUES (9, 99)").unwrap();

    // Act
    let error = fixture.error("ALTER DOMAIN qty ADD CONSTRAINT small CHECK (VALUE < 3)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_report_the_check_that_sorts_first_by_name() {
    // Arrange
    let fixture = sql_fixture(
        "domain_check_ordering",
        &["CREATE DOMAIN d AS int CONSTRAINT zz CHECK (VALUE > 10) CONSTRAINT aa CHECK (VALUE > 20)"],
    );

    // Act
    let message = fixture.error("SELECT (1)::d").to_string();

    // Assert
    assert!(message.contains("\"aa\""), "aa sorts before zz: {message}");
}

#[test]
fn should_number_unnamed_checks_after_the_first() {
    // Arrange
    let fixture = sql_fixture(
        "domain_unnamed_checks",
        &["CREATE DOMAIN multi AS int CHECK (VALUE > 0) CHECK (VALUE < 10)"],
    );

    // Act
    let message = fixture.error("SELECT (50)::multi").to_string();

    // Assert
    assert!(
        message.contains("multi_check1"),
        "second check is numbered: {message}"
    );
}
