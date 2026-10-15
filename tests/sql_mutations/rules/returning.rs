//! Multi-action INSERT rules retain the source row in RETURNING (#913).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_the_source_row_given_multiple_do_also_actions_when_insert_returning_executes() {
    // Arrange
    let fixture = sql_fixture(
        "insert-rule-multiple-actions-returning",
        &[
            "CREATE TABLE rule_returning_source (id INT4)",
            "CREATE TABLE rule_returning_log (sequence BIGINT GENERATED ALWAYS AS IDENTITY, action TEXT, source_id INT4)",
            "CREATE TABLE rule_returning_control (id INT4)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let insert_control =
        fixture.execute("INSERT INTO rule_returning_control VALUES (8) RETURNING id");
    let rule_b = fixture.execute(
        "CREATE RULE action_b AS ON INSERT TO rule_returning_source \
         DO ALSO INSERT INTO rule_returning_log (action, source_id) \
         VALUES ('b', NEW.id)",
    );
    let rule_a = fixture.execute(
        "CREATE RULE action_a AS ON INSERT TO rule_returning_source \
         DO ALSO ( \
             INSERT INTO rule_returning_log (action, source_id) VALUES ('a1', NEW.id); \
             INSERT INTO rule_returning_log (action, source_id) VALUES ('a2', NEW.id) \
         )",
    );
    let inserted = fixture.execute("INSERT INTO rule_returning_source VALUES (9) RETURNING id");
    let source_rows = fixture.execute("SELECT id FROM rule_returning_source ORDER BY id");
    let actions =
        fixture.execute("SELECT action, source_id FROM rule_returning_log ORDER BY sequence");

    // Act
    let insert_control = insert_control.expect("ordinary INSERT RETURNING control");
    let rule_b = rule_b.expect("create second alphabetical DO ALSO rule");
    let rule_a = rule_a.expect("create multi-action DO ALSO rule");
    let inserted = inserted.expect("INSERT RETURNING after both rewrite rules");
    let source_rows = source_rows.expect("read inserted source relation");
    let actions = actions.expect("read all rule action rows in sequence order");

    // Assert
    assert_eq!(insert_control.rows(), &[vec![Value::Int64(8)]]);
    assert_eq!(
        insert_control.columns()[0].type_oid,
        23,
        "ordinary INSERT RETURNING source identity"
    );
    assert_eq!(rule_b.command(), "CREATE RULE");
    assert_eq!(rule_a.command(), "CREATE RULE");
    assert_eq!(
        inserted.rows(),
        &[vec![Value::Int64(9)]],
        "DO ALSO actions do not replace or duplicate the source RETURNING row"
    );
    assert_eq!(inserted.columns()[0].type_oid, 23);
    assert_eq!(source_rows.rows(), &[vec![Value::Int64(9)]]);
    assert_eq!(
        actions.rows(),
        &[
            vec![Value::String("a1".to_owned()), Value::Int64(9)],
            vec![Value::String("a2".to_owned()), Value::Int64(9)],
            vec![Value::String("b".to_owned()), Value::Int64(9)],
        ]
    );
}
