//! Memory budget and cancellation of a weighted search.

use crate::world::{fast, vertex, Wg};
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::QueryCancellationHandle;

fn complete(count: i64) -> Wg {
    Wg::new(count, &links(count))
}

fn links(count: i64) -> Vec<crate::world::Link> {
    let mut links = Vec::new();
    let mut id = 0;
    for from in 1..=count {
        for to in 1..=count {
            if from != to {
                id += 1;
                links.push(fast(id, from, to, 1));
            }
        }
    }
    links
}

fn limited(count: i64, budget: usize) -> Wg {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = budget;
    Wg::with_config(count, &links(count), config)
}

fn peak(world: &Wg) -> usize {
    let metrics = world.fixture.cassie.metrics();
    usize::try_from(
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("peak"),
    )
    .expect("peak fits")
}

fn call(count: i64) -> String {
    Wg::select(&format!(
        "'wg', {}, {}, 'weight', {count}, 'out', NULL, NULL, 100000",
        vertex(1),
        vertex(count)
    ))
}

#[test]
fn should_enforce_the_measured_peak_as_the_budget_boundary() {
    // Arrange
    let statement = call(6);
    let measured = limited(6, 64 * 1024 * 1024);
    let rows = measured.fixture.rows(&statement).len();
    let needed = peak(&measured);

    // Act
    let below = limited(6, needed - 1).fixture.execute(&statement);
    let at = limited(6, needed).fixture.rows(&statement).len();

    // Assert
    assert_eq!(rows, 65);
    assert_eq!(at, 65);
    match below {
        Ok(_) => panic!("a statement over its budget returned rows"),
        Err(error) => assert_eq!(error.sql_state(), "54000"),
    }
}

#[test]
fn should_fail_with_the_resource_limit_when_the_budget_is_tiny() {
    // Arrange
    let world = limited(6, 4 * 1024);

    // Act
    let outcome = world.fixture.execute(&call(6));

    // Assert
    match outcome {
        Ok(_) => panic!("a statement over its budget returned rows"),
        Err(error) => assert_eq!(error.sql_state(), "54000"),
    }
}

#[test]
fn should_stop_a_canceled_search_with_57014() {
    // Arrange
    let world = complete(5);
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let outcome = world.fixture.cassie.execute_sql_with_cancellation(
        &world.fixture.session,
        &call(5),
        vec![],
        &cancellation,
    );

    // Assert
    match outcome {
        Ok(_) => panic!("a canceled search returns no rows"),
        Err(error) => assert_eq!(error.sql_state(), "57014"),
    }
}
