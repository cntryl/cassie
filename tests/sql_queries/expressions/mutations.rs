//! PostgreSQL 18 mutation forms tracked by #796, #823, and #825.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[path = "../inheritance/truncate_only_inheritance.rs"]
mod truncate_only_inheritance;

#[test]
fn should_assign_multiple_update_columns_from_a_scalar_subquery() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-tuple-subquery",
        &[
            "CREATE TABLE update_tuple_target (id INT PRIMARY KEY, label TEXT, version INT)",
            "INSERT INTO update_tuple_target VALUES (1, 'old', 1), (2, 'keep', 2)",
        ],
    );

    // Act
    let changed = fixture.execute(
        "UPDATE update_tuple_target AS target SET (label, version) = (SELECT 'next', 9) WHERE id = 1 RETURNING target.id, target.label, target.version",
    );
    let stored = fixture.execute("SELECT id, label, version FROM update_tuple_target ORDER BY id");

    // Assert
    assert_eq!(
        changed
            .expect("tuple assignment from a scalar subquery")
            .rows(),
        &[vec![
            Value::Int64(1),
            Value::String("next".to_owned()),
            Value::Int64(9),
        ]]
    );
    assert_eq!(
        stored.expect("updated and untouched rows").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("next".to_owned()),
                Value::Int64(9),
            ],
            vec![
                Value::Int64(2),
                Value::String("keep".to_owned()),
                Value::Int64(2),
            ],
        ]
    );
}

#[test]
fn should_insert_default_values_using_all_declared_defaults() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-insert-default-values",
        &[
            "CREATE TABLE default_insert_target (label TEXT DEFAULT 'pending', version INT DEFAULT 7)",
        ],
    );

    // Act
    let inserted = fixture
        .execute("INSERT INTO default_insert_target DEFAULT VALUES RETURNING label, version");

    // Assert
    assert_eq!(
        inserted.expect("INSERT DEFAULT VALUES").rows(),
        &[vec![Value::String("pending".to_owned()), Value::Int64(7)]]
    );
}

#[test]
fn should_apply_default_keywords_in_insert_values_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-insert-default-values-list",
        &[
            "CREATE TABLE default_values_target (label TEXT DEFAULT 'pending', version INT DEFAULT 7)",
        ],
    );

    // Act
    let inserted = fixture.execute(
        "INSERT INTO default_values_target VALUES (DEFAULT, 9), ('custom', DEFAULT) RETURNING label, version",
    );

    // Assert
    assert_eq!(
        inserted
            .expect("DEFAULT values in an INSERT VALUES list")
            .rows(),
        &[
            vec![Value::String("pending".to_owned()), Value::Int64(9)],
            vec![Value::String("custom".to_owned()), Value::Int64(7)],
        ]
    );
}

#[test]
fn should_apply_default_to_an_update_assignment() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-default-assignment",
        &[
            "CREATE TABLE default_update_target (id INT PRIMARY KEY, label TEXT DEFAULT 'pending')",
            "INSERT INTO default_update_target VALUES (1, 'old')",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE default_update_target SET label = DEFAULT WHERE id = 1 RETURNING id, label",
    );

    // Assert
    assert_eq!(
        updated.expect("DEFAULT in an UPDATE assignment").rows(),
        &[vec![Value::Int64(1), Value::String("pending".to_owned())]]
    );
}

#[test]
fn should_return_update_row_images() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-old-new-returning",
        &[
            "CREATE TABLE update_old_new_target (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO update_old_new_target VALUES (1, 'old')",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE update_old_new_target SET label = 'new' WHERE id = 1 RETURNING WITH (OLD AS old_row, NEW AS new_row) old_row.id, old_row.label, new_row.id, new_row.label",
    );

    // Assert
    assert_eq!(
        updated.expect("UPDATE RETURNING OLD and NEW rows").rows(),
        &[vec![
            Value::Int64(1),
            Value::String("old".to_owned()),
            Value::Int64(1),
            Value::String("new".to_owned()),
        ]]
    );
}

