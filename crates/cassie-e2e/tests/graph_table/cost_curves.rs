//! Deterministic cost curves of the `GRAPH_TABLE` read families over degree, depth, skew and
//! disconnected growth (#1355). Every row of a curve is a counter value on a fixed fixture:
//! index entries, row fetches, full rows, neighborhoods expanded and edges admitted. Nothing
//! is timed and results are compared with the exact full read on the same data.

use crate::world::{ints, link, node, Link, Node, Variant, VariantGuard, World};

const INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Curve {
    index_entries: u64,
    row_fetches: u64,
    full_rows: u64,
    neighborhoods: u64,
    edges_admitted: u64,
}

fn counters(world: &World) -> Curve {
    let metrics = world.fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
    Curve {
        index_entries: read("graph_index_entries"),
        row_fetches: read("graph_row_fetches"),
        full_rows: read("graph_full_rows"),
        neighborhoods: read("graph_neighborhoods_expanded"),
        edges_admitted: read("graph_edges_admitted"),
    }
}

/// The rows and counter row of one statement over the lazy indexed store.
fn measure(world: &World, sql: &str) -> (Vec<Vec<i64>>, Curve) {
    let before = counters(world);
    let rows = match world
        .fixture
        .cassie
        .execute_sql(&world.fixture.session, sql, vec![])
    {
        Ok(result) => ints(result.rows().to_vec()),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    };
    let after = counters(world);
    (
        rows,
        Curve {
            index_entries: after.index_entries - before.index_entries,
            row_fetches: after.row_fetches - before.row_fetches,
            full_rows: after.full_rows - before.full_rows,
            neighborhoods: after.neighborhoods - before.neighborhoods,
            edges_admitted: after.edges_admitted - before.edges_admitted,
        },
    )
}

/// The same statement's rows under the forced exact read.
fn exact_rows(world: &World, sql: &str) -> Vec<Vec<i64>> {
    world.fixture.cassie.set_graph_exact_reads(true);
    let result = world
        .fixture
        .cassie
        .execute_sql(&world.fixture.session, sql, vec![])
        .expect("exact read");
    world.fixture.cassie.set_graph_exact_reads(false);
    ints(result.rows().to_vec())
}

fn from_one(hops: &str) -> String {
    format!(
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 1) -[IS link]->{hops} \
         (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b"
    )
}

/// Vertex 1 with `degree` out-neighbors, plus a ring of `extra` unreachable vertices.
fn star(degree: i64, extra: i64) -> World {
    let _guard = VariantGuard::set(INDEXED);
    let mut nodes = (1..=degree + 1).map(node).collect::<Vec<Node>>();
    let mut links = (1..=degree)
        .map(|n| link(n, 1, n + 1))
        .collect::<Vec<Link>>();
    for step in 0..extra {
        nodes.push(node(1000 + step));
        links.push(link(
            degree + 1 + step,
            1000 + step,
            1000 + (step + 1) % extra,
        ));
    }
    World::simple(&nodes, &links)
}

/// A path 1 -> 2 -> ... -> `length + 1`.
fn chain(length: i64) -> World {
    let _guard = VariantGuard::set(INDEXED);
    let nodes = (1..=length + 1).map(node).collect::<Vec<Node>>();
    let links = (1..=length)
        .map(|n| link(n, n, n + 1))
        .collect::<Vec<Link>>();
    World::simple(&nodes, &links)
}

#[test]
fn should_admit_one_edge_per_incident_edge_given_a_degree_curve() {
    // Arrange
    let degrees = [2_i64, 4, 8, 16];

    // Act
    let curve = degrees.map(|degree| measure(&star(degree, 20), &from_one("{1,1}")).1);

    // Assert
    for (degree, row) in degrees.iter().zip(curve) {
        let degree = u64::try_from(*degree).expect("positive");
        assert_eq!(row.edges_admitted, degree);
        assert_eq!(row.neighborhoods, 1);
        assert_eq!(row.full_rows, 0);
    }
}

