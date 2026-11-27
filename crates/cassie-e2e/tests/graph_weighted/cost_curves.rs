//! Deterministic cost curves of `graph_weighted_paths` (#1355). The validation scan is
//! counted separately from the search. Each test fixes the queried neighbourhood and grows only what the
//! contract says may grow. Every value is a counter, never a timing.

use crate::world::{fast, started, vertex, Fixture, Link, Variant, VariantGuard, Wg};
use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};

const INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Curve {
    index_entries: u64,
    row_fetches: u64,
    full_rows: u64,
    validation_rows: u64,
    neighborhoods: u64,
    edges_admitted: u64,
}

fn counters(world: &Wg) -> Curve {
    let metrics = world.fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
    Curve {
        index_entries: read("graph_index_entries"),
        row_fetches: read("graph_row_fetches"),
        full_rows: read("graph_full_rows"),
        validation_rows: read("graph_validation_rows"),
        neighborhoods: read("graph_neighborhoods_expanded"),
        edges_admitted: read("graph_edges_admitted"),
    }
}

/// The counter row of one call from vertex 1 to vertex 3 within `hops` hops.
fn measure(world: &Wg, hops: u32) -> Curve {
    let before = counters(world);
    let outcome = world.call(1, 3, &format!("'weight', {hops}"));
    assert!(outcome.is_ok());
    let after = counters(world);
    Curve {
        index_entries: after.index_entries - before.index_entries,
        row_fetches: after.row_fetches - before.row_fetches,
        full_rows: after.full_rows - before.full_rows,
        validation_rows: after.validation_rows - before.validation_rows,
        neighborhoods: after.neighborhoods - before.neighborhoods,
        edges_admitted: after.edges_admitted - before.edges_admitted,
    }
}

/// The chain 1 -> 2 -> 3 and `ring` disconnected edges among vertices from 10 on.
fn ringed(ring: i64) -> Wg {
    let _guard = VariantGuard::set(INDEXED);
    let mut links = vec![fast(1, 1, 2, 1), fast(2, 2, 3, 1)];
    for step in 0..ring {
        links.push(fast(3 + step, 10 + step, 10 + (step + 1) % ring, 1));
    }
    Wg::new(10 + ring, &links)
}

#[test]
fn should_validate_every_selected_edge_given_ten_times_the_disconnected_edges() {
    // Arrange
    let small = ringed(10);
    let large = ringed(100);

    // Act
    let few = measure(&small, 2);
    let many = measure(&large, 2);

    // Assert
    assert_eq!((few.validation_rows, many.validation_rows), (12, 102));
}

#[test]
fn should_leave_the_search_counters_unchanged_given_ten_times_the_disconnected_edges() {
    // Arrange
    let small = ringed(10);
    let large = ringed(100);

    // Act
    let few = measure(&small, 2);
    let many = measure(&large, 2);

    // Assert
    assert_eq!(
        Curve {
            validation_rows: 0,
            ..few
        },
        Curve {
            validation_rows: 0,
            ..many
        }
    );
    assert_eq!((few.full_rows, many.full_rows), (0, 0));
}

#[test]
fn should_search_only_the_visited_neighbourhood_given_a_depth_curve() {
    // Arrange
    let _guard = VariantGuard::set(INDEXED);
    let links = (1..=6)
        .map(|id| fast(id, id, id + 1, 1))
        .collect::<Vec<Link>>();
    let world = Wg::new(7, &links);

    // Act
    let curve = [1_u32, 2, 3].map(|hops| measure(&world, hops));

    // Assert
    assert!(curve[0].neighborhoods <= curve[1].neighborhoods);
    assert!(curve[1].neighborhoods <= curve[2].neighborhoods);
    assert_eq!(curve.map(|row| row.validation_rows), [6, 6, 6]);
}

#[test]
fn should_reject_an_invalid_weight_outside_the_visited_neighbourhood() {
    // Arrange
    let world = ringed(10);
    world.run(&format!("UPDATE fast SET weight = -1 WHERE id = {}", 3 + 4));

    // Act
    let outcome = world.call(1, 3, "'weight', 2");

    // Assert
    assert_eq!(outcome.err().as_deref(), Some("22023"));
}

const INVALID_WEIGHTS: &str = "CREATE INDEX e_weight ON e ((1)) WHERE weight IS NULL OR weight < 0";
const NEGATIVE_ONLY: &str = "CREATE INDEX e_weight ON e ((1)) WHERE weight < 0";

fn signed_fixture(index: &str, disconnected_edges: &str) -> Fixture {
    signed_fixture_with_config(None, index, disconnected_edges)
}

