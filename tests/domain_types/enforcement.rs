//! Domain constraint, NOT NULL and DEFAULT enforcement on casts, INSERT, UPDATE and COPY paths.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
            "CREATE DOMAIN req AS text NOT NULL",
            "CREATE DOMAIN dflt AS int DEFAULT 7 CHECK (VALUE < 100)",
            "CREATE TABLE items (id int, qty posint, label req DEFAULT 'x', n dflt)",
        ],
    )
}

#[test]
fn should_reject_a_cast_violating_a_check_with_sqlstate_23514() {
    // Arrange
    let fixture = fixture("domain_cast_check");

    // Act
    let error = fixture.error("SELECT (0)::posint");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_name_the_domain_in_the_check_violation_message() {
    // Arrange
    let fixture = fixture("domain_check_message");

    // Act
    let message = fixture.error("SELECT (-1)::posint").to_string();

    // Assert
    assert!(
        message.contains("domain posint"),
        "message names the domain"
    );
}

#[test]
fn should_pass_a_null_cast_for_a_check_only_domain() {
    // Arrange
    let fixture = fixture("domain_cast_null");

    // Act
    let rows = fixture.rows("SELECT NULL::posint");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_reject_a_null_cast_for_a_not_null_domain_with_sqlstate_23502() {
    // Arrange
    let fixture = fixture("domain_cast_not_null");

    // Act
    let error = fixture.error("SELECT NULL::req");

    // Assert
    assert_eq!(error.sql_state(), "23502");
}

#[test]
fn should_reject_an_insert_violating_the_domain_check_with_sqlstate_23514() {
    // Arrange
    let fixture = fixture("domain_insert_check");

    // Act
    let error = fixture.error("INSERT INTO items (id, qty) VALUES (1, -5)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_an_explicit_null_for_a_not_null_domain_column_with_sqlstate_23502() {
    // Arrange
    let fixture = fixture("domain_insert_not_null");

    // Act
    let error = fixture.error("INSERT INTO items (id, label) VALUES (1, NULL)");

    // Assert
    assert_eq!(error.sql_state(), "23502");
}

#[test]
fn should_keep_the_row_of_a_valid_insert() {
    // Arrange
    let fixture = fixture("domain_insert_ok");

    // Act
    fixture
        .execute("INSERT INTO items (id, qty) VALUES (1, 4)")
        .unwrap();
    let rows = fixture.rows("SELECT id, qty FROM items");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(4)]]);
}

#[test]
fn should_apply_the_domain_default_when_the_column_has_none() {
    // Arrange
    let fixture = fixture("domain_default_applied");

    // Act
    fixture
        .execute("INSERT INTO items (id) VALUES (1)")
        .unwrap();
    let rows = fixture.rows("SELECT n FROM items");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_prefer_the_column_default_over_the_domain_default() {
    // Arrange
    let fixture = fixture("domain_default_column_wins");

    // Act
    fixture
        .execute("INSERT INTO items (id) VALUES (1)")
        .unwrap();
    let rows = fixture.rows("SELECT label FROM items");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("x".into())]]);
}

#[test]
fn should_reject_an_update_violating_the_domain_check_with_sqlstate_23514() {
    // Arrange
    let fixture = fixture("domain_update_check");
    fixture
        .execute("INSERT INTO items (id, qty) VALUES (1, 4)")
        .unwrap();

    // Act
    let error = fixture.error("UPDATE items SET qty = 0 WHERE id = 1");
    let rows = fixture.rows("SELECT qty FROM items");

    // Assert
    assert_eq!(error.sql_state(), "23514");
    assert_eq!(
        rows,
        vec![vec![Value::Int64(4)]],
        "failed update changed nothing"
    );
}

#[test]
fn should_reject_an_update_to_null_for_a_not_null_domain_column() {
    // Arrange
    let fixture = fixture("domain_update_not_null");
    fixture
        .execute("INSERT INTO items (id) VALUES (1)")
        .unwrap();

    // Act
    let error = fixture.error("UPDATE items SET label = NULL");

    // Assert
    assert_eq!(error.sql_state(), "23502");
}

#[test]
fn should_reject_a_multi_row_insert_atomically() {
    // Arrange
    let fixture = fixture("domain_insert_atomic");

    // Act
    let error = fixture.error("INSERT INTO items (id, qty) VALUES (1, 4), (2, -1)");
    let rows = fixture.rows("SELECT id FROM items");

    // Assert
    assert_eq!(error.sql_state(), "23514");
    assert!(rows.is_empty(), "no row of the failed statement persists");
}

