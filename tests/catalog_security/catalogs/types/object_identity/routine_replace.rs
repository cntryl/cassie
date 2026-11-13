//! `CREATE OR REPLACE FUNCTION|PROCEDURE` conflict SQLSTATEs, pinned to a
//! PostgreSQL 18.6 oracle (42723, 42P13, 42809, 42601). Overloads are not
//! supported by Cassie, so a changed parameter list is 0A000 instead of a
//! second routine. Routine ownership is not recorded, so no owner check exists.

use crate::support_sql_fixture::sql_fixture;

const SETUP: &[&str] = &[
    "CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 1\"",
    "CREATE PROCEDURE routine_three(note TEXT) AS \"SELECT 1\"",
];

fn sqlstate_of(label: &str, sql: &str) -> String {
    let fixture = sql_fixture(label, SETUP);
    fixture
        .execute(sql)
        .expect_err("the statement is rejected")
        .sql_state()
        .to_string()
}

#[test]
fn should_reject_a_duplicate_function_with_sqlstate_42723() {
    // Arrange
    let sql = "CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 9\"";

    // Act
    let sqlstate = sqlstate_of("reject_a_duplicate_function_with_sqlstate_42723", sql);

    // Assert
    assert_eq!(sqlstate, "42723");
}

#[test]
fn should_reject_a_duplicate_procedure_with_sqlstate_42723() {
    // Arrange
    let sql = "CREATE PROCEDURE routine_three(note TEXT) AS \"SELECT 9\"";

    // Act
    let sqlstate = sqlstate_of("reject_a_duplicate_procedure_with_sqlstate_42723", sql);

    // Assert
    assert_eq!(sqlstate, "42723");
}

#[test]
fn should_reject_a_changed_return_type_with_sqlstate_42p13() {
    // Arrange
    let sql = "CREATE OR REPLACE FUNCTION routine_one(x INT) RETURNS TEXT AS \"'a'\"";

    // Act
    let sqlstate = sqlstate_of("reject_a_changed_return_type_with_sqlstate_42p13", sql);

    // Assert
    assert_eq!(sqlstate, "42P13");
}

#[test]
fn should_reject_a_changed_parameter_name_with_sqlstate_42p13() {
    // Arrange
    let sql = "CREATE OR REPLACE FUNCTION routine_one(y INT) RETURNS INT AS \"y + 1\"";

    // Act
    let sqlstate = sqlstate_of("reject_a_changed_parameter_name_with_sqlstate_42p13", sql);

    // Assert
    assert_eq!(sqlstate, "42P13");
}

#[test]
fn should_reject_a_changed_procedure_parameter_name_with_sqlstate_42p13() {
    // Arrange
    let sql = "CREATE OR REPLACE PROCEDURE routine_three(memo TEXT) AS \"SELECT 1\"";

    // Act
    let sqlstate = sqlstate_of("reject_a_changed_procedure_parameter_name_with_sqlsta", sql);

    // Assert
    assert_eq!(sqlstate, "42P13");
}

#[test]
fn should_reject_an_overload_instead_of_replacing_the_function() {
    // Arrange
    let sql = "CREATE OR REPLACE FUNCTION routine_one(x INT, z INT) RETURNS INT AS \"x + z\"";

    // Act
    let sqlstate = sqlstate_of("reject_an_overload_instead_of_replacing_the_function", sql);

    // Assert
    assert_eq!(sqlstate, "0A000");
}

#[test]
fn should_reject_an_overload_instead_of_replacing_the_procedure() {
    // Arrange
    let sql = "CREATE OR REPLACE PROCEDURE routine_three(note TEXT, more INT) AS \"SELECT 1\"";

    // Act
    let sqlstate = sqlstate_of("reject_an_overload_instead_of_replacing_the_procedure", sql);

    // Assert
    assert_eq!(sqlstate, "0A000");
}

#[test]
fn should_reject_replacing_a_procedure_with_a_function_with_sqlstate_42809() {
    // Arrange
    let sql = "CREATE OR REPLACE FUNCTION routine_three(note TEXT) RETURNS INT AS \"1\"";

    // Act
    let sqlstate = sqlstate_of("reject_replacing_a_procedure_with_a_function_with_sql", sql);

    // Assert
    assert_eq!(sqlstate, "42809");
}

#[test]
fn should_reject_replacing_a_function_with_a_procedure_with_sqlstate_42809() {
    // Arrange
    let sql = "CREATE OR REPLACE PROCEDURE routine_one(x INT) AS \"SELECT 1\"";

    // Act
    let sqlstate = sqlstate_of("reject_replacing_a_function_with_a_procedure_with_sql", sql);

    // Assert
    assert_eq!(sqlstate, "42809");
}

#[test]
fn should_reject_or_replace_with_if_not_exists_for_a_function_with_sqlstate_42601() {
    // Arrange
    let sql = "CREATE OR REPLACE FUNCTION IF NOT EXISTS routine_one(x INT) RETURNS INT AS \"x\"";

    // Act
    let sqlstate = sqlstate_of("reject_or_replace_with_if_not_exists_for_a_function_w", sql);

    // Assert
    assert_eq!(sqlstate, "42601");
}

#[test]
fn should_reject_or_replace_with_if_not_exists_for_a_procedure_with_sqlstate_42601() {
    // Arrange
    let sql = "CREATE OR REPLACE PROCEDURE IF NOT EXISTS routine_three(note TEXT) AS \"SELECT 1\"";

    // Act
    let sqlstate = sqlstate_of("reject_or_replace_with_if_not_exists_for_a_procedure_", sql);

    // Assert
    assert_eq!(sqlstate, "42601");
}

#[test]
fn should_replace_a_function_with_the_same_signature_and_keep_its_definition_callable() {
    // Arrange
    let fixture = sql_fixture("replace_same_signature", SETUP);

    // Act
    fixture
        .execute("CREATE OR REPLACE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 100\"")
        .expect("replace with the same signature");
    let rows = fixture.rows("SELECT routine_one(1)");

    // Assert
    assert_eq!(rows, vec![vec![cassie::types::Value::Int64(101)]]);
}