#[test]
fn should_return_delete_row_images() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-delete-old-new-returning",
        &[
            "CREATE TABLE delete_old_new_target (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO delete_old_new_target VALUES (1, 'old')",
        ],
    );

    // Act
    let deleted = fixture.execute(
        "DELETE FROM delete_old_new_target WHERE id = 1 RETURNING WITH (OLD AS old_row, NEW AS new_row) old_row.id, old_row.label, new_row.id, new_row.label",
    );

    // Assert
    assert_eq!(
        deleted.expect("DELETE RETURNING OLD and NEW rows").rows(),
        &[vec![
            Value::Int64(1),
            Value::String("old".to_owned()),
            Value::Null,
            Value::Null,
        ]]
    );
}

#[test]
fn should_evaluate_a_case_guarded_returning_expression_per_updated_row() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-update-returning-case-arithmetic",
        &[
            "CREATE TABLE issue_796_returning_case (id BIGINT)",
            "INSERT INTO issue_796_returning_case VALUES (1), (2)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE issue_796_returning_case SET id = id RETURNING id, CASE WHEN id < 0 THEN id / (id - id) ELSE id END AS selected",
    );

    // Assert
    assert_eq!(
        updated
            .expect("guarded division stays unevaluated for positive rows")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_describe_an_empty_bigint_update_returning_expression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-empty-update-returning-descriptor",
        &[
            "CREATE TABLE issue_796_empty_returning (id BIGINT)",
            "INSERT INTO issue_796_empty_returning VALUES (1), (2)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE issue_796_empty_returning SET id = id + 10 WHERE id < 0 RETURNING id / (id - id) AS selected",
    );

    // Assert
    let updated = updated.expect("empty UPDATE still describes its RETURNING projection");
    assert_eq!(updated.rows(), &[] as &[Vec<Value>]);
    assert_eq!(updated.columns()[0].type_oid, 20);
}

#[test]
fn should_assign_conflicting_columns_from_a_row_constructor() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-conflict-row-assignment",
        &[
            "CREATE TABLE conflict_row_target (id INT PRIMARY KEY, label TEXT, version INT)",
            "INSERT INTO conflict_row_target VALUES (1, 'old', 1)",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO conflict_row_target VALUES (1, 'new', 9) ON CONFLICT (id) DO UPDATE SET (label, version) = ROW(EXCLUDED.label, EXCLUDED.version) RETURNING id, label, version",
    );

    // Assert
    assert_eq!(
        changed
            .expect("ON CONFLICT row-constructor assignment")
            .rows(),
        &[vec![
            Value::Int64(1),
            Value::String("new".to_owned()),
            Value::Int64(9),
        ]]
    );
}

#[test]
fn should_apply_insert_defaults_to_excluded_conflict_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-excluded-insert-default",
        &[
            "CREATE TABLE excluded_default_target (id INT PRIMARY KEY, label TEXT DEFAULT 'pending')",
            "INSERT INTO excluded_default_target (id, label) VALUES (1, 'old')",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO excluded_default_target (id) VALUES (1) \
         ON CONFLICT (id) DO UPDATE SET label = EXCLUDED.label \
         RETURNING id, label",
    );
    let stored = fixture.execute("SELECT id, label FROM excluded_default_target WHERE id = 1");

    // Assert
    assert_eq!(
        changed
            .expect("EXCLUDED carries the insert's column default")
            .rows(),
        &[vec![Value::Int64(1), Value::String("pending".to_owned())]]
    );
    assert_eq!(
        stored.expect("read updated conflict target").rows(),
        &[vec![Value::Int64(1), Value::String("pending".to_owned())]]
    );
}

