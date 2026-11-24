//! Enum DDL inside explicit transactions, savepoints and across sessions.
//! Authored against the PostgreSQL 18.6 oracle.

use crate::support_enum_types::{execute_on, scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const RANGE: &str = "SELECT enum_range(NULL::state_code)::text";

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE state_code AS ENUM ('open', 'closed')",
            "CREATE TABLE jobs (id int PRIMARY KEY, st state_code)",
            "INSERT INTO jobs VALUES (1, 'open')",
        ],
    )
}

#[test]
fn should_roll_back_create_type_with_its_transaction() {
    // Arrange
    let fixture = sql_fixture("enum_tx_create_rollback", &[]);
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE TYPE state_code AS ENUM ('open', 'closed')")
        .unwrap();
    let visible_inside = scalar(&fixture, "SELECT 'open'::state_code::text");

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let after = fixture.execute("SELECT 'open'::state_code::text");
    let recreated = fixture.execute("CREATE TYPE state_code AS ENUM ('again')");

    // Assert
    assert_eq!(visible_inside.as_deref(), Some("open"));
    assert!(after.is_err(), "a rolled-back type must not resolve");
    assert!(recreated.is_ok(), "the name is free again after rollback");
}

#[test]
fn should_keep_a_type_created_in_a_committed_transaction() {
    // Arrange
    let fixture = sql_fixture("enum_tx_create_commit", &[]);
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE TYPE state_code AS ENUM ('open', 'closed')")
        .unwrap();

    // Act
    fixture.execute("COMMIT").unwrap();
    let after = scalar(&fixture, "SELECT 'closed'::state_code::text");

    // Assert
    assert_eq!(after.as_deref(), Some("closed"));
}

#[test]
fn should_reject_using_a_label_added_in_the_same_transaction_with_sqlstate_55p04() {
    // Arrange
    let fixture = fixture("enum_tx_add_then_use");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    let cast = fixture.error("SELECT 'archived'::state_code");
    let aborted = fixture.error("SELECT 1");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert_eq!(cast.sql_state(), "55P04");
    assert_eq!(
        aborted.sql_state(),
        "25P02",
        "the failed use aborts the transaction"
    );
    assert_eq!(scalar(&fixture, RANGE).as_deref(), Some("{open,closed}"));
}

#[test]
fn should_reject_inserting_a_label_added_in_the_same_transaction_with_sqlstate_55p04() {
    // Arrange
    let fixture = fixture("enum_tx_add_then_insert");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    let insert = fixture.error("INSERT INTO jobs VALUES (2, 'archived')");
    fixture.execute("ROLLBACK").unwrap();
    let rows = strings(&fixture, "SELECT id::text FROM jobs ORDER BY id");

    // Assert
    assert_eq!(insert.sql_state(), "55P04");
    assert_eq!(rows, vec!["1"]);
}

#[test]
fn should_reject_enum_functions_returning_a_label_added_in_the_same_transaction() {
    // Arrange
    let fixture = fixture("enum_tx_add_then_range");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();
    fixture.execute("SAVEPOINT probe").unwrap();

    // Act
    let first = scalar(&fixture, "SELECT enum_first(NULL::state_code)::text");
    let range = fixture.error(RANGE);
    fixture.execute("ROLLBACK TO SAVEPOINT probe").unwrap();
    let last = fixture.error("SELECT enum_last(NULL::state_code)::text");
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert_eq!(first.as_deref(), Some("open"));
    assert_eq!(range.sql_state(), "55P04");
    assert_eq!(last.sql_state(), "55P04");
}

#[test]
fn should_allow_existing_labels_after_add_value_in_the_same_transaction() {
    // Arrange
    let fixture = fixture("enum_tx_add_then_existing");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    let existing = scalar(&fixture, "SELECT 'closed'::state_code::text");
    fixture
        .execute("INSERT INTO jobs VALUES (2, 'closed')")
        .unwrap();
    fixture.execute("COMMIT").unwrap();
    let rows = strings(&fixture, "SELECT st::text FROM jobs ORDER BY id");

    // Assert
    assert_eq!(existing.as_deref(), Some("closed"));
    assert_eq!(rows, vec!["open", "closed"]);
}

#[test]
fn should_use_an_added_label_in_a_new_transaction_after_commit() {
    // Arrange
    let fixture = fixture("enum_tx_add_commit_then_use");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();
    fixture.execute("COMMIT").unwrap();

    // Act
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("INSERT INTO jobs VALUES (2, 'archived')")
        .unwrap();
    fixture.execute("COMMIT").unwrap();
    let rows = strings(&fixture, "SELECT st::text FROM jobs ORDER BY id");

    // Assert
    assert_eq!(rows, vec!["open", "archived"]);
}

