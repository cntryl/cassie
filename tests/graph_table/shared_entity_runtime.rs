//! The statement's transaction view, memory budget and cancellation for joined patterns
//! (#1353).

use crate::shared_entity_fixture::{cells, limited_world, strings, world};
use crate::support_sql_fixture::SqlFixture;
use cassie::app::CassieSession;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::{QueryCancellationHandle, QueryScoringTestBoundary};
use std::time::Duration;

/// Sources of tenant `a`'s vertex 3 paired with tenant `a`'s new vertex 6.
const WITH_NEW_PERSON: &str = "SELECT g.p FROM GRAPH_TABLE (social MATCH \
     (a IS person) -[IS knows]-> (n IS person), \
     (b IS person WHERE b.tenant = 'a' AND b.id = 6) -[IS knows]-> (n) \
     COLUMNS (a.id AS p)) AS g";

const COMMON_NEIGHBORS: &str = "SELECT g.tenant, g.p, g.q, g.n FROM GRAPH_TABLE (social MATCH \
     (a IS person) -[IS knows]-> (n IS person), (b IS person) -[IS knows]-> (n) \
     WHERE a.id < b.id \
     COLUMNS (n.tenant AS tenant, a.id AS p, b.id AS q, n.id AS n)) AS g";

const ADD_PERSON: &str = "INSERT INTO person VALUES ('a', 6, 'fay')";
const ADD_EDGE: &str = "INSERT INTO knows VALUES (9, 'a', 6, 'a', 3)";

fn run(world: &SqlFixture, session: &CassieSession, sql: &str) -> Vec<String> {
    match world.cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => cells(result.rows().to_vec()),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn exec(world: &SqlFixture, session: &CassieSession, sql: &str) {
    if let Err(error) = world.cassie.execute_sql(session, sql, vec![]) {
        panic!("statement failed with SQLSTATE {}", error.sql_state());
    }
}

#[test]
fn should_see_own_uncommitted_writes_in_both_patterns() {
    // Arrange
    let world = world();
    let writer = &world.session;
    let reader = world.cassie.create_session("tester", None);
    let before = run(&world, writer, WITH_NEW_PERSON);
    exec(&world, writer, "BEGIN");

    // Act
    exec(&world, writer, ADD_PERSON);
    exec(&world, writer, ADD_EDGE);
    let own = run(&world, writer, WITH_NEW_PERSON);
    let other = run(&world, &reader, WITH_NEW_PERSON);
    exec(&world, writer, "COMMIT");
    let committed = run(&world, &reader, WITH_NEW_PERSON);

    // Assert
    assert_eq!(before, Vec::<String>::new());
    assert_eq!(own, strings(&["1", "1", "2", "6"]));
    assert_eq!(other, Vec::<String>::new());
    assert_eq!(committed, own);
}

#[test]
fn should_drop_a_rolled_back_savepoint_write_from_the_join() {
    // Arrange
    let world = world();
    let session = &world.session;
    exec(&world, session, "BEGIN");
    exec(&world, session, ADD_PERSON);

    // Act
    exec(&world, session, "SAVEPOINT edge");
    exec(&world, session, ADD_EDGE);
    let inside = run(&world, session, WITH_NEW_PERSON);
    exec(&world, session, "ROLLBACK TO SAVEPOINT edge");
    let rolled_back = run(&world, session, WITH_NEW_PERSON);

    // Assert
    assert_eq!(inside, strings(&["1", "1", "2", "6"]));
    assert_eq!(rolled_back, Vec::<String>::new());
}

#[test]
fn should_keep_a_repeatable_read_snapshot_across_a_concurrent_commit() {
    // Arrange
    let world = world();
    let reader = &world.session;
    let writer = world.cassie.create_session("tester", None);
    exec(&world, reader, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    let first = run(&world, reader, WITH_NEW_PERSON);

    // Act
    exec(&world, &writer, ADD_PERSON);
    exec(&world, &writer, ADD_EDGE);
    let snapshot = run(&world, reader, WITH_NEW_PERSON);
    exec(&world, reader, "COMMIT");
    let fresh = run(&world, reader, WITH_NEW_PERSON);

    // Assert
    assert_eq!(first, Vec::<String>::new());
    assert_eq!(snapshot, Vec::<String>::new());
    assert_eq!(fresh, strings(&["1", "1", "2", "6"]));
}

fn limited(budget: usize) -> SqlFixture {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = budget;
    limited_world(config)
}

fn peak(world: &SqlFixture) -> usize {
    let metrics = world.cassie.metrics();
    usize::try_from(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("peak"),
    )
    .expect("peak fits")
}

fn live_reservations(world: &SqlFixture) -> u64 {
    world.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted memory")
}

#[test]
fn should_fail_at_the_measured_peak_instead_of_returning_a_partial_join() {
    // Arrange
    let measured = limited(64 * 1024 * 1024);
    let rows = measured.rows(COMMON_NEIGHBORS).len();
    let needed = peak(&measured);
    let below = limited(needed - 1);
    let at = limited(needed);

    // Act
    let failure = below.execute(COMMON_NEIGHBORS);
    let admitted = at.rows(COMMON_NEIGHBORS).len();

    // Assert
    match failure {
        Ok(_) => panic!("a statement over its budget returned rows"),
        Err(error) => assert_eq!(error.sql_state(), "54000"),
    }
    assert_eq!((rows, admitted), (5, 5));
    assert_eq!(live_reservations(&below), 0);
}

fn cancel_at(boundary: QueryScoringTestBoundary) -> (SqlFixture, String) {
    let world = world();
    let mut cancellation = QueryCancellationHandle::new();
    let (selected, resume) = cancellation.pause_after_scored_candidate(boundary);

    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            world.cassie.execute_sql_with_cancellation(
                &world.session,
                COMMON_NEIGHBORS,
                vec![],
                &cancellation,
            )
        });
        selected
            .recv_timeout(Duration::from_secs(10))
            .expect("the statement reaches the boundary");
        cancellation.cancel();
        resume.send(()).expect("resume the statement");
        worker.join().expect("worker finishes")
    });

    let state = match outcome {
        Ok(_) => panic!("a canceled statement returns no rows"),
        Err(error) => error.sql_state().to_owned(),
    };
    (world, state)
}

#[test]
fn should_cancel_a_joined_statement_inside_the_first_pattern_search() {
    // Arrange
    let boundary = QueryScoringTestBoundary::GraphTraversal;

    // Act
    let (world, state) = cancel_at(boundary);

    // Assert
    assert_eq!(state, "57014");
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_cancel_a_joined_statement_while_extending_its_tuples() {
    // Arrange
    let boundary = QueryScoringTestBoundary::GraphJoin;

    // Act
    let (world, state) = cancel_at(boundary);

    // Assert
    assert_eq!(state, "57014");
    assert_eq!(live_reservations(&world), 0);
}