#[test]
fn should_match_an_explicit_operator_class_in_a_conflict_target() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-conflict-target-opclass",
        &[
            "CREATE TABLE conflict_opclass_target (email TEXT UNIQUE, note TEXT)",
            "INSERT INTO conflict_opclass_target VALUES ('a', 'old')",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO conflict_opclass_target VALUES ('a', 'new') ON CONFLICT (email pg_catalog.text_ops) DO UPDATE SET note = EXCLUDED.note RETURNING email, note",
    );

    // Assert
    assert_eq!(
        changed
            .expect("explicit operator-class conflict target")
            .rows(),
        &[vec![
            Value::String("a".to_owned()),
            Value::String("new".to_owned()),
        ]]
    );
}

#[test]
fn should_update_each_target_once_when_from_matches_repeat() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-from-repeated-source-matches",
        &[
            "CREATE TABLE issue_823_update_target (id INT PRIMARY KEY, amount INT)",
            "CREATE TABLE issue_823_update_source (target_id INT, increment INT)",
            "INSERT INTO issue_823_update_target VALUES (1,10),(2,20),(3,30)",
            "INSERT INTO issue_823_update_source VALUES (1,2),(1,2),(2,3)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE issue_823_update_target AS target SET amount = target.amount + source.increment FROM issue_823_update_source AS source WHERE target.id = source.target_id RETURNING target.id, target.amount, source.increment",
    );
    let stored = fixture.execute("SELECT id, amount FROM issue_823_update_target ORDER BY id");

    // Assert
    let updated = updated.expect("UPDATE FROM updates each matched target once");
    let mut returned = updated.rows().to_vec();
    returned.sort_by_key(|row| match &row[0] {
        Value::Int64(id) => *id,
        value => panic!("expected integer target id, got {value:?}"),
    });
    assert_eq!(
        returned,
        &[
            vec![Value::Int64(1), Value::Int64(12), Value::Int64(2)],
            vec![Value::Int64(2), Value::Int64(23), Value::Int64(3)],
        ]
    );
    assert_eq!(
        updated
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
    assert_eq!(
        stored
            .expect("read all target rows after UPDATE FROM")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(12)],
            vec![Value::Int64(2), Value::Int64(23)],
            vec![Value::Int64(3), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_delete_each_target_once_when_using_matches_repeat() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-delete-using-repeated-source-matches",
        &[
            "CREATE TABLE issue_823_delete_target (id INT PRIMARY KEY, label TEXT)",
            "CREATE TABLE issue_823_delete_source (target_id INT)",
            "INSERT INTO issue_823_delete_target VALUES (1,'keep'),(2,'delete')",
            "INSERT INTO issue_823_delete_source VALUES (2),(2)",
        ],
    );

    // Act
    let deleted = fixture.execute(
        "DELETE FROM issue_823_delete_target AS target USING issue_823_delete_source AS source WHERE target.id = source.target_id RETURNING target.id",
    );
    let remaining = fixture.execute("SELECT id, label FROM issue_823_delete_target ORDER BY id");

    // Assert
    assert_eq!(
        deleted
            .expect("DELETE USING deletes a matched target once")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
    assert_eq!(
        remaining.expect("read rows after DELETE USING").rows(),
        &[vec![Value::Int64(1), Value::String("keep".to_owned()),]]
    );
}

#[test]
fn should_project_using_source_columns_in_delete_returning() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-delete-using-source-returning",
        &[
            "CREATE TABLE issue_823_delete_returning_target (id INT PRIMARY KEY, label TEXT)",
            "CREATE TABLE issue_823_delete_returning_source (target_id INT, reason TEXT)",
            "INSERT INTO issue_823_delete_returning_target VALUES (1,'keep'),(2,'delete')",
            "INSERT INTO issue_823_delete_returning_source VALUES (2,'expired')",
        ],
    );

    // Act
    let deleted = fixture.execute(
        "DELETE FROM issue_823_delete_returning_target AS target USING issue_823_delete_returning_source AS source WHERE target.id = source.target_id RETURNING target.id, source.reason",
    );
    let remaining = fixture.execute("SELECT id FROM issue_823_delete_returning_target ORDER BY id");

    // Assert
    let deleted = deleted.expect("DELETE RETURNING can reference a USING source");
    assert_eq!(
        deleted.rows(),
        &[vec![Value::Int64(2), Value::String("expired".to_owned()),]]
    );
    assert_eq!(
        deleted
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25]
    );
    assert_eq!(
        remaining.expect("read remaining target ids").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_select_merge_actions_by_match_conditions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-merge-conditional-action-matrix",
        &[
            "CREATE TABLE issue_823_merge_target (id INT PRIMARY KEY, label TEXT)",
            "CREATE TABLE issue_823_merge_source (id INT, label TEXT)",
            "INSERT INTO issue_823_merge_target VALUES (1,'keep'),(2,'update'),(3,'remove'),(4,'source absent')",
            "INSERT INTO issue_823_merge_source VALUES (1,'ignored'),(2,'updated'),(3,'delete'),(5,'inserted'),(6,'ignored')",
        ],
    );

    // Act
    let merged = fixture.execute(
        "MERGE INTO issue_823_merge_target AS target USING issue_823_merge_source AS source ON target.id = source.id WHEN MATCHED AND source.label = 'delete' THEN DELETE WHEN MATCHED AND source.label = 'updated' THEN UPDATE SET label = source.label WHEN MATCHED THEN DO NOTHING WHEN NOT MATCHED BY TARGET AND source.label = 'inserted' THEN INSERT (id,label) VALUES (source.id,source.label) WHEN NOT MATCHED BY TARGET THEN DO NOTHING RETURNING merge_action(), target.id",
    );
    let stored = fixture.execute("SELECT id, label FROM issue_823_merge_target ORDER BY id");

    // Assert
    let merged = merged.expect("MERGE applies the first matching conditional action");
    let mut actions = merged.rows().to_vec();
    actions.sort_by_key(|row| match &row[1] {
        Value::Int64(id) => *id,
        value => panic!("expected integer target id, got {value:?}"),
    });
    assert_eq!(
        actions,
        &[
            vec![Value::String("UPDATE".to_owned()), Value::Int64(2)],
            vec![Value::String("DELETE".to_owned()), Value::Int64(3)],
            vec![Value::String("INSERT".to_owned()), Value::Int64(5)],
        ]
    );
    assert_eq!(
        merged
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23]
    );
    assert_eq!(
        stored.expect("read MERGE target rows").rows(),
        &[
            vec![Value::Int64(1), Value::String("keep".to_owned())],
            vec![Value::Int64(2), Value::String("updated".to_owned())],
            vec![Value::Int64(4), Value::String("source absent".to_owned())],
            vec![Value::Int64(5), Value::String("inserted".to_owned())],
        ]
    );
}

