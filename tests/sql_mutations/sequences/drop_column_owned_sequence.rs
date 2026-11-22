//! Dropping a column drops the serial or identity sequence it owns.

use cassie::types::Value;

use super::support_sql_fixture::{sql_fixture, SqlFixture};

fn sequence_count(fixture: &SqlFixture, name: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT count(*) FROM pg_class WHERE relname = '{name}' AND relkind = 'S'"
    ))
}

fn one() -> Vec<Vec<Value>> {
    vec![vec![Value::Int64(1)]]
}

fn zero() -> Vec<Vec<Value>> {
    vec![vec![Value::Int64(0)]]
}

#[test]
fn should_allow_adding_a_serial_column_after_dropping_the_same_one() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-serial-readd",
        &["CREATE TABLE s3 (id SERIAL, w INT)"],
    );

    // Act
    fixture
        .execute("ALTER TABLE s3 DROP COLUMN id")
        .expect("drop column");
    let added = fixture.execute("ALTER TABLE s3 ADD COLUMN id SERIAL");

    // Assert
    assert!(added.is_ok());
}

#[test]
fn should_remove_the_sequence_of_a_dropped_serial_column() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-serial-gone",
        &["CREATE TABLE s3 (id SERIAL, w INT)"],
    );

    // Act
    fixture
        .execute("ALTER TABLE s3 DROP COLUMN id")
        .expect("drop column");

    // Assert
    assert_eq!(sequence_count(&fixture, "s3_id_seq"), zero());
    assert!(fixture.execute("SELECT nextval('s3_id_seq')").is_err());
}

#[test]
fn should_remove_the_sequences_of_dropped_wide_serial_columns() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-serial-widths",
        &["CREATE TABLE s7 (a BIGSERIAL, b SMALLSERIAL, w INT)"],
    );

    // Act
    fixture
        .execute("ALTER TABLE s7 DROP COLUMN a")
        .expect("drop bigserial");
    fixture
        .execute("ALTER TABLE s7 DROP COLUMN b")
        .expect("drop smallserial");

    // Assert
    assert_eq!(sequence_count(&fixture, "s7_a_seq"), zero());
    assert_eq!(sequence_count(&fixture, "s7_b_seq"), zero());
}

#[test]
fn should_remove_the_sequence_of_a_dropped_identity_column() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-identity-gone",
        &["CREATE TABLE s4 (id INT GENERATED ALWAYS AS IDENTITY, w INT)"],
    );

    // Act
    fixture
        .execute("ALTER TABLE s4 DROP COLUMN id")
        .expect("drop column");

    // Assert
    assert_eq!(sequence_count(&fixture, "s4_id_seq"), zero());
}

#[test]
fn should_keep_a_separately_created_sequence_a_dropped_column_defaulted_from() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-unowned-sequence",
        &[
            "CREATE SEQUENCE sep",
            "CREATE TABLE s5 (id INT DEFAULT nextval('sep'), w INT)",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE s5 DROP COLUMN id")
        .expect("drop column");

    // Assert
    assert_eq!(fixture.rows("SELECT nextval('sep')"), one());
}

#[test]
fn should_remove_a_sequence_owned_by_the_dropped_column_through_alter_sequence() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-owned-by",
        &[
            "CREATE SEQUENCE own",
            "CREATE TABLE s6 (id INT DEFAULT nextval('own'), w INT)",
            "ALTER SEQUENCE own OWNED BY s6.id",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE s6 DROP COLUMN id")
        .expect("drop column");

    // Assert
    assert_eq!(sequence_count(&fixture, "own"), zero());
}

#[test]
fn should_restore_the_sequence_with_its_value_when_a_dropped_column_rolls_back() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-rollback",
        &["CREATE TABLE s7 (id BIGSERIAL, w INT)"],
    );
    fixture
        .execute("SELECT nextval('s7_id_seq')")
        .expect("advance");
    fixture
        .execute("SELECT nextval('s7_id_seq')")
        .expect("advance");

    // Act
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("ALTER TABLE s7 DROP COLUMN id")
        .expect("drop column");
    fixture.execute("ROLLBACK").expect("rollback");

    // Assert
    assert_eq!(
        fixture.rows("SELECT nextval('s7_id_seq')"),
        vec![vec![Value::Int64(3)]]
    );
}

#[test]
fn should_remove_the_sequence_when_the_serial_column_was_renamed_first() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-renamed-column",
        &["CREATE TABLE s7 (id BIGSERIAL, w INT)"],
    );
    fixture
        .execute("ALTER TABLE s7 RENAME COLUMN id TO ident")
        .expect("rename column");

    // Act
    fixture
        .execute("ALTER TABLE s7 DROP COLUMN ident")
        .expect("drop column");

    // Assert
    assert_eq!(sequence_count(&fixture, "s7_id_seq"), zero());
}

#[test]
fn should_remove_the_sequence_when_the_table_was_renamed_first() {
    // Arrange
    let fixture = sql_fixture(
        "drop-column-renamed-table",
        &["CREATE TABLE s7 (id BIGSERIAL, w INT)"],
    );
    fixture
        .execute("ALTER TABLE s7 RENAME TO s8")
        .expect("rename table");

    // Act
    fixture
        .execute("ALTER TABLE s8 DROP COLUMN id")
        .expect("drop column");

    // Assert
    assert_eq!(sequence_count(&fixture, "s7_id_seq"), zero());
    assert_eq!(sequence_count(&fixture, "s8_id_seq"), zero());
}

#[test]
fn should_report_an_existing_sequence_name_as_a_duplicate_relation() {
    // Arrange
    let fixture = sql_fixture("duplicate-sequence-sqlstate", &["CREATE SEQUENCE dup_seq"]);

    // Act
    let error = fixture.error("CREATE SEQUENCE dup_seq");

    // Assert
    assert_eq!(error.sql_state(), "42P07");
}
