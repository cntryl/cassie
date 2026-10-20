use crate::support_read_equivalence::{sql, with_fixture, with_fixture_config};
use cassie::types::Value;

#[test]
fn should_resolve_unqualified_lateral_columns_in_the_inner_scope() {
    // Arrange
    with_fixture("lateral_shadowing", |cassie, session| {
        for statement in [
            "CREATE TABLE shadow_users (k INT, name TEXT)",
            "CREATE TABLE shadow_orders (k INT, total INT)",
            "INSERT INTO shadow_users VALUES (1,'ada'),(2,'grace')",
            "INSERT INTO shadow_orders VALUES (1,42),(2,7)",
        ] {
            sql(cassie, session, statement);
        }
        // Act
        let result=sql(cassie,session,"SELECT shadow_users.name,x.total FROM shadow_users JOIN LATERAL (SELECT total FROM shadow_orders WHERE k=shadow_users.k) AS x ON true ORDER BY shadow_users.name");
        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("ada".into()), Value::Int64(42)],
                vec![Value::String("grace".into()), Value::Int64(7)]
            ]
        );
        let wildcard = sql(cassie,session,"SELECT * FROM shadow_users JOIN LATERAL (SELECT * FROM shadow_orders WHERE k=shadow_users.k) AS x ON true ORDER BY shadow_users.name");
        assert_eq!(wildcard.columns().len(), 6);
        assert_eq!(wildcard.rows().len(), 2);
        for row in wildcard.rows() {
            assert_eq!(row.len(), 6, "{row:?}");
        }
        let single = sql(cassie,session,"SELECT shadow_users.name,x.outer_key FROM shadow_users JOIN LATERAL (SELECT shadow_users.k AS outer_key) AS x ON true ORDER BY shadow_users.name");
        assert_eq!(
            single.rows(),
            vec![
                vec![Value::String("ada".into()), Value::Int64(1)],
                vec![Value::String("grace".into()), Value::Int64(2)]
            ]
        );
    });
}

#[test]
fn should_emit_complete_null_extensions_for_empty_outer_join_sources() {
    // Arrange
    with_fixture("empty_join_shapes", |cassie, session| {
        for statement in [
            "CREATE TABLE shape_left (k INT, name TEXT)",
            "CREATE TABLE shape_right (k INT, total INT)",
            "INSERT INTO shape_left VALUES (1,'ada'),(2,'grace')",
        ] {
            sql(cassie, session, statement);
        }
        // Act
        for statement in [
            "SELECT * FROM shape_left LEFT JOIN shape_right ON shape_left.k=shape_right.k",
            "SELECT * FROM shape_right RIGHT JOIN shape_left ON shape_left.k=shape_right.k",
            "SELECT * FROM shape_left FULL JOIN shape_right ON shape_left.k=shape_right.k",
            "SELECT * FROM shape_left LEFT JOIN shape_right ON false",
        ] {
            let result = sql(cassie, session, statement);
            // Assert
            assert_eq!(result.columns().len(), 6, "{statement}");
            assert_eq!(result.rows().len(), 2, "{statement}");
            for row in result.rows() {
                assert_eq!(row.len(), result.columns().len(), "{statement}: {row:?}");
            }
        }
        for source in [
            "(SELECT k,total FROM shape_right) AS empty",
            "(SELECT k,total FROM shape_right WHERE false) AS empty",
        ] {
            let result=sql(cassie,session,&format!("SELECT shape_left.name,empty.k,empty.total FROM shape_left LEFT JOIN {source} ON shape_left.k=empty.k ORDER BY shape_left.name"));
            assert_eq!(
                result.rows(),
                vec![
                    vec![Value::String("ada".into()), Value::Null, Value::Null],
                    vec![Value::String("grace".into()), Value::Null, Value::Null]
                ]
            );
        }
        let result=sql(cassie,session,"WITH empty AS (SELECT k,total FROM shape_right) SELECT * FROM shape_left LEFT JOIN empty ON shape_left.k=empty.k");
        assert_eq!(result.columns().len(), 5);
        for row in result.rows() {
            assert_eq!(row.len(), 5, "{row:?}");
        }
        sql(
            cassie,
            session,
            "CREATE VIEW shape_view AS SELECT k,total FROM shape_right",
        );
        for statement in ["SELECT * FROM shape_left LEFT JOIN shape_view ON shape_left.k=shape_view.k", "WITH seed AS (SELECT k,total FROM shape_right), renamed(x,y) AS (SELECT k,total FROM seed) SELECT * FROM shape_left LEFT JOIN renamed ON shape_left.k=renamed.x"] {
                let result=sql(cassie,session,statement);
                assert_eq!(result.columns().len(),5,"{statement}");
                assert_eq!(result.rows().len(),2,"{statement}");
                for row in result.rows() { assert_eq!(row.len(),5,"{statement}: {row:?}"); assert_eq!(&row[3..],&[Value::Null,Value::Null]); }
            }
    });
}