#[test]
fn should_grow_index_entries_linearly_with_degree_given_a_degree_curve() {
    // Arrange
    let degrees = [2_i64, 4, 8, 16];

    // Act
    let curve = degrees.map(|degree| measure(&star(degree, 20), &from_one("{1,1}")).1);

    // Assert
    for (degree, row) in degrees.iter().zip(curve) {
        let degree = u64::try_from(*degree).expect("positive");
        // One start vertex, `degree` incident edges and `degree` far vertices.
        assert_eq!(row.index_entries, 1 + 2 * degree);
        assert_eq!(row.row_fetches, 1 + 2 * degree);
    }
}

#[test]
fn should_expand_one_neighborhood_per_hop_given_a_depth_curve() {
    // Arrange
    let world = chain(8);

    // Act
    let curve =
        [1_u64, 2, 4, 6].map(|depth| measure(&world, &from_one(&format!("{{1,{depth}}}"))).1);

    // Assert
    for (depth, row) in [1_u64, 2, 4, 6].iter().zip(curve) {
        assert_eq!(row.neighborhoods, *depth);
        assert_eq!(row.edges_admitted, *depth);
        assert_eq!(row.full_rows, 0);
    }
}

#[test]
fn should_leave_every_counter_unchanged_given_ten_times_the_disconnected_edges() {
    // Arrange
    let small = star(4, 20);
    let large = star(4, 200);

    // Act
    let (small_rows, few) = measure(&small, &from_one("{1,2}"));
    let (large_rows, many) = measure(&large, &from_one("{1,2}"));

    // Assert
    assert_eq!(small_rows, large_rows);
    assert_eq!(few, many);
}

#[test]
fn should_return_the_exact_rows_given_a_depth_curve() {
    // Arrange
    let world = chain(8);
    let sql = from_one("{1,6}");

    // Act
    let (lazy, _) = measure(&world, &sql);
    let exact = exact_rows(&world, &sql);

    // Assert
    assert_eq!(lazy, exact);
}

/// A hub (vertex 1) with `fan` spokes; spoke `n + 1` also links to a private leaf.
fn hub(fan: i64) -> World {
    let _guard = VariantGuard::set(INDEXED);
    let mut nodes = (1..=fan + 1).map(node).collect::<Vec<Node>>();
    let mut links = (1..=fan).map(|n| link(n, 1, n + 1)).collect::<Vec<Link>>();
    nodes.push(node(5000));
    links.push(link(fan + 1, 2, 5000));
    World::simple(&nodes, &links)
}

fn from_two(hops: &str) -> String {
    format!(
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS node WHERE a.num = 2) -[IS link]->{hops} \
         (b IS node) COLUMNS (b.num AS b)) AS t ORDER BY b"
    )
}

#[test]
fn should_not_touch_the_hub_given_a_leaf_anchored_pattern_pointing_away_from_it() {
    // Arrange
    let narrow = hub(4);
    let wide = hub(64);

    // Act
    let (narrow_rows, few) = measure(&narrow, &from_two("{1,1}"));
    let (wide_rows, many) = measure(&wide, &from_two("{1,1}"));

    // Assert
    assert_eq!(narrow_rows, vec![vec![5000]]);
    assert_eq!(narrow_rows, wide_rows);
    assert_eq!(few, many);
    assert_eq!(few.edges_admitted, 1);
}

#[test]
fn should_pay_the_hub_degree_once_given_a_pattern_anchored_at_the_hub() {
    // Arrange
    let narrow = hub(4);
    let wide = hub(64);

    // Act
    let (_, few) = measure(&narrow, &from_one("{1,1}"));
    let (_, many) = measure(&wide, &from_one("{1,1}"));

    // Assert
    assert_eq!(few.edges_admitted, 4);
    assert_eq!(many.edges_admitted, 64);
    assert_eq!(few.neighborhoods, many.neighborhoods);
}
