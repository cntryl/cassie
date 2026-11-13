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

// Authored, not run: domains through table row types and JSON record sources
// (integration review F2 and F3).

fn row_type_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
            "CREATE DOMAIN req AS int NOT NULL",
            "CREATE TABLE dt (a posint)",
            "CREATE TABLE rt (a req)",
        ],
    )
}

#[test]
fn should_reject_a_text_literal_cast_to_a_row_type_violating_a_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_row_type_text_literal");

    // Act
    let error = fixture.error("SELECT '(-1)'::dt");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_a_row_constructor_cast_to_a_row_type_violating_a_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_row_type_row_constructor");

    // Act
    let error = fixture.error("SELECT row(-1)::dt");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_populate_record_into_a_row_type_violating_a_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_row_type_populate_record");

    // Act
    let error = fixture.error("SELECT * FROM jsonb_populate_record(null::dt, '{\"a\":-1}')");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_accept_a_row_type_cast_satisfying_the_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_row_type_valid_cast");

    // Act
    let rows = fixture.rows("SELECT ('(3)'::dt).a");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(3)]]);
}

#[test]
fn should_reject_a_row_constructor_null_cast_to_a_row_type_with_a_not_null_domain() {
    // Arrange
    let fixture = row_type_fixture("domain_row_type_not_null");

    // Act
    let error = fixture.error("SELECT row(NULL::int)::rt");

    // Assert
    assert_eq!(error.sql_state(), "23502");
}

#[test]
fn should_convert_a_domain_column_of_jsonb_to_record() {
    // Arrange
    let fixture = row_type_fixture("domain_jsonb_to_record_ok");

    // Act
    let rows = fixture.rows("SELECT * FROM jsonb_to_record('{\"a\":1}') AS t(a posint)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_a_jsonb_to_record_value_violating_the_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_jsonb_to_record_violation");

    // Act
    let error = fixture.error("SELECT * FROM jsonb_to_record('{\"a\":-1}') AS t(a posint)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_reject_a_json_to_recordset_value_violating_the_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_json_to_recordset_violation");

    // Act
    let error = fixture.error("SELECT * FROM json_to_recordset('[{\"a\":-1}]') AS t(a posint)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_yield_null_for_a_json_table_domain_violation_under_the_default_on_error() {
    // Arrange: PostgreSQL 18.6 turns the conversion error into NULL by default.
    let fixture = row_type_fixture("domain_json_table_default_on_error");

    // Act
    let rows = fixture
        .rows("SELECT * FROM JSON_TABLE('{\"a\":-1}'::jsonb, '$' COLUMNS (a posint PATH '$.a'))");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_reject_a_json_table_domain_violation_under_a_column_error_on_error() {
    // Arrange
    let fixture = row_type_fixture("domain_json_table_error_on_error");

    // Act
    let error = fixture.error(
        "SELECT * FROM JSON_TABLE('{\"a\":-1}'::jsonb, '$' COLUMNS (a posint PATH '$.a' ERROR ON ERROR))",
    );

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_return_a_json_table_value_satisfying_the_domain_check() {
    // Arrange
    let fixture = row_type_fixture("domain_json_table_valid");

    // Act
    let rows = fixture
        .rows("SELECT * FROM JSON_TABLE('{\"a\":3}'::jsonb, '$' COLUMNS (a posint PATH '$.a'))");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(3)]]);
}

// Authored, not run: revision 2 (whole-row reads, live descriptors, field inference).

#[test]
fn should_read_a_whole_row_that_violates_a_not_valid_domain_check() {
    // Arrange: PostgreSQL does not revalidate stored values on read.
    let fixture = row_type_fixture("domain_whole_row_not_valid");
    fixture.execute("INSERT INTO dt VALUES (5)").unwrap();
    fixture
        .execute("ALTER DOMAIN posint ADD CONSTRAINT small CHECK (VALUE < 3) NOT VALID")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT dt::text FROM dt");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("(5)".into())]]);
}

#[test]
fn should_stop_enforcing_a_dropped_domain_check_on_row_type_casts() {
    // Arrange
    let fixture = row_type_fixture("domain_row_cast_dropped_check");
    fixture
        .execute("ALTER DOMAIN posint DROP CONSTRAINT posint_check")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT ('(-1)'::dt)::text");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("(-1)".into())]]);
}

#[test]
fn should_enforce_an_added_domain_check_on_row_type_casts() {
    // Arrange
    let fixture = row_type_fixture("domain_row_cast_added_check");
    fixture
        .execute("ALTER DOMAIN posint ADD CONSTRAINT small CHECK (VALUE < 10)")
        .unwrap();

    // Act
    let error = fixture.error("SELECT '(50)'::dt");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_infer_the_base_type_for_arithmetic_on_a_row_type_domain_field() {
    // Arrange
    let fixture = row_type_fixture("domain_row_field_arithmetic");

    // Act
    let rows = fixture.rows("SELECT pg_typeof(('(3)'::dt).a + 1), ('(3)'::dt).a + 1");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("integer".into()), Value::Int64(4)]]
    );
}
