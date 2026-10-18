use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::{data_dir, use_local_storage};

fn seeded(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in statements {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    (cassie, session, path)
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .to_vec()
}

fn mixed_case_scores(label: &str) -> (Cassie, CassieSession, String) {
    seeded(
        label,
        &[
            "CREATE TABLE mc (Label TEXT, Score INT)",
            "INSERT INTO mc (Label, Score) VALUES ('low', 1), ('mid', 5), ('high', 9)",
        ],
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_aggregate_a_mixed_case_column_referenced_in_lowercase() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_aggregate_lower");

    // Act
    let aggregates = rows(
        &cassie,
        &session,
        "SELECT MAX(score), MIN(score), SUM(score), COUNT(score) FROM mc",
    );

    // Assert
    assert_eq!(
        aggregates,
        vec![vec![
            Value::Int64(9),
            Value::Int64(1),
            Value::Int64(15),
            Value::Int64(3)
        ]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_aggregate_a_lowercase_column_referenced_in_uppercase() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_aggregate_upper",
        &[
            "CREATE TABLE a (x INT, t TEXT)",
            "INSERT INTO a (x, t) VALUES (1, 'P'), (2, 'Q'), (3, 'R'), (4, 'S'), (5, 'T')",
        ],
    );

    // Act
    let aggregates = rows(&cassie, &session, "SELECT SUM(X), COUNT(X), MAX(X) FROM a");

    // Assert
    assert_eq!(
        aggregates,
        vec![vec![Value::Int64(15), Value::Int64(5), Value::Int64(5)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_evaluate_scalar_expressions_over_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_scalar_expression",
        &[
            "CREATE TABLE upper_t (AMOUNT INT, Tag TEXT)",
            "INSERT INTO upper_t (amount, tag) VALUES (5, 'Aa'), (7, 'Bb')",
        ],
    );

    // Act
    let evaluated = rows(
        &cassie,
        &session,
        "SELECT amount + 1, lower(TAG) FROM upper_t ORDER BY amount",
    );

    // Assert
    assert_eq!(
        evaluated,
        vec![
            vec![Value::Int64(6), text("aa")],
            vec![Value::Int64(8), text("bb")]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_on_an_other_case_column_in_where() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_where_filter");

    // Act
    let counted = rows(&cassie, &session, "SELECT COUNT(*) FROM mc WHERE score > 2");
    let selected = rows(
        &cassie,
        &session,
        "SELECT label, SCORE FROM mc WHERE score > 2 ORDER BY SCORE",
    );

    // Assert
    assert_eq!(counted, vec![vec![Value::Int64(2)]]);
    assert_eq!(
        selected,
        vec![
            vec![text("mid"), Value::Int64(5)],
            vec![text("high"), Value::Int64(9)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_group_by_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_group_by",
        &[
            "CREATE TABLE gb (g TEXT, n INT)",
            "INSERT INTO gb (g, n) VALUES ('x', 1), ('x', 2), ('y', 3)",
        ],
    );

    // Act
    let grouped = rows(
        &cassie,
        &session,
        "SELECT G, count(*), sum(N) FROM gb GROUP BY G ORDER BY g",
    );

    // Assert
    assert_eq!(
        grouped,
        vec![
            vec![text("x"), Value::Int64(2), Value::Int64(3)],
            vec![text("y"), Value::Int64(1), Value::Int64(3)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_group_by_a_mixed_case_column_referenced_in_lowercase() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_group_by_declared");

    // Act
    let grouped = rows(
        &cassie,
        &session,
        "SELECT label, COUNT(*) FROM mc GROUP BY label ORDER BY label",
    );

    // Assert
    assert_eq!(
        grouped,
        vec![
            vec![text("high"), Value::Int64(1)],
            vec![text("low"), Value::Int64(1)],
            vec![text("mid"), Value::Int64(1)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_groups_on_an_other_case_aggregate_in_having() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_having",
        &[
            "CREATE TABLE m (g TEXT, v INT)",
            "INSERT INTO m (g, v) VALUES ('a', 1), ('a', 2), ('b', 3), ('c', 1)",
        ],
    );

    // Act
    let grouped = rows(
        &cassie,
        &session,
        "SELECT g, SUM(v) FROM m GROUP BY g HAVING SUM(V) > 2 ORDER BY g",
    );

    // Assert
    assert_eq!(
        grouped,
        vec![
            vec![text("a"), Value::Int64(3)],
            vec![text("b"), Value::Int64(3)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_distinct_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_distinct",
        &[
            "CREATE TABLE upper_d (AMOUNT INT)",
            "INSERT INTO upper_d (amount) VALUES (5), (7), (5)",
        ],
    );

    // Act
    let distinct = rows(
        &cassie,
        &session,
        "SELECT DISTINCT amount FROM upper_d ORDER BY amount",
    );

    // Assert
    assert_eq!(distinct, vec![vec![Value::Int64(5)], vec![Value::Int64(7)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_sort_unprojected_rows_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_unprojected_sort",
        &[
            "CREATE TABLE oc (v INT, w TEXT)",
            "INSERT INTO oc (v, w) VALUES (5, 'e'), (1, 'a'), (4, 'd'), (2, 'b'), (3, 'c')",
        ],
    );

    // Act
    let sorted = rows(&cassie, &session, "SELECT * FROM oc ORDER BY W LIMIT 3");

    // Assert
    let order = sorted
        .iter()
        .map(|row| row[row.len() - 1].clone())
        .collect::<Vec<_>>();
    assert_eq!(order, vec![text("a"), text("b"), text("c")]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_join_on_qualified_mixed_case_columns() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_qualified_join",
        &[
            "CREATE TABLE CaseUsers (UserKey INT, Name TEXT)",
            "CREATE TABLE CaseOrders (OrderUserKey INT, Total INT)",
            "INSERT INTO CaseUsers (UserKey, Name) VALUES (1, 'ada')",
            "INSERT INTO CaseOrders (OrderUserKey, Total) VALUES (1, 42)",
        ],
    );

    // Act
    let inner = rows(
        &cassie,
        &session,
        "SELECT CaseUsers.Name, CaseOrders.Total FROM CaseUsers \
         JOIN CaseOrders ON CaseUsers.UserKey = CaseOrders.OrderUserKey",
    );
    let left = rows(
        &cassie,
        &session,
        "SELECT caseusers.name, caseorders.total FROM CaseUsers \
         LEFT JOIN CaseOrders ON caseusers.userkey = caseorders.orderuserkey",
    );

    // Assert
    assert_eq!(inner, vec![vec![text("ada"), Value::Int64(42)]]);
    assert_eq!(left, inner);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_update_rows_matched_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_update");

    // Act
    let updated = cassie
        .execute_sql(
            &session,
            "UPDATE mc SET Score = 100 WHERE score = 1",
            vec![],
        )
        .expect("update");
    let scores = rows(&cassie, &session, "SELECT Score FROM mc ORDER BY Score");

    // Assert
    assert_eq!(updated.command(), "UPDATE 1");
    assert_eq!(
        scores,
        vec![
            vec![Value::Int64(5)],
            vec![Value::Int64(9)],
            vec![Value::Int64(100)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_delete_rows_matched_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_delete");

    // Act
    let deleted = cassie
        .execute_sql(&session, "DELETE FROM mc WHERE score = 9", vec![])
        .expect("delete");
    let remaining = rows(&cassie, &session, "SELECT COUNT(*) FROM mc");

    // Assert
    assert_eq!(deleted.command(), "DELETE 1");
    assert_eq!(remaining, vec![vec![Value::Int64(2)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_project_one_column_referenced_in_several_spellings() {
    // Arrange
    let (cassie, session, path) = mixed_case_scores("case_repeated_projection");

    // Act
    let repeated = rows(
        &cassie,
        &session,
        "SELECT DISTINCT score, SCORE, Score FROM mc ORDER BY score",
    );

    // Assert
    assert_eq!(
        repeated,
        vec![
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(5), Value::Int64(5), Value::Int64(5)],
            vec![Value::Int64(9), Value::Int64(9), Value::Int64(9)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_project_a_repeated_column_on_every_occurrence() {
    // Arrange
    let (cassie, session, path) = seeded(
        "repeated_projection",
        &[
            "CREATE TABLE apdocs (title TEXT, n INT)",
            "INSERT INTO apdocs (title, n) VALUES ('a', 1), ('b', 2)",
        ],
    );

    // Act
    let repeated = rows(&cassie, &session, "SELECT n, n, n FROM apdocs ORDER BY n");
    let aliased = rows(
        &cassie,
        &session,
        "SELECT title AS t1, title AS t2 FROM apdocs ORDER BY title",
    );

    // Assert
    assert_eq!(
        repeated,
        vec![
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2), Value::Int64(2)]
        ]
    );
    assert_eq!(
        aliased,
        vec![vec![text("a"), text("a")], vec![text("b"), text("b")]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_project_a_column_beside_a_wildcard_on_both_sides() {
    // Arrange
    let (cassie, session, path) = seeded(
        "wildcard_repeated_projection",
        &[
            "CREATE TABLE wdocs (title TEXT, n INT)",
            "INSERT INTO wdocs (title, n) VALUES ('a', 1)",
        ],
    );

    // Act
    let trailing = cassie
        .execute_sql(&session, "SELECT *, n FROM wdocs", vec![])
        .expect("wildcard then column");
    let leading = cassie
        .execute_sql(&session, "SELECT n, * FROM wdocs", vec![])
        .expect("column then wildcard");

    // Assert
    let last = trailing.rows()[0].len() - 1;
    assert_eq!(trailing.rows()[0][last], Value::Int64(1));
    assert!(trailing.rows()[0][..last].contains(&Value::Int64(1)));
    assert_eq!(leading.rows()[0][0], Value::Int64(1));
    assert!(leading.rows()[0][1..].contains(&Value::Int64(1)));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_agree_across_aggregate_paths_for_an_other_case_indexed_column() {
    // Arrange
    let (cassie, session, path) = seeded(
        "case_column_batch_aggregate",
        &[
            "CREATE TABLE mcb (Amount INT, Grp INT)",
            "INSERT INTO mcb (Amount, Grp) VALUES (1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1)",
            "CREATE INDEX mcbix ON mcb USING column (Amount, Grp) WITH (segment_size = 2)",
        ],
    );

    // Act
    let accelerated = rows(&cassie, &session, "SELECT SUM(amount) FROM mcb");
    let limited = rows(&cassie, &session, "SELECT SUM(amount) FROM mcb LIMIT 1");
    let grouped = rows(
        &cassie,
        &session,
        "SELECT SUM(amount) FROM mcb GROUP BY grp",
    );
    let counted = rows(&cassie, &session, "SELECT COUNT(amount) FROM mcb LIMIT 1");

    // Assert
    assert_eq!(accelerated, vec![vec![Value::Int64(21)]]);
    assert_eq!(limited, accelerated);
    assert_eq!(grouped, accelerated);
    assert_eq!(counted, vec![vec![Value::Int64(6)]]);
    let _ = std::fs::remove_dir_all(path);
}