#[test]
fn should_apply_conditional_merge_actions_to_rows_missing_from_the_source() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-merge-by-source-conditional-action",
        &[
            "CREATE TABLE issue_823_merge_by_source_target (id INT PRIMARY KEY, label TEXT)",
            "CREATE TABLE issue_823_merge_by_source_source (id INT)",
            "INSERT INTO issue_823_merge_by_source_target VALUES (1,'matched'),(2,'expire'),(3,'keep')",
            "INSERT INTO issue_823_merge_by_source_source VALUES (1)",
        ],
    );

    // Act
    let merged = fixture.execute(
        "MERGE INTO issue_823_merge_by_source_target AS target \
         USING issue_823_merge_by_source_source AS source ON target.id = source.id \
         WHEN NOT MATCHED BY SOURCE AND target.label = 'expire' \
           THEN UPDATE SET label = 'expired' \
         WHEN NOT MATCHED BY SOURCE THEN DO NOTHING \
         RETURNING merge_action(), target.id, target.label",
    );
    let stored =
        fixture.execute("SELECT id, label FROM issue_823_merge_by_source_target ORDER BY id");

    // Assert
    let merged = merged.expect("apply the conditional action only to the selected target-only row");
    assert_eq!(
        merged.rows(),
        &[vec![
            Value::String("UPDATE".to_owned()),
            Value::Int64(2),
            Value::String("expired".to_owned()),
        ]]
    );
    assert_eq!(
        merged
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 25]
    );
    assert_eq!(
        stored.expect("read all MERGE target rows").rows(),
        &[
            vec![Value::Int64(1), Value::String("matched".to_owned())],
            vec![Value::Int64(2), Value::String("expired".to_owned())],
            vec![Value::Int64(3), Value::String("keep".to_owned())],
        ]
    );
}

