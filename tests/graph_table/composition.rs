//! `GRAPH_TABLE` as an ordinary source: joins, LATERAL, aggregates, EXISTS, views and
//! prepared parameters.

use crate::world::{ints, link, node, World};
use cassie::types::Value;

fn world() -> World {
    World::simple(
        &[node(1), node(2), node(3), node(4)],
        &[link(1, 1, 2), link(2, 1, 3), link(3, 2, 3)],
    )
}

const PAIRS: &str = "GRAPH_TABLE (g MATCH (s IS node) -[IS link]-> (t IS node) \
                     COLUMNS (s.num AS src, t.num AS dst)) AS p";

#[test]
fn should_filter_a_graph_table_with_an_outer_where() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(&format!(
        "SELECT p.src, p.dst FROM {PAIRS} WHERE p.dst = 3 ORDER BY p.src"
    )));

    // Assert
    assert_eq!(rows, vec![vec![1, 3], vec![2, 3]]);
}

#[test]
fn should_join_a_graph_table_with_an_ordinary_table() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(&format!(
        "SELECT n.score, p.dst FROM node n JOIN {PAIRS} ON p.src = n.num ORDER BY n.score, p.dst"
    )));

    // Assert
    assert_eq!(rows, vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
}

#[test]
fn should_correlate_a_lateral_graph_table_per_outer_row() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(
        "SELECT n.num, t.dst FROM node n, LATERAL GRAPH_TABLE (g MATCH \
         (a IS node WHERE a.num = n.num) -[IS link]-> (b IS node) COLUMNS (b.num AS dst)) AS t \
         ORDER BY n.num, t.dst",
    ));

    // Assert
    assert_eq!(rows, vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
}

#[test]
fn should_aggregate_over_a_graph_table() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(&format!(
        "SELECT p.src, count(*) FROM {PAIRS} GROUP BY p.src ORDER BY p.src"
    )));

    // Assert
    assert_eq!(rows, vec![vec![1, 2], vec![2, 1]]);
}

#[test]
fn should_use_a_graph_table_inside_exists() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(
        "SELECT n.num FROM node n WHERE EXISTS (SELECT 1 FROM GRAPH_TABLE (g MATCH \
         (a IS node) -[IS link]-> (b IS node WHERE b.num = 3) COLUMNS (a.num AS src)) AS t \
         WHERE t.src = n.num) ORDER BY n.num",
    ));

    // Assert
    assert_eq!(rows, vec![vec![1], vec![2]]);
}

#[test]
fn should_read_a_graph_table_through_a_derived_table() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(&format!(
        "SELECT d.total FROM (SELECT count(*) AS total FROM {PAIRS}) AS d"
    )));

    // Assert
    assert_eq!(rows, vec![vec![3]]);
}

#[test]
fn should_read_a_view_over_a_graph_table() {
    // Arrange
    let world = world();
    world
        .fixture
        .execute(&format!(
            "CREATE VIEW pairs AS SELECT p.src, p.dst FROM {PAIRS}"
        ))
        .expect("create view");

    // Act
    let before = world.fixture.rows("SELECT * FROM pairs").len();
    world
        .fixture
        .execute("INSERT INTO link VALUES (4, 3, 4, TRUE, 1)")
        .expect("insert");
    let after = world.fixture.rows("SELECT * FROM pairs").len();

    // Assert
    assert_eq!((before, after), (3, 4));
}

#[test]
fn should_bind_parameters_in_a_prepared_graph_table() {
    // Arrange
    let world = world();
    let sql = "SELECT t.dst FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = $1) \
               -[e IS link WHERE e.weight >= $2]-> (b IS node) COLUMNS (b.num AS dst)) AS t \
               ORDER BY t.dst";

    // Act
    let run = |source: i64, weight: i64| {
        let params = vec![Value::Int64(source), Value::Int64(weight)];
        match world
            .fixture
            .cassie
            .execute_sql(&world.fixture.session, sql, params)
        {
            Ok(result) => ints(result.rows().to_vec()),
            Err(error) => panic!(
                "prepared statement failed with SQLSTATE {}",
                error.sql_state()
            ),
        }
    };

    // Assert
    assert_eq!(run(1, 1), vec![vec![2], vec![3]]);
    assert_eq!(run(2, 1), vec![vec![3]]);
    assert_eq!(run(1, 2), Vec::<Vec<i64>>::new());
}

#[test]
fn should_reevaluate_a_lateral_parameter_for_each_outer_row() {
    // Arrange
    let world = world();

    // Act
    let rows = ints(world.fixture.rows(
        "SELECT o.k, t.dst FROM generate_series(1, 2) AS o(k), LATERAL GRAPH_TABLE (g MATCH \
         (a IS node WHERE a.num = o.k) -[IS link]-> (b IS node) COLUMNS (b.num AS dst)) AS t \
         ORDER BY o.k, t.dst",
    ));

    // Assert
    assert_eq!(rows, vec![vec![1, 2], vec![1, 3], vec![2, 3]]);
}
