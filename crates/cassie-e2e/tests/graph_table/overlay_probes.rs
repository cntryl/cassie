//! The lazy indexed read against the exact read for uncommitted changes that move an edge
//! between vertices, remove or re-key a vertex, and for text keys that differ only by case
//! or trailing space.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use crate::world::{ints, link, node, Variant, VariantGuard, World};
use cassie::app::CassieSession;
use cassie::types::Value;

const INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};

fn rows_of(fixture: &SqlFixture, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

/// The rows of each query of a batch, in query order.
type Results = Vec<Vec<Vec<Value>>>;

/// Each query's rows under the lazy store and under the forced exact read.
fn both(fixture: &SqlFixture, session: &CassieSession, queries: &[String]) -> (Results, Results) {
    let run = |exact: bool| {
        fixture.cassie.set_graph_exact_reads(exact);
        queries
            .iter()
            .map(|sql| rows_of(fixture, session, sql))
            .collect::<Vec<_>>()
    };
    let lazy = run(false);
    let exact = run(true);
    fixture.cassie.set_graph_exact_reads(false);
    (lazy, exact)
}

fn neighbours(anchor: i64) -> Vec<String> {
    ["-[e IS link]->", "<-[e IS link]-", "-[e IS link]-"]
        .iter()
        .map(|edge| {
            format!(
                "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = {anchor}) {edge} \
                 (b IS node) COLUMNS (e.num AS e, b.num AS b)) AS t ORDER BY e, b"
            )
        })
        .collect()
}

fn ring_world() -> World {
    let _guard = VariantGuard::set(INDEXED);
    World::simple(
        &(1..=6).map(node).collect::<Vec<_>>(),
        &[
            link(1, 1, 2),
            link(2, 1, 3),
            link(3, 4, 5),
            link(4, 5, 6),
            link(5, 6, 4),
        ],
    )
}

#[test]
fn should_match_the_exact_read_when_pending_updates_move_edges_between_vertices() {
    // Arrange
    let world = ring_world();
    let session = &world.fixture.session;
    for sql in [
        "BEGIN",
        "UPDATE link SET src = 4 WHERE num = 1",
        "UPDATE link SET src = 1 WHERE num = 3",
        "UPDATE link SET dst = 1 WHERE num = 4",
        "INSERT INTO link VALUES (90, 1, 6, TRUE, 1)",
        "DELETE FROM link WHERE num = 90",
        "INSERT INTO link VALUES (91, 6, 1, TRUE, 1)",
        "UPDATE link SET src = 2 WHERE num = 91",
    ] {
        rows_of(&world.fixture, session, sql);
    }
    let queries = [1, 2, 4, 5, 6]
        .into_iter()
        .flat_map(neighbours)
        .collect::<Vec<_>>();

    // Act
    let (lazy, exact) = both(&world.fixture, session, &queries);
    let from_one = ints(lazy[0].clone());

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(from_one, vec![vec![2, 3], vec![3, 5]]);
}

#[test]
fn should_match_the_exact_read_when_pending_changes_remove_or_rekey_a_vertex() {
    // Arrange
    let world = ring_world();
    let session = &world.fixture.session;
    for sql in [
        "BEGIN",
        "DELETE FROM node WHERE num = 2",
        "UPDATE node SET num = 77 WHERE num = 3",
        "INSERT INTO node VALUES (2, 'again', 9)",
        "SAVEPOINT s",
        "DELETE FROM node WHERE num = 5",
        "ROLLBACK TO SAVEPOINT s",
    ] {
        rows_of(&world.fixture, session, sql);
    }
    let queries = [1, 2, 3, 4, 5, 77]
        .into_iter()
        .flat_map(neighbours)
        .collect::<Vec<_>>();

    // Act
    let (lazy, exact) = both(&world.fixture, session, &queries);
    let from_one = ints(lazy[0].clone());

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(from_one, vec![vec![1, 2]]);
}

#[test]
fn should_match_the_exact_read_after_a_truncate_inside_the_transaction() {
    // Arrange
    let world = ring_world();
    let session = &world.fixture.session;
    for sql in [
        "BEGIN",
        "TRUNCATE link",
        "INSERT INTO link VALUES (7, 1, 4, TRUE, 1)",
    ] {
        rows_of(&world.fixture, session, sql);
    }
    let queries = [1, 4].into_iter().flat_map(neighbours).collect::<Vec<_>>();

    // Act
    let (lazy, exact) = both(&world.fixture, session, &queries);
    let from_one = ints(lazy[0].clone());

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(from_one, vec![vec![7, 4]]);
}

#[test]
fn should_match_the_exact_read_for_text_keys_that_differ_by_case_or_padding() {
    // Arrange
    let fixture = sql_fixture(
        "graph-text-keys",
        &[
            "CREATE TABLE t_node (code TEXT PRIMARY KEY, n BIGINT)",
            "CREATE TABLE t_link (id BIGINT PRIMARY KEY, src TEXT NOT NULL, dst TEXT NOT NULL)",
            "INSERT INTO t_node VALUES ('a', 1), ('A', 2), ('a ', 3), ('-1', 4), ('', 5)",
            "INSERT INTO t_link VALUES (1, 'a', 'A'), (2, 'a ', 'a'), (3, 'A', '-1'), \
             (4, 'a', 'a '), (5, '', 'a'), (6, 'a', ''), (7, 'missing', 'a')",
            "CREATE INDEX t_link_src ON t_link (src)",
            "CREATE INDEX t_link_dst ON t_link (dst)",
            "CREATE PROPERTY GRAPH tg \
               VERTEX TABLES (t_node AS n KEY (code) LABEL node PROPERTIES (code, n)) \
               EDGE TABLES (t_link AS l KEY (id) \
                 SOURCE KEY (src) REFERENCES n (code) \
                 DESTINATION KEY (dst) REFERENCES n (code) \
                 LABEL link PROPERTIES (id))",
        ],
    );
    let queries = ["a", "A", "a ", "-1", ""]
        .iter()
        .flat_map(|code| {
            ["-[e IS link]->", "<-[e IS link]-"].map(|edge| {
                format!(
                    "SELECT * FROM GRAPH_TABLE (tg MATCH (a IS node WHERE a.code = '{code}') \
                     {edge} (b IS node) COLUMNS (e.id AS e, b.n AS b)) AS t ORDER BY e"
                )
            })
        })
        .collect::<Vec<_>>();

    // Act
    let (lazy, exact) = both(&fixture, &fixture.session, &queries);
    let outward_from_a = ints(lazy[0].clone());

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(outward_from_a, vec![vec![1, 2], vec![4, 3], vec![6, 5]]);
}

/// The edge table after a statement that reads the graph while inserting into it, run twice.
fn edges_after_self_insert(exact: bool) -> Vec<Vec<i64>> {
    let world = ring_world();
    world.fixture.cassie.set_graph_exact_reads(exact);
    let session = &world.fixture.session;
    for offset in [100, 200] {
        rows_of(
            &world.fixture,
            session,
            &format!(
                "INSERT INTO link SELECT t.e + {offset}, t.b, 1, TRUE, 1 FROM GRAPH_TABLE (g MATCH \
                 (a IS node WHERE a.num = 1) -[e IS link]-> (b IS node) \
                 COLUMNS (e.num AS e, b.num AS b)) AS t"
            ),
        );
    }
    ints(rows_of(
        &world.fixture,
        session,
        "SELECT num, src, dst FROM link ORDER BY num",
    ))
}

#[test]
fn should_read_the_statement_snapshot_when_a_statement_writes_the_table_it_traverses() {
    // Arrange
    let expected_new = vec![
        vec![101, 2, 1],
        vec![102, 3, 1],
        vec![201, 2, 1],
        vec![202, 3, 1],
    ];

    // Act
    let lazy = edges_after_self_insert(false);
    let exact = edges_after_self_insert(true);
    let inserted = lazy
        .iter()
        .filter(|row| row[0] > 100)
        .cloned()
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(lazy, exact);
    assert_eq!(inserted, expected_new);
}