#[test]
fn should_use_an_added_label_immediately_when_the_type_was_created_in_the_same_transaction() {
    // Arrange
    let fixture = sql_fixture("enum_tx_fresh_type_add_use", &[]);
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE TYPE state_code AS ENUM ('a')")
        .unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'b'")
        .unwrap();

    // Act
    let usable = scalar(&fixture, "SELECT 'b'::state_code::text");
    fixture.execute("COMMIT").unwrap();
    let after = scalar(&fixture, RANGE);

    // Assert
    assert_eq!(usable.as_deref(), Some("b"));
    assert_eq!(after.as_deref(), Some("{a,b}"));
}

#[test]
fn should_discard_an_added_label_when_the_transaction_rolls_back() {
    // Arrange
    let fixture = fixture("enum_tx_add_rollback");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let range = scalar(&fixture, RANGE);
    let cast = fixture.error("SELECT 'archived'::state_code");
    let readded = fixture.execute("ALTER TYPE state_code ADD VALUE 'archived'");

    // Assert
    assert_eq!(range.as_deref(), Some("{open,closed}"));
    assert_eq!(cast.sql_state(), "22P02");
    assert!(readded.is_ok(), "the rolled-back label can be added again");
}

#[test]
fn should_discard_a_rename_value_when_the_transaction_rolls_back() {
    // Arrange
    let fixture = fixture("enum_tx_rename_rollback");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code RENAME VALUE 'open' TO 'started'")
        .unwrap();
    let inside = scalar(&fixture, "SELECT st::text FROM jobs WHERE id = 1");

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let after = scalar(&fixture, "SELECT st::text FROM jobs WHERE id = 1");

    // Assert
    assert_eq!(inside.as_deref(), Some("started"));
    assert_eq!(after.as_deref(), Some("open"));
}

#[test]
fn should_keep_the_enum_when_a_drop_type_rolls_back() {
    // Arrange
    let fixture = sql_fixture(
        "enum_tx_drop_rollback",
        &["CREATE TYPE state_code AS ENUM ('open')"],
    );
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP TYPE state_code").unwrap();
    let gone_inside = fixture.execute("SELECT 'open'::state_code::text");
    fixture.execute("ROLLBACK").unwrap();

    // Act
    let after = scalar(&fixture, "SELECT 'open'::state_code::text");

    // Assert
    assert!(gone_inside.is_err());
    assert_eq!(after.as_deref(), Some("open"));
}

#[test]
fn should_discard_an_added_label_when_a_savepoint_rolls_back() {
    // Arrange
    let fixture = fixture("enum_tx_savepoint");
    fixture.execute("BEGIN").unwrap();
    fixture.execute("SAVEPOINT before_add").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    fixture.execute("ROLLBACK TO before_add").unwrap();
    let inside = scalar(&fixture, RANGE);
    fixture.execute("COMMIT").unwrap();
    let after = scalar(&fixture, RANGE);

    // Assert
    assert_eq!(inside.as_deref(), Some("{open,closed}"));
    assert_eq!(after.as_deref(), Some("{open,closed}"));
}

#[test]
fn should_keep_labels_added_before_a_savepoint_when_it_rolls_back() {
    // Arrange
    let fixture = fixture("enum_tx_savepoint_partial");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'first_added'")
        .unwrap();
    fixture.execute("SAVEPOINT mid").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'second_added'")
        .unwrap();

    // Act
    fixture.execute("ROLLBACK TO mid").unwrap();
    fixture.execute("COMMIT").unwrap();
    let after = scalar(&fixture, RANGE);

    // Assert
    assert_eq!(after.as_deref(), Some("{open,closed,first_added}"));
}

#[test]
fn should_show_other_sessions_an_added_label_only_after_commit() {
    // Arrange
    let fixture = fixture("enum_tx_cross_session_add");
    let other = fixture.cassie.create_session("tester", None);
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Act
    let before_commit = execute_on(&other, &fixture, RANGE).expect("read before commit");
    fixture.execute("COMMIT").unwrap();
    let after_commit = execute_on(&other, &fixture, RANGE).expect("read after commit");
    let usable = execute_on(&other, &fixture, "SELECT 'archived'::state_code::text");

    // Assert
    assert_eq!(
        before_commit.rows()[0][0].as_str(),
        Some("{open,closed}"),
        "an uncommitted label is invisible to other sessions"
    );
    assert_eq!(
        after_commit.rows()[0][0].as_str(),
        Some("{open,closed,archived}")
    );
    assert!(usable.is_ok(), "the committed label is usable elsewhere");
}

#[test]
fn should_show_other_sessions_a_created_type_only_after_commit() {
    // Arrange
    let fixture = sql_fixture("enum_tx_cross_session_create", &[]);
    let other = fixture.cassie.create_session("tester", None);
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("CREATE TYPE state_code AS ENUM ('open')")
        .unwrap();

    // Act
    let before_commit = execute_on(&other, &fixture, "SELECT 'open'::state_code::text");
    fixture.execute("COMMIT").unwrap();
    let after_commit = execute_on(&other, &fixture, "SELECT 'open'::state_code::text");

    // Assert
    assert!(before_commit.is_err(), "an uncommitted type is invisible");
    assert!(after_commit.is_ok(), "a committed type is visible");
}