#[test]
fn should_return_action_specific_merge_row_images() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-merge-old-new-returning",
        &[
            "CREATE TABLE merge_old_new_target (id INT PRIMARY KEY, label TEXT)",
            "CREATE TABLE merge_old_new_source (id INT, action TEXT, label TEXT)",
            "INSERT INTO merge_old_new_target VALUES (1, 'before update'), (2, 'before delete')",
            "INSERT INTO merge_old_new_source VALUES \
                (1, 'UPDATE', 'after update'), \
                (2, 'DELETE', 'unused'), \
                (3, 'INSERT', 'inserted')",
        ],
    );

    // Act
    let merged = fixture.execute(
        "MERGE INTO merge_old_new_target AS target \
         USING merge_old_new_source AS source ON target.id = source.id \
         WHEN MATCHED AND source.action = 'UPDATE' THEN UPDATE SET label = source.label \
         WHEN MATCHED AND source.action = 'DELETE' THEN DELETE \
         WHEN NOT MATCHED BY TARGET AND source.action = 'INSERT' \
           THEN INSERT (id, label) VALUES (source.id, source.label) \
         RETURNING WITH (OLD AS old_row, NEW AS new_row) \
           merge_action() AS action, \
           old_row.id AS old_id, old_row.label AS old_label, \
           new_row.id AS new_id, new_row.label AS new_label",
    );

    // Assert
    let merged = merged.expect("MERGE should return OLD and NEW for each action");
    let mut images = merged.rows().to_vec();
    images.sort_by_key(|row| match &row[0] {
        Value::String(action) => action.clone(),
        value => panic!("expected MERGE action text, got {value:?}"),
    });
    assert_eq!(
        images,
        &[
            vec![
                Value::String("DELETE".to_owned()),
                Value::Int64(2),
                Value::String("before delete".to_owned()),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::String("INSERT".to_owned()),
                Value::Null,
                Value::Null,
                Value::Int64(3),
                Value::String("inserted".to_owned()),
            ],
            vec![
                Value::String("UPDATE".to_owned()),
                Value::Int64(1),
                Value::String("before update".to_owned()),
                Value::Int64(1),
                Value::String("after update".to_owned()),
            ],
        ]
    );
    assert_eq!(
        merged
            .columns()
            .iter()
            .map(|column| (
                column.name.as_str(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
                column.format_code,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("action", 25, -1, -1, 0),
            ("old_id", 23, 4, -1, 0),
            ("old_label", 25, -1, -1, 0),
            ("new_id", 23, 4, -1, 0),
            ("new_label", 25, -1, -1, 0),
        ]
    );
}

#[test]
fn should_truncate_multiple_tables_in_one_statement() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-truncate-multiple-tables",
        &[
            "CREATE TABLE issue_823_truncate_first (id INT)",
            "CREATE TABLE issue_823_truncate_second (id INT)",
            "INSERT INTO issue_823_truncate_first VALUES (1),(2)",
            "INSERT INTO issue_823_truncate_second VALUES (3)",
        ],
    );

    // Act
    let truncated =
        fixture.execute("TRUNCATE TABLE issue_823_truncate_first, issue_823_truncate_second");
    let first_count = fixture.execute("SELECT count(*) FROM issue_823_truncate_first");
    let second_count = fixture.execute("SELECT count(*) FROM issue_823_truncate_second");

    // Assert
    truncated.expect("truncate both relations in one statement");
    assert_eq!(
        first_count.expect("count first truncated relation").rows(),
        &[vec![Value::Int64(0)]]
    );
    assert_eq!(
        second_count
            .expect("count second truncated relation")
            .rows(),
        &[vec![Value::Int64(0)]]
    );
}
