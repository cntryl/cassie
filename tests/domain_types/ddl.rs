//! CREATE DOMAIN and DROP DOMAIN lifecycle, errors and transactional publication.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const POSITIVE: &str = "CREATE DOMAIN posint AS int CHECK (VALUE > 0)";

fn fixture_with_posint(label: &str) -> SqlFixture {
    sql_fixture(label, &[POSITIVE])
}

#[test]
fn should_make_a_created_domain_usable_in_casts() {
    // Arrange
    let fixture = fixture_with_posint("domain_create_cast");

    // Act
    let rows = fixture.rows("SELECT (5)::posint + 1");

    // Assert
    assert_eq!(rows, vec![vec![cassie::types::Value::Int64(6)]]);
}

#[test]
fn should_reject_a_duplicate_domain_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture_with_posint("domain_duplicate");

    // Act
    let error = fixture.error(POSITIVE);

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_an_unknown_base_type_with_sqlstate_42704() {
    // Arrange
    let fixture = sql_fixture("domain_unknown_base", &[]);

    // Act
    let error = fixture.error("CREATE DOMAIN d AS nosuchtype");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_stop_resolving_a_dropped_domain() {
    // Arrange
    let fixture = fixture_with_posint("domain_drop");

    // Act
    let dropped = fixture.execute("DROP DOMAIN posint");
    let after_drop = fixture.execute("SELECT (5)::posint");

    // Assert
    assert!(dropped.is_ok(), "DROP DOMAIN must succeed");
    assert!(after_drop.is_err(), "dropped domain must no longer resolve");
}

#[test]
fn should_accept_drop_domain_if_exists_for_a_missing_domain() {
    // Arrange
    let fixture = sql_fixture("domain_drop_if_exists", &[]);

    // Act
    let dropped = fixture.execute("DROP DOMAIN IF EXISTS nothing_here RESTRICT");
    let missing = fixture.error("DROP DOMAIN nothing_here");

    // Assert
    assert!(dropped.is_ok(), "IF EXISTS tolerates a missing domain");
    assert_eq!(missing.sql_state(), "42704");
}

#[test]
fn should_reject_drop_domain_with_a_dependent_column_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture_with_posint("domain_drop_dependent");
    fixture.rows("CREATE TABLE items (id int, qty posint)");

    // Act
    let error = fixture.error("DROP DOMAIN posint");
    let after_table_drop = fixture
        .execute("DROP TABLE items")
        .and_then(|_| fixture.execute("DROP DOMAIN posint"));

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
    assert!(
        after_table_drop.is_ok(),
        "drop succeeds once nothing depends on it"
    );
}

#[test]
fn should_reject_drop_domain_on_a_composite_type_with_sqlstate_42809() {
    // Arrange
    let fixture = sql_fixture(
        "domain_drop_wrong_kind",
        &["CREATE TYPE address AS (street text, zip int)"],
    );

    // Act
    let error = fixture.error("DROP DOMAIN address");

    // Assert
    assert_eq!(error.sql_state(), "42809");
}

#[test]
fn should_reject_a_domain_over_a_composite_type_with_sqlstate_0a000() {
    // Arrange
    let fixture = sql_fixture(
        "domain_over_composite",
        &["CREATE TYPE address AS (street text, zip int)"],
    );

    // Act
    let error = fixture.error("CREATE DOMAIN d AS address");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_a_domain_over_a_domain_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture_with_posint("domain_over_domain");

    // Act
    let error = fixture.error("CREATE DOMAIN d AS posint");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_accept_a_domain_over_an_array_base_type() {
    // Arrange
    let fixture = sql_fixture("domain_over_array", &[]);

    // Act
    let created = fixture.execute("CREATE DOMAIN tags AS text[] CHECK (VALUE IS NOT NULL)");
    let cast = fixture.execute("SELECT ARRAY['a','b']::tags");

    // Assert
    assert!(created.is_ok(), "array base type is supported");
    assert!(cast.is_ok(), "array domain accepts its base values");
}

#[test]
fn should_not_publish_a_rolled_back_create_domain() {
    // Arrange
    let fixture = sql_fixture("domain_create_rollback", &[]);

    // Act
    fixture.execute("BEGIN").unwrap();
    fixture.execute(POSITIVE).unwrap();
    fixture.execute("ROLLBACK").unwrap();
    let recreated = fixture.execute(POSITIVE);

    // Assert
    assert!(recreated.is_ok(), "rolled-back domain left no trace");
}

#[test]
fn should_restore_a_domain_dropped_in_a_rolled_back_transaction() {
    // Arrange
    let fixture = fixture_with_posint("domain_drop_rollback");

    // Act
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP DOMAIN posint").unwrap();
    fixture.execute("ROLLBACK").unwrap();
    let usable = fixture.execute("SELECT (3)::posint");

    // Assert
    assert!(usable.is_ok(), "rollback keeps the domain");
}

#[test]
fn should_drop_a_domain_with_its_schema_when_nothing_outside_depends_on_it() {
    // Arrange
    let fixture = sql_fixture(
        "domain_schema_cascade",
        &[
            "CREATE SCHEMA s",
            "CREATE DOMAIN s.posint AS int CHECK (VALUE > 0)",
        ],
    );

    // Act
    let dropped = fixture.execute("DROP SCHEMA s CASCADE");
    let after = fixture.execute("SELECT (1)::s.posint");

    // Assert
    assert!(dropped.is_ok(), "DROP SCHEMA CASCADE removes its domains");
    assert!(after.is_err(), "the schema's domain is gone");
}

#[test]
fn should_reject_drop_schema_when_a_table_outside_uses_its_domain() {
    // Arrange
    let fixture = sql_fixture(
        "domain_schema_outside_dependent",
        &[
            "CREATE SCHEMA s",
            "CREATE DOMAIN s.posint AS int CHECK (VALUE > 0)",
            "CREATE TABLE items (id int, qty s.posint)",
        ],
    );

    // Act
    let error = fixture.error("DROP SCHEMA s CASCADE");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}
