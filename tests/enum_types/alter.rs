//! ALTER TYPE on enums: ADD VALUE, RENAME VALUE, RENAME TO, SET SCHEMA, OWNER TO.
//! Authored against the PostgreSQL 18.6 oracle; not yet executed.

use crate::support_enum_types::{scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const ORDERED_LABELS: &str = "SELECT enumlabel FROM pg_enum \
     WHERE enumtypid = 'state_code'::regtype ORDER BY enumsortorder";
const LABELS_WITH_ORDER: &str = "SELECT enumlabel || ':' || enumsortorder::text FROM pg_enum \
     WHERE enumtypid = 'state_code'::regtype ORDER BY enumsortorder";

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE state_code AS ENUM ('open', 'closed')",
            "CREATE TABLE jobs (id int PRIMARY KEY, st state_code)",
            "INSERT INTO jobs VALUES (1, 'open'), (2, 'closed'), (3, NULL)",
        ],
    )
}

#[test]
fn should_append_a_trailing_value_with_the_next_whole_sort_order() {
    // Arrange
    let fixture = fixture("enum_alter_add_trailing");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Assert
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["open:1", "closed:2", "archived:3"]
    );
}

#[test]
fn should_place_a_value_before_a_neighbour_at_the_midpoint_sort_order() {
    // Arrange
    let fixture = fixture("enum_alter_add_before");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'pending' BEFORE 'closed'")
        .unwrap();
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived'")
        .unwrap();

    // Assert
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["open:1", "pending:1.5", "closed:2", "archived:3"]
    );
}

#[test]
fn should_place_a_value_after_a_neighbour_at_the_midpoint_sort_order() {
    // Arrange
    let fixture = fixture("enum_alter_add_after");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'pending' AFTER 'open'")
        .unwrap();

    // Assert
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["open:1", "pending:1.5", "closed:2"]
    );
}

#[test]
fn should_place_a_value_before_the_first_label_one_below_it() {
    // Arrange
    let fixture = fixture("enum_alter_add_head");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'draft' BEFORE 'open'")
        .unwrap();

    // Assert
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["draft:0", "open:1", "closed:2"]
    );
}

#[test]
fn should_place_a_value_after_the_last_label_one_above_it() {
    // Arrange
    let fixture = fixture("enum_alter_add_after_last");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'archived' AFTER 'closed'")
        .unwrap();

    // Assert
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["open:1", "closed:2", "archived:3"]
    );
}

#[test]
fn should_add_the_first_value_of_an_empty_enum_with_sort_order_one() {
    // Arrange
    let fixture = sql_fixture(
        "enum_alter_add_to_empty",
        &["CREATE TYPE state_code AS ENUM ()"],
    );

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'first'")
        .unwrap();

    // Assert
    assert_eq!(strings(&fixture, LABELS_WITH_ORDER), vec!["first:1"]);
}

#[test]
fn should_keep_existing_rows_and_their_order_when_a_value_is_added_before() {
    // Arrange
    let fixture = fixture("enum_alter_add_keeps_rows");

    // Act
    fixture
        .execute("ALTER TYPE state_code ADD VALUE 'pending' BEFORE 'closed'")
        .unwrap();
    fixture
        .execute("INSERT INTO jobs VALUES (4, 'pending')")
        .unwrap();
    let ordered = strings(
        &fixture,
        "SELECT id::text || '=' || st::text FROM jobs WHERE st IS NOT NULL ORDER BY st",
    );

    // Assert
    assert_eq!(ordered, vec!["1=open", "4=pending", "2=closed"]);
}

#[test]
fn should_not_change_anything_for_add_value_if_not_exists_on_an_existing_label() {
    // Arrange
    let fixture = fixture("enum_alter_add_if_not_exists");
    let before_id = scalar(
        &fixture,
        "SELECT oid::text FROM pg_enum WHERE enumlabel = 'open'",
    );

    // Act
    let repeated = fixture.execute("ALTER TYPE state_code ADD VALUE IF NOT EXISTS 'open'");
    let with_neighbour =
        fixture.execute("ALTER TYPE state_code ADD VALUE IF NOT EXISTS 'closed' BEFORE 'open'");
    let after_id = scalar(
        &fixture,
        "SELECT oid::text FROM pg_enum WHERE enumlabel = 'open'",
    );

    // Assert
    assert!(
        repeated.is_ok(),
        "IF NOT EXISTS emits a notice, not an error"
    );
    assert!(
        with_neighbour.is_ok(),
        "an existing label short-circuits the position"
    );
    assert_eq!(
        strings(&fixture, LABELS_WITH_ORDER),
        vec!["open:1", "closed:2"]
    );
    assert_eq!(before_id, after_id, "the member identity is unchanged");
}

