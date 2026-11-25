//! Cancellation at the traversal loop itself, and release of query memory after failures.

use crate::world::{link, node, Link, Node, World};
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::{QueryCancellationHandle, QueryScoringTestBoundary};
use std::time::Duration;

const TRAVERSAL: &str = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) \
                         -[IS link]->{1,3} (t IS node) \
                         COLUMNS (s.num AS s, t.num AS t)) AS t";

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

fn live_reservations(world: &World) -> u64 {
    world.fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted memory")
}

fn state(error: &CassieError) -> String {
    error.sql_state().to_owned()
}

#[test]
fn should_cancel_a_traversal_paused_inside_the_search_loop() {
    // Arrange
    let (nodes, links) = complete(4);
    let world = World::simple(&nodes, &links);
    let mut cancellation = QueryCancellationHandle::new();
    let (selected, resume) =
        cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::GraphTraversal);

    // Act
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            world.fixture.cassie.execute_sql_with_cancellation(
                &world.fixture.session,
                TRAVERSAL,
                vec![],
                &cancellation,
            )
        });
        selected
            .recv_timeout(Duration::from_secs(10))
            .expect("the search loop reaches its first iteration");
        cancellation.cancel();
        resume.send(()).expect("resume the traversal");
        worker.join().expect("worker finishes")
    });

    // Assert
    let state = match outcome {
        Ok(_) => panic!("a canceled traversal returns no rows"),
        Err(error) => state(&error),
    };
    assert_eq!(state, "57014");
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_release_query_memory_after_a_budget_failure() {
    // Arrange
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    let (nodes, links) = complete(6);
    let world = World::limited(config, &nodes, &links);
    let heavy = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[IS link]->{1,7} (t IS node) \
                 COLUMNS (s.num AS s, t.num AS t)) AS t";

    // Act
    let outcome = world.fixture.execute(heavy);

    // Assert
    let state = match outcome {
        Ok(_) => panic!("a statement over its budget returned rows"),
        Err(error) => state(&error),
    };
    assert_eq!(state, "54000");
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_release_query_memory_after_a_pre_traversal_cancellation() {
    // Arrange
    let (nodes, links) = complete(4);
    let world = World::simple(&nodes, &links);
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let outcome = world.fixture.cassie.execute_sql_with_cancellation(
        &world.fixture.session,
        TRAVERSAL,
        vec![],
        &cancellation,
    );

    // Assert
    assert!(outcome.is_err());
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_cancel_a_selection_paused_before_its_final_sort() {
    // Arrange
    let (nodes, links) = complete(4);
    let world = World::simple(&nodes, &links);
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS node) -[IS link]->{1,3} (t IS node) \
               KEEP SHORTEST 2 PATHS COLUMNS (s.num AS s, t.num AS t)) AS t";
    let mut cancellation = QueryCancellationHandle::new();
    let (selected, resume) =
        cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::GraphSelection);

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
            .recv_timeout(Duration::from_secs(10))
            .expect("the selector reaches its final sort");
        cancellation.cancel();
        resume.send(()).expect("resume the selector");
        worker.join().expect("worker finishes")
    });

    // Assert
    let state = match outcome {
        Ok(_) => panic!("a canceled selection returns no rows"),
        Err(error) => state(&error),
    };
    assert_eq!(state, "57014");
    assert_eq!(live_reservations(&world), 0);
}
