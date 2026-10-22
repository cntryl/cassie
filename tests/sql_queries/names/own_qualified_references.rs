use super::support_sql as support;

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

use support::data_dir;

#[test]
fn should_resolve_aggregate_clauses_given_own_qualified_columns_when_values_and_types_are_projected(
) {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "own-qualified-aggregate-modifiers",
        &[
            "CREATE TABLE qualifier_inputs (amount INT4, included BOOLEAN)",
            "INSERT INTO qualifier_inputs VALUES \
             (2, TRUE), (2, TRUE), (5, FALSE), (7, NULL), (NULL, TRUE)",
        ],
    );
    let qualifiers = [
        "qualifier_inputs",
        "public.qualifier_inputs",
        "postgres.public.qualifier_inputs",
    ];

    // Act
    let results = qualifiers.map(|qualifier| {
        fixture.execute(&format!(
            "SELECT SUM(DISTINCT {qualifier}.amount) FILTER (WHERE {qualifier}.included), \
             COUNT({qualifier}.amount ORDER BY {qualifier}.amount DESC) \
             FILTER (WHERE {qualifier}.included), \
             BOOL_AND({qualifier}.included ORDER BY {qualifier}.amount) FROM qualifier_inputs"
        ))
    });

    // Assert
    for result in results {
        let result = result.expect("qualified aggregate arguments, filters and sort keys");
        assert_eq!(
            result.rows(),
            &[vec![Value::Int64(2), Value::Int64(2), Value::Bool(false)]]
        );
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [20, 20, 16]
        );
    }
}

#[test]
fn should_not_strip_a_qualified_reference_to_a_distinct_relation_with_the_same_local_name() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "distinct-qualified-own-name",
        &[
            "CREATE TABLE qualifier_inputs (amount INT4)",
            "CREATE TABLE public.qualifier_inputs (other INT4)",
        ],
    );

    // Act
    let error = fixture.error("SELECT public.qualifier_inputs.amount FROM qualifier_inputs");

    // Assert
    assert!(matches!(
        error,
        CassieError::UndefinedColumn(name) if name == "public.qualifier_inputs.amount"
    ));
}

#[test]
fn should_resolve_aggregate_clauses_given_a_quoted_relation_alias_when_filter_parameters_are_bound()
{
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "quoted-alias-aggregate-modifiers",
        &[
            "CREATE TABLE alias_inputs (amount INT4, included BOOLEAN)",
            "INSERT INTO alias_inputs VALUES (2, TRUE), (2, TRUE), (5, FALSE), (7, NULL)",
        ],
    );

    // Act
    let results = [true, false].map(|included| {
        fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT SUM(DISTINCT \"A\".amount ORDER BY \"A\".amount) \
             FILTER (WHERE \"A\".included = $1) FROM alias_inputs AS \"A\"",
            vec![Value::Bool(included)],
        )
    });

    // Assert
    for (result, expected) in results.into_iter().zip([2, 5]) {
        let result = result.expect("quoted aliases reach all aggregate clauses");
        assert_eq!(result.rows(), &[vec![Value::Int64(expected)]]);
        assert_eq!(result.columns()[0].type_oid, 20);
    }
}

