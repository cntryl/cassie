//! PostgreSQL 18 rewrite-rule behavior contracts.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

#[test]
fn should_apply_a_qualified_insert_rule_using_new_row_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-913-qualified-insert-rule",
        &[
            "CREATE TABLE issue_913_rule_source (id INT)",
            "CREATE TABLE issue_913_rule_log (id INT)",
        ],
    );

    // Act
    let rule = fixture.execute(
        "CREATE RULE issue_913_log_positive_insert AS ON INSERT TO issue_913_rule_source WHERE (NEW.id > 0) DO ALSO INSERT INTO issue_913_rule_log VALUES (NEW.id)",
    );
    let positive_insert = fixture.execute("INSERT INTO issue_913_rule_source VALUES (7)");
    let negative_insert = fixture.execute("INSERT INTO issue_913_rule_source VALUES (-1)");
    let source_rows = fixture.execute("SELECT id FROM issue_913_rule_source ORDER BY id");
    let logged_rows = fixture.execute("SELECT id FROM issue_913_rule_log ORDER BY id");

    // Assert
    rule.expect("create qualified DO ALSO INSERT rule");
    positive_insert.expect("insert a row that matches the rule qualification");
    negative_insert.expect("insert a row that does not match the rule qualification");
    assert_eq!(
        source_rows
            .expect("read source rows after rule rewrites")
            .rows(),
        &[vec![Value::Int64(-1)], vec![Value::Int64(7)]]
    );
    assert_eq!(
        logged_rows
            .expect("read rows inserted by the rule action")
            .rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_replace_an_update_with_a_rule_using_old_new_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-913-instead-update-rule",
        &[
            "CREATE TABLE issue_913_update_source (id INT PRIMARY KEY, value INT)",
            "CREATE TABLE issue_913_update_log (id INT, value INT)",
            "INSERT INTO issue_913_update_source VALUES (1, 10)",
        ],
    );

    // Act
    let rule = fixture.execute(
        "CREATE RULE issue_913_replace_update AS ON UPDATE TO issue_913_update_source \
         DO INSTEAD INSERT INTO issue_913_update_log VALUES (OLD.id, NEW.value)",
    );
    let update = fixture.execute("UPDATE issue_913_update_source SET value = 20 WHERE id = 1");
    let source_rows = fixture.execute("SELECT id, value FROM issue_913_update_source ORDER BY id");
    let logged_rows = fixture.execute("SELECT id, value FROM issue_913_update_log ORDER BY id");

    // Assert
    rule.expect("create an ON UPDATE DO INSTEAD rule");
    update.expect("rewrite the UPDATE into the rule action");
    assert_eq!(
        source_rows
            .expect("read the source relation after rewrite")
            .rows(),
        &[vec![Value::Int64(1), Value::Int64(10)]]
    );
    assert_eq!(
        logged_rows
            .expect("read OLD and NEW values from the rule action")
            .rows(),
        &[vec![Value::Int64(1), Value::Int64(20)]]
    );
}

#[test]
fn should_log_old_rows_for_delete_rule_with_returning() {
    // Arrange
    let fixture = sql_fixture(
        "issue-913-delete-rule-old-returning",
        &[
            "CREATE TABLE issue_913_delete_source (id INT PRIMARY KEY, value TEXT)",
            "CREATE TABLE issue_913_delete_log (id INT, value TEXT)",
            "INSERT INTO issue_913_delete_source VALUES (1, 'removed'), (2, 'kept')",
        ],
    );

    // Act
    let rule = fixture.execute(
        "CREATE RULE issue_913_log_deleted_row AS ON DELETE TO issue_913_delete_source \
         DO ALSO INSERT INTO issue_913_delete_log VALUES (OLD.id, OLD.value)",
    );
    let deleted =
        fixture.execute("DELETE FROM issue_913_delete_source WHERE id = 1 RETURNING id, value");
    let source_rows = fixture.execute("SELECT id, value FROM issue_913_delete_source ORDER BY id");
    let logged_rows = fixture.execute("SELECT id, value FROM issue_913_delete_log ORDER BY id");

    // Assert
    rule.expect("create a DELETE DO ALSO rule using OLD values");
    assert_eq!(
        deleted
            .expect("DELETE RETURNING reports the deleted source row")
            .rows(),
        &[vec![Value::Int64(1), Value::String("removed".to_owned())]]
    );
    assert_eq!(
        source_rows
            .expect("read the source after deleting one row")
            .rows(),
        &[vec![Value::Int64(2), Value::String("kept".to_owned())]]
    );
    assert_eq!(
        logged_rows
            .expect("read the row emitted by the DELETE rule")
            .rows(),
        &[vec![Value::Int64(1), Value::String("removed".to_owned())]]
    );
}