#[test]
fn should_reject_a_duplicate_add_value_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture("enum_alter_add_duplicate");

    // Act
    let error = fixture.error("ALTER TYPE state_code ADD VALUE 'open'");

    // Assert
    assert_eq!(error.sql_state(), "42710");
    assert_eq!(strings(&fixture, ORDERED_LABELS), vec!["open", "closed"]);
}

#[test]
fn should_reject_an_unknown_neighbour_with_sqlstate_22023() {
    // Arrange
    let fixture = fixture("enum_alter_unknown_neighbour");

    // Act
    let before = fixture.error("ALTER TYPE state_code ADD VALUE 'x' BEFORE 'nope'");
    let after = fixture.error("ALTER TYPE state_code ADD VALUE 'x' AFTER 'nope'");
    let guarded = fixture.error("ALTER TYPE state_code ADD VALUE IF NOT EXISTS 'x' BEFORE 'nope'");

    // Assert
    assert_eq!(before.sql_state(), "22023");
    assert_eq!(after.sql_state(), "22023");
    assert_eq!(guarded.sql_state(), "22023");
    assert_eq!(strings(&fixture, ORDERED_LABELS), vec!["open", "closed"]);
}

#[test]
fn should_reject_an_added_label_longer_than_63_bytes_with_sqlstate_42602() {
    // Arrange
    let fixture = fixture("enum_alter_add_long_label");
    let label = "a".repeat(64);

    // Act
    let error = fixture.error(&format!("ALTER TYPE state_code ADD VALUE '{label}'"));

    // Assert
    assert_eq!(error.sql_state(), "42602");
}

#[test]
fn should_keep_order_when_many_values_are_inserted_into_one_gap() {
    // Arrange
    let fixture = fixture("enum_alter_renumber");
    fixture
        .execute("INSERT INTO jobs VALUES (10, 'closed')")
        .unwrap();

    // Act
    for step in 1..=40 {
        fixture
            .execute(&format!(
                "ALTER TYPE state_code ADD VALUE 'v{step}' AFTER 'open'"
            ))
            .unwrap();
    }
    let labels = strings(&fixture, ORDERED_LABELS);
    let orders = strings(
        &fixture,
        "SELECT enumsortorder::text FROM pg_enum \
         WHERE enumtypid = 'state_code'::regtype ORDER BY enumsortorder",
    );
    let stored = scalar(&fixture, "SELECT st::text FROM jobs WHERE id = 10");

    // Assert
    let mut expected = vec!["open".to_owned()];
    expected.extend((1..=40).rev().map(|step| format!("v{step}")));
    expected.push("closed".to_owned());
    assert_eq!(labels, expected, "relative order survives any renumbering");
    let parsed: Vec<f64> = orders.iter().map(|order| order.parse().unwrap()).collect();
    assert!(
        parsed.windows(2).all(|pair| pair[0] < pair[1]),
        "sort orders stay strictly increasing"
    );
    assert_eq!(
        stored.as_deref(),
        Some("closed"),
        "stored rows are unaffected"
    );
}

#[test]
fn should_show_the_new_label_for_stored_rows_after_rename_value() {
    // Arrange
    let fixture = fixture("enum_alter_rename_value");

    // Act
    fixture
        .execute("ALTER TYPE state_code RENAME VALUE 'closed' TO 'done'")
        .unwrap();
    let rows = strings(
        &fixture,
        "SELECT coalesce(st::text, 'NULL') FROM jobs ORDER BY id",
    );

    // Assert
    assert_eq!(rows, vec!["open", "done", "NULL"]);
    assert_eq!(strings(&fixture, ORDERED_LABELS), vec!["open", "done"]);
}

#[test]
fn should_reject_the_old_label_as_input_after_rename_value() {
    // Arrange
    let fixture = fixture("enum_alter_rename_old_input");
    fixture
        .execute("ALTER TYPE state_code RENAME VALUE 'closed' TO 'done'")
        .unwrap();

    // Act
    let cast = fixture.error("SELECT 'closed'::state_code");
    let insert = fixture.error("INSERT INTO jobs VALUES (9, 'closed')");
    let accepted = scalar(&fixture, "SELECT 'done'::state_code::text");

    // Assert
    assert_eq!(cast.sql_state(), "22P02");
    assert_eq!(insert.sql_state(), "22P02");
    assert_eq!(accepted.as_deref(), Some("done"));
}

