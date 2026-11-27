//! Deterministic cost curves of `graph_weighted_paths` (#1355). The validation scan is
//! O(selected edges) by contract and is counted on its own; the search is O(visited
//! neighbourhood). Each test fixes the queried neighbourhood and grows only what the
//! contract says may grow. Every value is a counter, never a timing.

use crate::world::{fast, started, vertex, Link, Variant, VariantGuard, Wg};
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

#[test]
fn should_reject_a_missing_not_null_weight_written_by_projection_replay() {
    // Arrange
    let fixture = started(
        None,
        &[
            "CREATE TABLE v (id BIGINT PRIMARY KEY)",
            "CREATE TABLE e (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, \
             weight INTEGER NOT NULL)",
            "CREATE INDEX e_weight ON e (weight)",
            "INSERT INTO v VALUES (1), (2), (3)",
            "INSERT INTO e VALUES (1, 1, 2, 1)",
            "CREATE PROPERTY GRAPH wg \
             VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id)) \
             EDGE TABLES (e AS s KEY (id) SOURCE KEY (a) REFERENCES n (id) \
               DESTINATION KEY (b) REFERENCES n (id) LABEL slow PROPERTIES (weight))",
        ],
    );
    let replay = fixture
        .cassie
        .replay_projection_batch(ProjectionReplayBatch {
            projection: fixture.cassie.midge.canonical_collection_name("e"),
            source_identity: "edges".to_owned(),
            batch_id: "missing-weight".to_owned(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "edge-2".to_owned(),
                checkpoint: "2".to_owned(),
                position: None,
                document_id: "edge-2".to_owned(),
                payload: Some(serde_json::json!({"id": 2, "a": 3, "b": 3})),
            }],
        });
    assert_eq!(
        replay.expect("replay writes the edge").applied_event_count,
        1
    );

    // Act
    let outcome = fixture.execute(&Wg::select(&format!(
        "'wg', {}, {}, 'weight', 1",
        vertex(1),
        vertex(2)
    )));

    // Assert
    assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22023"));
}