#[test]
fn should_resolve_operators_through_the_base_type() {
    // Arrange
    let fixture = fixture("domain_flatten_operators");
    fixture
        .execute("INSERT INTO items (id, qty) VALUES (1, 4), (2, 9)")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT qty + 1, abs(qty) FROM items WHERE qty > 5");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(10), Value::Int64(9)]]);
}

#[test]
fn should_reject_a_violating_value_even_when_on_conflict_do_nothing_applies() {
    // Arrange
    let fixture = sql_fixture(
        "domain_on_conflict_do_nothing",
        &[
            "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
            "CREATE TABLE items (id int PRIMARY KEY, q posint)",
            "INSERT INTO items VALUES (1, 5)",
        ],
    );

    // Act
    let error = fixture.error("INSERT INTO items VALUES (1, -1) ON CONFLICT DO NOTHING");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_an_upsert_update_assigning_a_violating_excluded_value() {
    // Arrange
    let fixture = sql_fixture(
        "domain_upsert_excluded",
        &[
            "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
            "CREATE TABLE items (id int PRIMARY KEY, q posint)",
            "INSERT INTO items VALUES (1, 5)",
        ],
    );

    // Act
    let error = fixture
        .error("INSERT INTO items VALUES (1, 0) ON CONFLICT (id) DO UPDATE SET q = EXCLUDED.q");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_apply_the_domain_default_for_set_column_to_default() {
    // Arrange
    let fixture = fixture("domain_set_default_keyword");
    fixture
        .execute("INSERT INTO items (id, n) VALUES (1, 50)")
        .unwrap();

    // Act
    fixture
        .execute("UPDATE items SET n = DEFAULT WHERE id = 1")
        .unwrap();
    let rows = fixture.rows("SELECT n FROM items");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(7)]]);
}

fn copy_statement() -> cassie::sql::ast::CopyStatement {
    cassie::sql::ast::CopyStatement {
        table: "items".to_string(),
        columns: vec!["id".to_string(), "qty".to_string()],
        format: cassie::sql::ast::CopyFormat::Csv,
        header: false,
    }
}

#[test]
fn should_reject_a_copy_row_violating_the_domain_check_with_sqlstate_23514() {
    // Arrange
    let fixture = fixture("domain_copy_check");

    // Act
    let result =
        fixture
            .cassie
            .copy_from_csv_stdin(&fixture.session, &copy_statement(), b"1,4\n2,-3\n");

    // Assert
    assert_eq!(result.expect_err("copy must fail").sql_state(), "23514");
    assert!(
        fixture.rows("SELECT id FROM items").is_empty(),
        "copy is atomic"
    );
}

#[test]
fn should_reject_a_copy_row_with_a_null_for_a_not_null_domain_with_sqlstate_23502() {
    // Arrange
    let fixture = fixture("domain_copy_not_null");
    let statement = cassie::sql::ast::CopyStatement {
        columns: vec!["id".to_string(), "label".to_string()],
        ..copy_statement()
    };

    // Act
    let result = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"1,\n");

    // Assert
    assert_eq!(result.expect_err("copy must fail").sql_state(), "23502");
}

#[test]
fn should_copy_valid_rows_into_a_domain_column() {
    // Arrange
    let fixture = fixture("domain_copy_ok");

    // Act
    let copied =
        fixture
            .cassie
            .copy_from_csv_stdin(&fixture.session, &copy_statement(), b"1,4\n2,9\n");
    let rows = fixture.rows("SELECT qty FROM items ORDER BY id");

    // Assert
    assert!(copied.is_ok(), "valid rows copy");
    assert_eq!(rows, vec![vec![Value::Int64(4)], vec![Value::Int64(9)]]);
}

#[test]
fn should_name_the_generated_constraint_in_the_check_violation_message() {
    // Arrange
    let fixture = fixture("domain_check_constraint_name");

    // Act
    let message = fixture.error("SELECT (-1)::posint").to_string();

    // Assert
    assert!(message.contains("posint_check"), "names the constraint");
}

#[test]
fn should_reject_a_violating_proposed_row_even_when_do_update_does_not_assign_it() {
    // Arrange
    let fixture = sql_fixture(
        "domain_on_conflict_do_update_proposed",
        &[
            "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
            "CREATE TABLE items (id int PRIMARY KEY, q posint)",
            "INSERT INTO items VALUES (1, 5)",
        ],
    );

    // Act
    let error =
        fixture.error("INSERT INTO items VALUES (1, -9) ON CONFLICT (id) DO UPDATE SET id = 1");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}
