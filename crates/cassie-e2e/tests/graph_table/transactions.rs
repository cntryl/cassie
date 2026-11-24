//! The statement's own transaction view: uncommitted writes, savepoints, other sessions and
//! snapshot isolation (#1254).

use crate::world::{ints, link, node, World};
use cassie::app::CassieSession;

const EDGES: &str = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[e IS link]-> (t IS node) \
                     COLUMNS (e.num AS e)) AS t ORDER BY e";
const ACTIVE_EDGES: &str = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) \
                            -[e IS link WHERE e.active]-> (t IS node) COLUMNS (e.num AS e)) AS t \
                            ORDER BY e";

fn base() -> World {
    World::simple(&[node(1), node(2)], &[link(1, 1, 2)])
}

fn run(world: &World, session: &CassieSession, sql: &str) -> Vec<Vec<i64>> {
    match world.fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => ints(result.rows().to_vec()),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn exec(world: &World, session: &CassieSession, sql: &str) {
    if let Err(error) = world.fixture.cassie.execute_sql(session, sql, vec![]) {
        panic!("statement failed with SQLSTATE {}", error.sql_state());
    }
}

fn other_session(world: &World) -> CassieSession {
    world.fixture.cassie.create_session("tester", None)
}

#[test]
pub(crate) fn should_see_own_uncommitted_inserts() {
    // Arrange
    let world = base();
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");

    // Act
    exec(&world, session, "INSERT INTO node VALUES (3, 'n', 3)");
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (2, 2, 3, TRUE, 1)",
    );
    let inside = run(&world, session, EDGES);

    // Assert
    assert_eq!(inside, vec![vec![1], vec![2]]);
}

#[test]
pub(crate) fn should_hide_uncommitted_writes_from_another_session_until_commit() {
    // Arrange
    let world = base();
    let writer = &world.fixture.session;
    let reader = other_session(&world);
    exec(&world, writer, "BEGIN");
    exec(&world, writer, "INSERT INTO node VALUES (3, 'n', 3)");
    exec(&world, writer, "INSERT INTO link VALUES (2, 2, 3, TRUE, 1)");

    // Act
    let before = run(&world, &reader, EDGES);
    exec(&world, writer, "COMMIT");
    let after = run(&world, &reader, EDGES);

    // Assert
    assert_eq!((before, after), (vec![vec![1]], vec![vec![1], vec![2]]));
}

#[test]
pub(crate) fn should_see_an_own_update_that_changes_an_inline_predicate() {
    // Arrange
    let world = base();
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");

    // Act
    let before = run(&world, session, ACTIVE_EDGES);
    exec(
        &world,
        session,
        "UPDATE link SET active = FALSE WHERE num = 1",
    );
    let inside = run(&world, session, ACTIVE_EDGES);
    let outside = run(&world, &other_session(&world), ACTIVE_EDGES);

    // Assert
    assert_eq!(
        (before, inside, outside),
        (vec![vec![1]], vec![], vec![vec![1]])
    );
}

#[test]
pub(crate) fn should_see_own_deletes() {
    // Arrange
    let world = World::simple(
        &[node(1), node(2), node(3)],
        &[link(1, 1, 2), link(2, 2, 3)],
    );
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");

    // Act
    exec(&world, session, "DELETE FROM link WHERE num = 1");
    let after_edge = run(&world, session, EDGES);
    exec(&world, session, "DELETE FROM node WHERE num = 3");
    let after_vertex = run(&world, session, EDGES);

    // Assert
    assert_eq!((after_edge, after_vertex), (vec![vec![2]], vec![]));
}

#[test]
pub(crate) fn should_follow_repeated_mutation_of_one_element() {
    // Arrange
    let world = base();
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");

    // Act
    exec(&world, session, "DELETE FROM node WHERE num = 2");
    let deleted = run(&world, session, EDGES);
    exec(&world, session, "INSERT INTO node VALUES (2, 'again', 2)");
    let reinserted = run(&world, session, EDGES);
    exec(
        &world,
        session,
        "UPDATE link SET active = FALSE WHERE num = 1",
    );
    exec(
        &world,
        session,
        "UPDATE link SET active = TRUE WHERE num = 1",
    );
    let restored = run(&world, session, ACTIVE_EDGES);

    // Assert
    assert_eq!(
        (deleted, reinserted, restored),
        (vec![], vec![vec![1]], vec![vec![1]])
    );
}

#[test]
pub(crate) fn should_roll_back_to_a_savepoint() {
    // Arrange
    let world = base();
    let session = &world.fixture.session;
    exec(&world, session, "BEGIN");
    exec(&world, session, "SAVEPOINT before_edge");

    // Act
    exec(
        &world,
        session,
        "INSERT INTO link VALUES (2, 2, 1, TRUE, 1)",
    );
    let inside = run(&world, session, EDGES);
    exec(&world, session, "ROLLBACK TO SAVEPOINT before_edge");
    let rolled_back = run(&world, session, EDGES);

    // Assert
    assert_eq!(
        (inside, rolled_back),
        (vec![vec![1], vec![2]], vec![vec![1]])
    );
}

#[test]
pub(crate) fn should_keep_a_repeatable_read_snapshot_across_a_concurrent_commit() {
    // Arrange
    let world = base();
    let reader = &world.fixture.session;
    let writer = other_session(&world);
    exec(&world, reader, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    let first = run(&world, reader, EDGES);

    // Act
    exec(
        &world,
        &writer,
        "INSERT INTO link VALUES (2, 2, 1, TRUE, 1)",
    );
    let snapshot = run(&world, reader, EDGES);
    exec(&world, reader, "COMMIT");
    let fresh = run(&world, reader, EDGES);

    // Assert
    assert_eq!(
        (first, snapshot, fresh),
        (vec![vec![1]], vec![vec![1]], vec![vec![1], vec![2]])
    );
}
