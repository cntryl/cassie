//! Memory budget and cancellation of a weighted search.

use crate::world::{fast, slow, vertex, Wg};
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::{QueryCancellationHandle, QueryScoringTestBoundary};

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

fn live_reservations(world: &Wg) -> u64 {
    world.fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted memory")
}

#[test]
fn should_cancel_a_search_paused_inside_the_search_loop() {
    // Arrange
    let world = complete(5);
    let mut cancellation = QueryCancellationHandle::new();
    let (selected, resume) =
        cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::GraphWeightedSearch);
    let statement = call(5);

    // Act
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            world.fixture.cassie.execute_sql_with_cancellation(
                &world.fixture.session,
                &statement,
                vec![],
                &cancellation,
            )
        });
        selected
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the search loop reaches its first iteration");
        cancellation.cancel();
        resume.send(()).expect("resume the search");
        worker.join().expect("worker finishes")
    });

    // Assert
    let state = match outcome {
        Ok(_) => panic!("a canceled search returns no rows"),
        Err(error) => error.sql_state().to_owned(),
    };
    assert_eq!(state, "57014");
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_release_query_memory_after_a_budget_failure() {
    // Arrange
    let world = limited(6, 4 * 1024);

    // Act
    let outcome = world.fixture.execute(&call(6));

    // Assert
    let state = match outcome {
        Ok(_) => panic!("a statement over its budget returned rows"),
        Err(error) => error.sql_state().to_owned(),
    };
    assert_eq!(state, "54000");
    assert_eq!(live_reservations(&world), 0);
}

#[test]
fn should_release_query_memory_after_a_pre_search_cancellation() {
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
    assert!(outcome.is_err());
    assert_eq!(live_reservations(&world), 0);
}

/// Edge rows the weight validation scan of one call read, from the read-path metrics.
fn validated(world: &Wg, rest: &str) -> u64 {
    let counter = || {
        world.fixture.cassie.metrics()["read_paths"]["graph_validation_rows"]
            .as_u64()
            .expect("validation counter")
    };
    let before = counter();
    let statement = Wg::select(&format!("'wg', {}, {}, {rest}", vertex(1), vertex(2)));
    let outcome = world.fixture.execute(&statement);
    assert!(outcome.is_ok());
    counter() - before
}

#[test]
fn should_count_every_selected_edge_row_in_the_weight_validation_scan() {
    // Arrange
    let world = Wg::new(
        3,
        &[
            fast(1, 1, 2, 1),
            fast(2, 2, 3, 1),
            fast(3, 1, 3, 1),
            slow(1, 3, 1, 1),
            slow(2, 2, 1, 1),
        ],
    );

    // Act
    let everything = validated(&world, "'weight', 1, 'out', NULL, NULL, 1");
    let fast_only = validated(&world, "'weight', 1, 'out', ARRAY['fast'], NULL, 1");

    // Assert
    assert_eq!((everything, fast_only), (5, 3));
}
