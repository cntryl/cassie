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

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_allow_drop_domain_once_no_column_depends_on_it() {
    // Arrange
    let fixture = fixture_with_posint("domain_drop_after_table");
    fixture.rows("CREATE TABLE items (id int, qty posint)");
    fixture.execute("DROP TABLE items").unwrap();

    // Act
    let dropped = fixture.execute("DROP DOMAIN posint");

    // Assert
    assert!(dropped.is_ok(), "nothing depends on the domain any more");
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

#[test]
fn should_reject_create_type_over_an_existing_domain_name() {
    // Arrange
    let fixture = fixture_with_posint("domain_name_vs_create_type");

    // Act
    let error = fixture.error("CREATE TYPE posint AS (a int)");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_create_table_over_an_existing_domain_name() {
    // Arrange
    let fixture = fixture_with_posint("domain_name_vs_create_table");

    // Act
    let result = fixture.execute("CREATE TABLE posint (id int)");

    // Assert
    assert!(result.is_err(), "a domain owns the relation name");
}

#[test]
fn should_skip_create_table_if_not_exists_over_an_existing_domain_name() {
    // Arrange
    let fixture = fixture_with_posint("domain_name_vs_create_table_if_not_exists");

    // Act
    let result = fixture.execute("CREATE TABLE IF NOT EXISTS posint (id int)");

    // Assert
    assert!(result.is_ok(), "IF NOT EXISTS skips over the domain name");
}

#[test]
fn should_reject_create_domain_over_an_existing_sequence_name() {
    // Arrange
    let fixture = sql_fixture("domain_name_vs_sequence", &["CREATE SEQUENCE posint"]);

    // Act
    let error = fixture.error(POSITIVE);

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_create_domain_over_an_existing_table_name() {
    // Arrange
    let fixture = sql_fixture("domain_name_vs_table", &["CREATE TABLE posint (id int)"]);

    // Act
    let error = fixture.error(POSITIVE);

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_adding_a_not_null_domain_column_to_a_populated_table() {
    // Arrange
    let fixture = sql_fixture(
        "domain_add_column_not_null",
        &[
            "CREATE DOMAIN req AS int NOT NULL",
            "CREATE TABLE items (id int)",
            "INSERT INTO items VALUES (1)",
        ],
    );

    // Act
    let error = fixture.error("ALTER TABLE items ADD COLUMN r req");

    // Assert
    assert_eq!(error.sql_state(), "23502");
}

#[test]
fn should_allow_adding_a_not_null_domain_column_to_an_empty_table() {
    // Arrange
    let fixture = sql_fixture(
        "domain_add_column_empty",
        &[
            "CREATE DOMAIN req AS int NOT NULL",
            "CREATE TABLE items (id int)",
        ],
    );

    // Act
    let added = fixture.execute("ALTER TABLE items ADD COLUMN r req");

    // Assert
    assert!(added.is_ok(), "no rows need a backfill");
}

#[test]
fn should_backfill_the_domain_default_when_adding_a_column() {
    // Arrange
    let fixture = sql_fixture(
        "domain_add_column_default",
        &[
            "CREATE DOMAIN nine AS int NOT NULL DEFAULT 9",
            "CREATE TABLE items (id int)",
            "INSERT INTO items VALUES (1)",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE items ADD COLUMN d nine")
        .unwrap();
    let rows = fixture.rows("SELECT d FROM items");

    // Assert
    assert_eq!(rows, vec![vec![cassie::types::Value::Int64(9)]]);
}

#[test]
fn should_reject_a_backfilled_default_that_violates_the_domain_check() {
    // Arrange
    let fixture = sql_fixture(
        "domain_add_column_check",
        &[
            "CREATE DOMAIN small AS int CHECK (VALUE < 5)",
            "CREATE TABLE items (id int)",
            "INSERT INTO items VALUES (1)",
        ],
    );

    // Act
    let error = fixture.error("ALTER TABLE items ADD COLUMN s small DEFAULT 50");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_round_trip_domains_over_numeric_jsonb_or_timestamp_bases() {
    // Arrange
    let fixture = sql_fixture(
        "domain_extended_bases",
        &[
            "CREATE DOMAIN money2 AS numeric(10,2) CHECK (VALUE >= 0)",
            "CREATE DOMAIN doc AS jsonb",
            "CREATE DOMAIN moment AS timestamp",
            "CREATE TABLE t (id int, m money2, d doc, ts moment)",
            "INSERT INTO t VALUES (1, 12.345, '{\"a\": 1}', '2026-01-02 03:04:05')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT m::text, d::text, ts::text FROM t");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            cassie::types::Value::String("12.35".into()),
            cassie::types::Value::String("{\"a\": 1}".into()),
            cassie::types::Value::String("2026-01-02 03:04:05".into()),
        ]]
    );
}