#[test]
fn should_preserve_exact_output_for_empty_right_cte_on_left_join() {
    // Arrange
    for vectorized in [false, true] {
        with_fixture_config(
            "ordered_empty_outer_join",
            |config| config.limits.vectorized_joins_enabled = vectorized,
            |cassie, session| {
                seed_ordered_outer_join_tables(cassie, session);

                // Act
                let before_left_join = cassie.metrics();
                let empty_right = sql(
                    cassie,
                    session,
                    "WITH empty_right(right_key, right_label) AS (SELECT right_key, right_label FROM ordered_right WHERE false) SELECT ordered_left.left_key AS left_key_alias, ordered_left.left_label AS left_label_alias, empty_right.right_key AS right_key_alias, empty_right.right_label AS right_label_alias FROM ordered_left LEFT JOIN empty_right ON ordered_left.left_key = empty_right.right_key ORDER BY ordered_left.left_key",
                );
                // Assert
                let after_left_join = cassie.metrics();
                let vectorized_before = before_left_join["joins"]["vectorized_joins"]
                    .as_u64()
                    .unwrap_or_default();
                let vectorized_after = after_left_join["joins"]["vectorized_joins"]
                    .as_u64()
                    .unwrap_or_default();
                assert_eq!(
                    vectorized_after.saturating_sub(vectorized_before),
                    u64::from(vectorized),
                    "vectorized join selection: {vectorized}"
                );
                if !vectorized {
                    assert_eq!(after_left_join["joins"]["last_strategy"], "merge");
                }
                assert_ordered_outer_join_result(
                    &empty_right,
                    &[
                        vec![
                            Value::Int64(1),
                            Value::String("left-one".into()),
                            Value::Null,
                            Value::Null,
                        ],
                        vec![
                            Value::Int64(2),
                            Value::String("left-two".into()),
                            Value::Null,
                            Value::Null,
                        ],
                    ],
                );
            },
        );
    }
}

#[test]
fn should_preserve_exact_output_for_empty_left_cte_on_right_join() {
    // Arrange
    with_fixture_config(
        "ordered_empty_left_outer_join",
        |config| config.limits.vectorized_joins_enabled = true,
        |cassie, session| {
            seed_ordered_outer_join_tables(cassie, session);
            let merge_joins_before = cassie.metrics()["joins"]["merge_joins"]
                .as_u64()
                .unwrap_or_default();

            // Act
            let empty_left = sql(
                cassie,
                session,
                "WITH empty_left(left_key, left_label) AS (SELECT left_key, left_label FROM ordered_left WHERE false) SELECT empty_left.left_key AS left_key_alias, empty_left.left_label AS left_label_alias, ordered_right.right_key AS right_key_alias, ordered_right.right_label AS right_label_alias FROM empty_left RIGHT JOIN ordered_right ON empty_left.left_key = ordered_right.right_key ORDER BY ordered_right.right_key",
            );

            // Assert
            let merge_joins_after = cassie.metrics()["joins"]["merge_joins"]
                .as_u64()
                .unwrap_or_default();
            assert_eq!(
                merge_joins_after.saturating_sub(merge_joins_before),
                1,
                "RIGHT JOIN uses its supported merge fallback"
            );
            assert_ordered_outer_join_result(
                &empty_left,
                &[
                    vec![
                        Value::Null,
                        Value::Null,
                        Value::Int64(1),
                        Value::String("right-one".into()),
                    ],
                    vec![
                        Value::Null,
                        Value::Null,
                        Value::Int64(2),
                        Value::String("right-two".into()),
                    ],
                ],
            );
        },
    );
}