fn signed_fixture_with_config(
    config: Option<cassie::config::CassieRuntimeConfig>,
    index: &str,
    disconnected_edges: &str,
) -> Fixture {
    started(
        config,
        &[
            "CREATE TABLE v (id BIGINT PRIMARY KEY)",
            "CREATE TABLE e (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, \
             weight BIGINT NOT NULL, unrelated JSONB)",
            "CREATE INDEX e_a ON e (a)",
            index,
            "INSERT INTO v VALUES (1), (2), (3), (4)",
            "INSERT INTO e (id, a, b, weight) VALUES (1, 1, 2, 0)",
            disconnected_edges,
            "CREATE PROPERTY GRAPH wg \
             VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id)) \
             EDGE TABLES (e AS s KEY (id) SOURCE KEY (a) REFERENCES n (id) \
               DESTINATION KEY (b) REFERENCES n (id) LABEL slow PROPERTIES (weight))",
        ],
    )
}

fn signed_query() -> String {
    Wg::select(&format!("'wg', {}, {}, 'weight', 1", vertex(1), vertex(2)))
}

#[test]
fn should_stop_integer_search_at_the_target_given_more_hops() {
    // Arrange
    let fixture = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 2, 3, 0), (3, 2, 4, 1), (4, 3, 4, 1)",
    );
    let bounded = Wg::select(&format!("'wg', {}, {}, 'weight', 4", vertex(1), vertex(2)));
    let expected = fixture.rows(&signed_query());
    let neighborhoods = || {
        fixture.cassie.metrics()["read_paths"]["graph_neighborhoods_expanded"]
            .as_u64()
            .expect("expansion counter")
    };
    let before = neighborhoods();

    // Act
    let actual = fixture.rows(&bounded);
    let expansions = neighborhoods() - before;

    // Assert
    assert_eq!(actual.len(), 1);
    assert_eq!(actual, expected);
    assert_eq!(expansions, 1);
}

#[test]
fn should_reject_an_invalid_integer_weight_beyond_the_target() {
    // Arrange
    let fixture = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 2, 3, -1)",
    );

    // Act
    let result = fixture.execute(&signed_query());

    // Assert
    assert_eq!(result.err().map(|error| error.sql_state()), Some("22023"));
}

#[test]
fn should_preserve_numeric_overflow_beyond_the_target() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), fast(2, 2, 3, 1)]);
    world.run("UPDATE fast SET weight = 9 * (10::numeric ^ 131071)");
    let sql = Wg::select(&format!("'wg', {}, {}, 'weight', 2", vertex(1), vertex(2)));

    // Act
    let result = world.fixture.execute(&sql);

    // Assert
    assert_eq!(result.err().map(|error| error.sql_state()), Some("22003"));
}

fn validation_counts(fixture: &Fixture) -> (u64, u64, u64) {
    let metrics = fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
    (
        read("graph_validation_rows"),
        read("graph_weight_validation_index_probes"),
        read("graph_weight_validation_full_scans"),
    )
}

fn replay_weights(fixture: &Fixture, payload: serde_json::Value) {
    let replay = fixture
        .cassie
        .replay_projection_batch(ProjectionReplayBatch {
            projection: fixture.cassie.midge.canonical_collection_name("e"),
            source_identity: "edges".to_owned(),
            batch_id: "invalid-weight".to_owned(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "edge-3".to_owned(),
                checkpoint: "3".to_owned(),
                position: None,
                document_id: "edge-3".to_owned(),
                payload: Some(payload),
            }],
        });
    assert_eq!(
        replay.expect("replay writes the edge").applied_event_count,
        1
    );
}

#[test]
fn should_reject_a_missing_not_null_weight_written_by_projection_replay() {
    for index in ["CREATE INDEX e_weight ON e (weight)", INVALID_WEIGHTS] {
        // Arrange
        let fixture = signed_fixture(
            index,
            "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 1)",
        );
        replay_weights(&fixture, serde_json::json!({"id": 3, "a": 4, "b": 3}));

        // Act
        let outcome = fixture.execute(&signed_query());

        // Assert
        assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22023"));
    }
}

#[test]
fn should_reject_an_explicit_null_weight_written_by_projection_replay() {
    // Arrange
    let fixture = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 1)",
    );
    replay_weights(
        &fixture,
        serde_json::json!({"id": 3, "a": 4, "b": 3, "weight": null}),
    );

    // Act
    let outcome = fixture.execute(&signed_query());

    // Assert
    assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22023"));
}

