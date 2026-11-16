//! Memory budget and cancellation of a graph traversal.

use crate::world::{link, node, Link, Node, World};
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::QueryCancellationHandle;

const BUDGET_BYTES: usize = 1024 * 1024;

fn complete(count: i64) -> (Vec<Node>, Vec<Link>) {
    let nodes = (1..=count).map(node).collect();
    let mut links = Vec::new();
    for from in 1..=count {
        for to in 1..=count {
            if from != to {
                let number = i64::try_from(links.len()).expect("few links") + 1;
                links.push(link(number, from, to));
            }
        }
    }
    (nodes, links)
}

fn limited(count: i64, budget: usize) -> World {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = budget;
    let (nodes, links) = complete(count);
    World::limited(config, &nodes, &links)
}

fn failure(world: &World, sql: &str) -> CassieError {
    match world.fixture.execute(sql) {
        Ok(_) => panic!("a statement that should fail succeeded"),
        Err(error) => error,
    }
}

#[test]
fn should_fail_with_the_resource_limit_when_the_budget_is_exhausted() {
    // Arrange
    let world = limited(6, BUDGET_BYTES);
    let control = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[IS link]-> (t IS node) \
                   COLUMNS (s.num AS s)) AS t";
    let heavy = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[IS link]->{1,7} (t IS node) \
                 COLUMNS (s.num AS s, t.num AS t)) AS t";

    // Act
    let rows = world.fixture.rows(control).len();
    let error = failure(&world, heavy);

    // Assert
    assert_eq!(rows, 30);
    assert_eq!(error.sql_state(), "54000");
}

fn peak(world: &World) -> usize {
    let metrics = world.fixture.cassie.metrics();
    usize::try_from(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("peak"),
    )
    .expect("peak fits")
}

#[test]
fn should_enforce_the_measured_peak_as_the_budget_boundary() {
    // Arrange
    let query = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[IS link]->{1,2} (t IS node) \
                 COLUMNS (s.num AS s, t.num AS t)) AS t";
    let measured = limited(6, 64 * 1024 * 1024);
    let measured_rows = measured.fixture.rows(query).len();
    let needed = peak(&measured);

    // Act
    let below = failure(&limited(6, needed - 1), query);
    let at = limited(6, needed).fixture.rows(query).len();

    // Assert
    assert_eq!(below.sql_state(), "54000");
    assert_eq!((measured_rows, at), (180, 180));
}

#[test]
fn should_cancel_a_traversal_at_an_inline_predicate_boundary() {
    // Arrange
    let (nodes, links) = complete(4);
    let world = World::simple(&nodes, &links);
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) \
               -[IS link WHERE pg_sleep(0) IS NOT NULL]->{1,3} (t IS node) \
               COLUMNS (s.num AS s, t.num AS t)) AS t";
    let (_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
    let cancellation = QueryCancellationHandle::new();

    // Act
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            world.fixture.cassie.execute_sql_with_cancellation(
                &world.fixture.session,
                sql,
                vec![],
                &cancellation,
            )
        });
        selected
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("traversal reaches its first predicate");
        cancellation.cancel();
        resume.send(()).expect("resume the traversal");
        worker.join().expect("worker finishes")
    });

    // Assert
    match outcome {
        Ok(_) => panic!("a canceled traversal returns no rows"),
        Err(error) => assert_eq!(error.sql_state(), "57014"),
    }
}

#[test]
fn should_account_projected_path_values_against_the_budget() {
    // Arrange
    let pattern = "(s IS node) -[IS link]->{1,2} (t IS node)";
    let path_query =
        format!("SELECT * FROM GRAPH_TABLE (g MATCH p = {pattern} COLUMNS (p AS path)) AS t");
    let scalar_query =
        format!("SELECT * FROM GRAPH_TABLE (g MATCH {pattern} COLUMNS (s.num AS s)) AS t");
    let measured = limited(6, 64 * 1024 * 1024);
    let path_rows = measured.fixture.rows(&path_query).len();
    let needed = peak(&measured);
    let scalar = limited(6, 64 * 1024 * 1024);
    let scalar_rows = scalar.fixture.rows(&scalar_query).len();

    // Act
    let below = failure(&limited(6, needed - 1), &path_query);
    let at = limited(6, needed).fixture.rows(&path_query).len();

    // Assert
    assert_eq!(scalar_rows, 180);
    assert!(needed > peak(&scalar));
    assert_eq!(below.sql_state(), "54000");
    assert_eq!((path_rows, at), (180, 180));
}