fn seed_ordered_outer_join_tables(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
) {
    for statement in [
        "CREATE TABLE ordered_left (left_key INT, left_label TEXT)",
        "CREATE TABLE ordered_right (right_key INT, right_label TEXT)",
        "INSERT INTO ordered_left VALUES (1, 'left-one'), (2, 'left-two')",
        "INSERT INTO ordered_right VALUES (1, 'right-one'), (2, 'right-two')",
    ] {
        sql(cassie, session, statement);
    }
}

fn assert_ordered_outer_join_result(result: &cassie::executor::QueryResult, rows: &[Vec<Value>]) {
    let fields = result
        .columns()
        .iter()
        .map(|column| {
            (
                column.name.as_str(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fields,
        vec![
            ("left_key_alias", 23, 4, -1),
            ("left_label_alias", 25, -1, -1),
            ("right_key_alias", 23, 4, -1),
            ("right_label_alias", 25, -1, -1),
        ]
    );
    assert_eq!(result.rows(), rows);
}

#[test]
fn should_preserve_lateral_output_shape_when_a_left_row_has_no_match() {
    // Arrange
    with_fixture("lateral_empty_shape", |cassie, session| {
        for statement in [
            "CREATE TABLE lateral_users (user_key INT, name TEXT)",
            "CREATE TABLE lateral_orders (order_key INT, total INT)",
            "INSERT INTO lateral_users VALUES (1,'ada'),(2,'grace'),(3,'linus')",
            "INSERT INTO lateral_orders VALUES (1,42),(3,7)",
        ] {
            sql(cassie, session, statement);
        }
        // Act
        let result=sql(cassie,session,"SELECT * FROM lateral_users LEFT JOIN LATERAL (SELECT total FROM lateral_orders WHERE order_key=lateral_users.user_key) AS recent ON true ORDER BY lateral_users.name");
        // Assert
        assert_eq!(result.columns().len(), 4);
        assert_eq!(result.rows().len(), 3);
        for row in result.rows() {
            assert_eq!(row.len(), 4, "{row:?}");
        }
        assert_eq!(result.rows()[0][3], Value::Int64(42));
        assert_eq!(result.rows()[1][3], Value::Null);
        assert_eq!(result.rows()[2][3], Value::Int64(7));
    });
}
#[test]
fn should_preserve_nested_outer_join_shapes_in_each_execution_mode() {
    // Arrange
    for vectorized in [false, true] {
        with_fixture_config(
            "nested_join_shape",
            |config| config.limits.vectorized_joins_enabled = vectorized,
            |cassie, session| {
                for statement in [
                    "CREATE TABLE nested_populated (k INT)",
                    "CREATE TABLE nested_empty (k INT)",
                    "CREATE TABLE nested_other (k INT)",
                    "INSERT INTO nested_populated VALUES (1),(2)",
                ] {
                    sql(cassie, session, statement);
                }
                // Act
                for predicate in ["nested_populated.k=nested_empty.k", "false"] {
                    let result=sql(cassie,session,&format!("SELECT * FROM nested_populated LEFT JOIN nested_empty ON {predicate} LEFT JOIN nested_other ON nested_empty.k=nested_other.k"));
                    // Assert
                    assert_eq!(result.columns().len(), 6);
                    assert_eq!(result.rows().len(), 2);
                    for row in result.rows() {
                        assert_eq!(row.len(), 6, "{row:?}");
                        assert_eq!(
                            &row[2..],
                            &[Value::Null, Value::Null, Value::Null, Value::Null]
                        );
                    }
                    let qualified=sql(cassie,session,&format!("SELECT nested_empty.k,nested_other.k FROM nested_populated LEFT JOIN nested_empty ON {predicate} LEFT JOIN nested_other ON nested_empty.k=nested_other.k"));
                    assert_eq!(qualified.rows(), vec![vec![Value::Null, Value::Null]; 2]);
                }
            },
        );
    }
}