#[test]
fn should_keep_member_identity_and_order_across_rename_value() {
    // Arrange
    let fixture = fixture("enum_alter_rename_identity");
    let before = scalar(
        &fixture,
        "SELECT oid::text FROM pg_enum WHERE enumlabel = 'closed'",
    );

    // Act
    fixture
        .execute("ALTER TYPE state_code RENAME VALUE 'closed' TO 'done'")
        .unwrap();
    let after = scalar(
        &fixture,
        "SELECT oid::text FROM pg_enum WHERE enumlabel = 'done'",
    );
    let ordered = strings(&fixture, LABELS_WITH_ORDER);

    // Assert
    assert_eq!(before, after, "pg_enum.oid is the immutable member ID");
    assert_eq!(ordered, vec!["open:1", "done:2"]);
}

#[test]
fn should_reject_rename_value_to_an_existing_label_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture("enum_alter_rename_collision");

    // Act
    let error = fixture.error("ALTER TYPE state_code RENAME VALUE 'open' TO 'closed'");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_rename_value_of_an_unknown_label_with_sqlstate_22023() {
    // Arrange
    let fixture = fixture("enum_alter_rename_unknown");

    // Act
    let error = fixture.error("ALTER TYPE state_code RENAME VALUE 'nope' TO 'x'");

    // Assert
    assert_eq!(error.sql_state(), "22023");
}

#[test]
fn should_reject_a_renamed_label_longer_than_63_bytes_with_sqlstate_42602() {
    // Arrange
    let fixture = fixture("enum_alter_rename_long_label");
    let label = "a".repeat(64);

    // Act
    let error = fixture.error(&format!(
        "ALTER TYPE state_code RENAME VALUE 'open' TO '{label}'"
    ));

    // Assert
    assert_eq!(error.sql_state(), "42602");
}

#[test]
fn should_not_offer_dropping_an_enum_label() {
    // Arrange
    let fixture = fixture("enum_alter_drop_value");

    // Act
    let rejected = fixture.execute("ALTER TYPE state_code DROP VALUE 'open'");

    // Assert
    assert!(rejected.is_err(), "enum labels can never be dropped");
    assert_eq!(strings(&fixture, ORDERED_LABELS), vec!["open", "closed"]);
}

#[test]
fn should_resolve_an_enum_under_its_new_name_after_rename_to() {
    // Arrange
    let fixture = fixture("enum_alter_rename_to");

    // Act
    fixture
        .execute("ALTER TYPE state_code RENAME TO task_state")
        .unwrap();
    let renamed = scalar(&fixture, "SELECT 'open'::task_state::text");
    let old_name = fixture.execute("SELECT 'open'::state_code::text");
    let column_type = scalar(
        &fixture,
        "SELECT pg_typeof(st)::text FROM jobs WHERE id = 1",
    );
    let stored = strings(
        &fixture,
        "SELECT st::text FROM jobs WHERE id <= 2 ORDER BY id",
    );

    // Assert
    assert_eq!(renamed.as_deref(), Some("open"));
    assert!(old_name.is_err(), "the old name must no longer resolve");
    assert_eq!(column_type.as_deref(), Some("task_state"));
    assert_eq!(stored, vec!["open", "closed"]);
}

#[test]
fn should_reject_rename_to_a_name_used_by_another_type_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture("enum_alter_rename_to_collision");
    fixture
        .execute("CREATE TYPE other_kind AS ENUM ('p')")
        .unwrap();

    // Act
    let error = fixture.error("ALTER TYPE state_code RENAME TO other_kind");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_move_an_enum_to_another_schema_with_set_schema() {
    // Arrange
    let fixture = fixture("enum_alter_set_schema");
    fixture.execute("CREATE SCHEMA archive").unwrap();

    // Act
    fixture
        .execute("ALTER TYPE state_code SET SCHEMA archive")
        .unwrap();
    let qualified = scalar(&fixture, "SELECT 'open'::archive.state_code::text");
    let unqualified = fixture.execute("SELECT 'open'::state_code::text");
    let stored = strings(
        &fixture,
        "SELECT st::text FROM jobs WHERE id <= 2 ORDER BY id",
    );
    let column_type = scalar(
        &fixture,
        "SELECT pg_typeof(st)::text FROM jobs WHERE id = 1",
    );

    // Assert
    assert_eq!(qualified.as_deref(), Some("open"));
    assert!(unqualified.is_err(), "the type left the search path");
    assert_eq!(stored, vec!["open", "closed"]);
    assert_eq!(column_type.as_deref(), Some("archive.state_code"));
}

#[test]
fn should_reject_owner_to_an_unknown_role_with_sqlstate_42704() {
    // Arrange
    let fixture = fixture("enum_alter_owner_unknown");

    // Act
    let error = fixture.error("ALTER TYPE state_code OWNER TO nobody");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_reject_altering_a_missing_enum_with_sqlstate_42704() {
    // Arrange
    let fixture = fixture("enum_alter_missing");

    // Act
    let error = fixture.error("ALTER TYPE missing_kind ADD VALUE 'x'");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}
