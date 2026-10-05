use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::catalog::canonical_relation_name;
use cassie::sql::ast::{CopyFormat, CopyStatement};
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

fn stored_default(fixture: &SqlFixture, table: &str, field: &str) -> serde_json::Value {
    fixture
        .cassie
        .catalog
        .get_constraint(&canonical_relation_name("postgres", "public", table), field)
        .expect("declared column constraint")
        .default_value
        .expect("canonical literal default")
}

fn copy_statement(table: &str, field: &str) -> CopyStatement {
    CopyStatement {
        table: table.to_string(),
        columns: vec!["id".to_string(), field.to_string()],
        format: CopyFormat::Csv,
        header: false,
    }
}

#[test]
fn should_canonicalize_boolean_prefixes_in_create_defaults() {
    // Arrange
    let fixture = metadata_fixture("bool-create-default-prefix", &[]);

    // Act
    let created = fixture.execute(
        "CREATE TABLE bool_default_create (id INT, yes_flag BOOLEAN DEFAULT '  Tr  ', no_flag BOOLEAN DEFAULT 'OF')",
    );

    // Assert
    created.expect("declared Boolean defaults use canonical input vocabulary");
    assert_eq!(
        stored_default(&fixture, "bool_default_create", "yes_flag"),
        true
    );
    assert_eq!(
        stored_default(&fixture, "bool_default_create", "no_flag"),
        false
    );
    let inserted = fixture
        .execute("INSERT INTO bool_default_create (id) VALUES (1) RETURNING yes_flag, no_flag")
        .expect("apply canonical defaults");
    assert_eq!(
        inserted.rows,
        vec![vec![Value::Bool(true), Value::Bool(false)]]
    );
    assert_eq!(inserted.columns[0].type_oid, 16);
    assert_eq!(inserted.columns[1].type_oid, 16);
}

#[test]
fn should_canonicalize_boolean_prefixes_in_set_default() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-alter-set-default-prefix",
        &["CREATE TABLE bool_default_alter (id INT, flag BOOLEAN DEFAULT TRUE)"],
    );

    // Act
    let altered =
        fixture.execute("ALTER TABLE bool_default_alter ALTER COLUMN flag SET DEFAULT '  fa  '");

    // Assert
    altered.expect("SET DEFAULT uses canonical Boolean vocabulary");
    assert_eq!(
        stored_default(&fixture, "bool_default_alter", "flag"),
        false
    );
    assert_eq!(
        fixture.rows("INSERT INTO bool_default_alter (id) VALUES (1) RETURNING flag"),
        vec![vec![Value::Bool(false)]]
    );
}

#[test]
fn should_canonicalize_boolean_prefixes_in_add_column_defaults() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-alter-add-default-prefix",
        &["CREATE TABLE bool_default_added (id INT)"],
    );

    // Act
    let altered =
        fixture.execute("ALTER TABLE bool_default_added ADD COLUMN flag BOOLEAN DEFAULT '  ye  '");

    // Assert
    altered.expect("ADD COLUMN uses declared Boolean default adapter");
    assert_eq!(stored_default(&fixture, "bool_default_added", "flag"), true);
    assert_eq!(
        fixture.rows("INSERT INTO bool_default_added (id) VALUES (1) RETURNING flag"),
        vec![vec![Value::Bool(true)]]
    );
}

#[test]
fn should_copy_canonical_boolean_text_spellings() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-copy-canonical-input",
        &["CREATE TABLE bool_copy_inputs (id INT, flag BOOLEAN)"],
    );
    let statement = copy_statement("bool_copy_inputs", "flag");
    let payload = b"1,y\n2,n\n3,tr\n4,fa\n5,of\n6,\"  YES  \"\n7,\n";

    // Act
    let copied = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, payload);

    // Assert
    assert_eq!(copied.expect("COPY uses canonical Boolean input"), 7);
    assert_eq!(
        fixture.rows("SELECT flag FROM bool_copy_inputs ORDER BY id"),
        vec![
            vec![Value::Bool(true)],
            vec![Value::Bool(false)],
            vec![Value::Bool(true)],
            vec![Value::Bool(false)],
            vec![Value::Bool(false)],
            vec![Value::Bool(true)],
            vec![Value::Null],
        ]
    );
}

#[test]
fn should_preserve_copy_boolean_rejection_atomicity() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-copy-invalid-input",
        &[
            "CREATE TABLE bool_copy_rejected (id INT, flag BOOLEAN)",
            "INSERT INTO bool_copy_rejected VALUES (0, FALSE)",
        ],
    );
    let statement = copy_statement("bool_copy_rejected", "flag");

    // Act
    let copied = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"1,t\n2,o\n");

    // Assert
    assert!(matches!(copied, Err(CassieError::Parse(_))));
    assert_eq!(
        fixture.rows("SELECT id, flag FROM bool_copy_rejected ORDER BY id"),
        vec![vec![Value::Int64(0), Value::Bool(false)]]
    );
}

#[test]
fn should_copy_canonical_boolean_array_elements() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-copy-array-input",
        &["CREATE TABLE bool_array_inputs (id INT, flags BOOLEAN[])"],
    );
    let statement = copy_statement("bool_array_inputs", "flags");
    // CSV doubles quotes around the PostgreSQL array's quoted whitespace element.
    let payload = b"1,\"{tr,of,NULL,\"\"  Ye  \"\"}\"\n2,{}\n3,\n";

    // Act
    let copied = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, payload);

    // Assert
    assert_eq!(
        copied.expect("typed ARRAY elements use Boolean input vocabulary"),
        3
    );
    assert_eq!(
        fixture.rows("SELECT flags FROM bool_array_inputs ORDER BY id"),
        vec![
            vec![Value::Json(serde_json::json!([true, false, null, true]))],
            vec![Value::Json(serde_json::json!([]))],
            vec![Value::Null],
        ]
    );
}

#[test]
fn should_preserve_copy_boolean_array_rejection_atomicity() {
    // Arrange
    let fixture = metadata_fixture(
        "bool-copy-array-invalid",
        &["CREATE TABLE bool_array_rejected (id INT, flags BOOLEAN[])"],
    );
    let statement = copy_statement("bool_array_rejected", "flags");

    // Act
    let copied = fixture.cassie.copy_from_csv_stdin(
        &fixture.session,
        &statement,
        b"1,\"{t,f,NULL}\"\n2,{o}\n",
    );

    // Assert
    assert!(matches!(copied, Err(CassieError::Parse(_))));
    assert_eq!(
        fixture.rows("SELECT id FROM bool_array_rejected"),
        Vec::<Vec<Value>>::new()
    );
}