#[test]
fn should_skip_nonnegative_disconnected_rows_given_a_complete_invalid_weight_index() {
    // Arrange
    let small = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 9223372036854775807)",
    );
    let large = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 9223372036854775807), (3, 4, 3, 0), (4, 3, 4, 1)",
    );
    let query = signed_query();

    // Act
    let indexed = [&small, &large].map(|fixture| fixture.rows(&query));
    let counts = [&small, &large].map(validation_counts);
    let exact = [&small, &large].map(|fixture| {
        fixture.cassie.set_graph_exact_reads(true);
        fixture.rows(&query)
    });

    // Assert
    assert_eq!(exact.each_ref().map(Vec::len), [1, 1]);
    assert_eq!(indexed, exact);
    assert_eq!(counts, [(0, 1, 0), (0, 1, 0)]);
}

#[test]
fn should_reject_a_disconnected_edge_after_its_weight_enters_the_index() {
    for negative in ["-1", "CAST('-9223372036854775808' AS BIGINT)"] {
        // Arrange
        let fixture = signed_fixture(
            INVALID_WEIGHTS,
            "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 0)",
        );

        // Act
        fixture
            .execute(&format!("UPDATE e SET weight = {negative} WHERE id = 2"))
            .expect("update the disconnected weight");
        let outcome = fixture.execute(&signed_query());

        // Assert
        assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22023"));
    }
}

#[test]
fn should_read_pending_weights_when_the_committed_index_has_no_candidate() {
    // Arrange
    let fixture = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 0)",
    );
    let query = signed_query();
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("UPDATE e SET weight = -1 WHERE id = 2")
        .expect("stage negative weight");

    // Act
    let outcome = fixture.execute(&query);
    fixture.execute("ROLLBACK").expect("rollback");
    let committed = fixture.rows(&query);

    // Assert
    assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22023"));
    assert_eq!(committed.len(), 1);
}

#[test]
fn should_fall_back_after_the_invalid_weight_index_is_replaced_by_an_incomplete_one() {
    // Arrange
    let fixture = signed_fixture(
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 0)",
    );
    fixture
        .execute("DROP INDEX e_weight")
        .expect("drop the complete index");
    fixture
        .execute(NEGATIVE_ONLY)
        .expect("publish different predicate");

    // Act
    let _ = fixture.rows(&signed_query());
    let counts = validation_counts(&fixture);

    // Assert
    assert_eq!(counts, (2, 0, 1));
}

#[test]
fn should_validate_a_dangling_candidate_within_budget_given_a_wide_unrelated_property() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 256 * 1024;
    config.limits.parallel_scan_workers = 1;
    let fixture = signed_fixture_with_config(
        Some(config),
        INVALID_WEIGHTS,
        "INSERT INTO e (id, a, b, weight) VALUES (2, 3, 4, 0)",
    );
    replay_weights(
        &fixture,
        serde_json::json!({"id": 3, "a": 4, "b": 5, "weight": -1, "unrelated": "x".repeat(64 * 1024)}),
    );
    let query = signed_query();

    // Act
    let indexed = fixture.rows(&query);
    let indexed_counts = validation_counts(&fixture);
    fixture
        .execute("BEGIN")
        .expect("begin explicit transaction");
    let scanned = fixture.rows(&query);
    let total_counts = validation_counts(&fixture);
    fixture.execute("ROLLBACK").expect("rollback");

    // Assert
    assert_eq!(scanned.len(), 1);
    assert_eq!(indexed, scanned);
    assert_eq!(indexed_counts, (1, 1, 0));
    assert_eq!(total_counts, (4, 1, 1));
}

#[test]
fn should_reject_disconnected_negative_weight_given_a_quoted_source_column() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::default();
    config.limits.parallel_scan_workers = 2;
    let fixture = started(
        Some(config),
        &[
            "CREATE TABLE v (id BIGINT PRIMARY KEY)",
            r#"CREATE TABLE e (id BIGINT PRIMARY KEY, "Src" BIGINT NOT NULL, b BIGINT NOT NULL,
               weight BIGINT NOT NULL)"#,
            INVALID_WEIGHTS,
            "INSERT INTO v VALUES (1), (2), (3), (4)",
            "INSERT INTO e VALUES (1, 1, 2, 0), (2, 3, 4, -1)",
            r#"CREATE PROPERTY GRAPH wg
               VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id))
               EDGE TABLES (e AS s KEY (id) SOURCE KEY ("Src") REFERENCES n (id)
                 DESTINATION KEY (b) REFERENCES n (id) LABEL slow PROPERTIES (weight))"#,
        ],
    );
    let query = signed_query();

    // Act
    let indexed = fixture.execute(&query);
    fixture
        .execute("DROP INDEX e_weight")
        .expect("use full validation");
    let scanned = fixture.execute(&query);

    // Assert
    assert_eq!(scanned.err().map(|error| error.sql_state()), Some("22023"));
    assert_eq!(indexed.err().map(|error| error.sql_state()), Some("22023"));
}