fn seeded(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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

fn two_rows(label: &str) -> (Cassie, CassieSession, String) {
    seeded(
        label,
        &[
            "CREATE TABLE tq (k INT, name TEXT)",
            "INSERT INTO tq (k, name) VALUES (1, 'ada'), (2, 'bob')",
        ],
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_project_a_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_projection");

    // Act
    let projected = rows(
        &cassie,
        &session,
        "SELECT tq.k, tq.name FROM tq ORDER BY tq.k",
    );

    // Assert
    assert_eq!(
        projected,
        vec![
            vec![Value::Int64(1), text("ada")],
            vec![Value::Int64(2), text("bob")]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_on_a_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = seeded(
            "own_qualified_filter",
            &[
                "CREATE TABLE apdocs (title TEXT, score FLOAT, n INT)",
                "INSERT INTO apdocs (title, score, n) VALUES ('a', 1.0, 1), ('b', 2.5, 2), ('c', 3.0, 3), ('d', 4.5, 4)",
            ],
        );

    // Act
    let filtered = rows(
        &cassie,
        &session,
        "SELECT apdocs.title FROM apdocs WHERE apdocs.score > 2.0 ORDER BY apdocs.title",
    );
    let schema_qualified = rows(
        &cassie,
        &session,
        "SELECT public.apdocs.n FROM apdocs WHERE public.apdocs.title = 'c'",
    );

    // Assert
    assert_eq!(
        filtered,
        vec![vec![text("b")], vec![text("c")], vec![text("d")]]
    );
    assert_eq!(schema_qualified, vec![vec![Value::Int64(3)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_an_indexed_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = seeded(
        "own_qualified_indexed_filter",
        &[
            "CREATE TABLE ti (k INT, name TEXT)",
            "INSERT INTO ti (k, name) VALUES (1, 'ada'), (2, 'bob')",
            "CREATE INDEX ti_k ON ti (k)",
        ],
    );

    // Act
    let filtered = rows(&cassie, &session, "SELECT ti.name FROM ti WHERE ti.k = 2");

    // Assert
    assert_eq!(filtered, vec![vec![text("bob")]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_aggregate_columns_qualified_by_their_own_table() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_aggregate");

    // Act
    let summed = rows(&cassie, &session, "SELECT SUM(tq.k) FROM tq");
    let grouped = rows(
        &cassie,
        &session,
        "SELECT tq.name, COUNT(*) FROM tq GROUP BY tq.name ORDER BY tq.name",
    );

    // Assert
    assert_eq!(summed, vec![vec![Value::Int64(3)]]);
    assert_eq!(
        grouped,
        vec![
            vec![text("ada"), Value::Int64(1)],
            vec![text("bob"), Value::Int64(1)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_top_k_order_a_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_top_k");

    // Act
    let top = rows(
        &cassie,
        &session,
        "SELECT tq.name FROM tq ORDER BY tq.k DESC LIMIT 1",
    );

    // Assert
    assert_eq!(top, vec![vec![text("bob")]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_a_cte_column_qualified_by_the_cte_name() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_cte");

    // Act
    let filtered = rows(
        &cassie,
        &session,
        "WITH c AS (SELECT k, name FROM tq) SELECT c.name FROM c WHERE c.k = 1",
    );

    // Assert
    assert_eq!(filtered, vec![vec![text("ada")]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_update_rows_matched_on_a_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_update");

    // Act
    let updated = cassie
        .execute_sql(
            &session,
            "UPDATE tq SET name = 'zed' WHERE tq.k = 2",
            vec![],
        )
        .expect("update");
    let names = rows(&cassie, &session, "SELECT name FROM tq ORDER BY k");

    // Assert
    assert_eq!(updated.command(), "UPDATE 1");
    assert_eq!(names, vec![vec![text("ada")], vec![text("zed")]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_delete_rows_matched_on_a_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = two_rows("own_qualified_delete");

    // Act
    let deleted = cassie
        .execute_sql(&session, "DELETE FROM tq WHERE tq.k = 2", vec![])
        .expect("delete");
    let remaining = rows(&cassie, &session, "SELECT name FROM tq");

    // Assert
    assert_eq!(deleted.command(), "DELETE 1");
    assert_eq!(remaining, vec![vec![text("ada")]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_a_lateral_body_column_qualified_by_its_own_table() {
    // Arrange
    let (cassie, session, path) = seeded(
        "own_qualified_lateral",
        &[
            "CREATE TABLE sh_u (k INT, name TEXT)",
            "CREATE TABLE sh_o (k INT, total INT)",
            "INSERT INTO sh_u (k, name) VALUES (1, 'ada'), (2, 'bob')",
            "INSERT INTO sh_o (k, total) VALUES (1, 10), (2, 20)",
        ],
    );

    // Act
    let joined = rows(
        &cassie,
        &session,
        "SELECT sh_u.name, x.total FROM sh_u JOIN LATERAL \
             (SELECT total FROM sh_o WHERE sh_o.k = sh_u.k) AS x ON true ORDER BY sh_u.name",
    );

    // Assert
    assert_eq!(
        joined,
        vec![
            vec![text("ada"), Value::Int64(10)],
            vec![text("bob"), Value::Int64(20)]
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}
