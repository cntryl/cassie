//! Counted reads of `graph_weighted_paths` over the lazy indexed store: the validation scan
//! streams every selected edge row once, and the search reads only the neighbourhoods of
//! the vertices it visits (#1258, #1268). Reads are counted, never timed.

use crate::world::{fast, vertex, Link, Variant, VariantGuard, Wg};

const INDEXED: Variant = Variant {
    exact: false,
    endpoint_indexes: true,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    index_entries: u64,
    row_fetches: u64,
    full_rows: u64,
    validation_rows: u64,
}

fn counts(world: &Wg) -> Counts {
    let metrics = world.fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().expect("counter");
    Counts {
        index_entries: read("graph_index_entries"),
        row_fetches: read("graph_row_fetches"),
        full_rows: read("graph_full_rows"),
        validation_rows: read("graph_validation_rows"),
    }
}

/// The counted reads one call of `rest` over vertices `from` and `to` adds.
fn measure(world: &Wg, from: i64, to: i64, rest: &str) -> Counts {
    let before = counts(world);
    let outcome = world.call(from, to, rest);
    assert!(outcome.is_ok());
    let after = counts(world);
    Counts {
        index_entries: after.index_entries - before.index_entries,
        row_fetches: after.row_fetches - before.row_fetches,
        full_rows: after.full_rows - before.full_rows,
        validation_rows: after.validation_rows - before.validation_rows,
    }
}

/// Vertices `1..=3` joined by a chain and a ring of `ring` unrelated vertices from 10 on.
fn ringed(ring: i64) -> Wg {
    let _guard = VariantGuard::set(INDEXED);
    let mut links = vec![fast(1, 1, 2, 1), fast(2, 2, 3, 1)];
    for step in 0..ring {
        links.push(fast(3 + step, 10 + step, 10 + (step + 1) % ring, 1));
    }
    Wg::new(10 + ring, &links)
}

#[test]
fn should_not_read_unrelated_edges_given_endpoint_indexes() {
    // Arrange
    let small = ringed(3);
    let large = ringed(60);

    // Act
    let few = measure(&small, 1, 3, "'weight', 2");
    let many = measure(&large, 1, 3, "'weight', 2");

    // Assert
    assert_eq!(
        (few.index_entries, few.row_fetches),
        (many.index_entries, many.row_fetches)
    );
    assert_eq!((few.full_rows, many.full_rows), (0, 0));
}

#[test]
fn should_validate_every_selected_edge_row_without_reading_them_in_full() {
    // Arrange
    let world = ringed(60);

    // Act
    let reads = measure(&world, 1, 3, "'weight', 2");

    // Assert
    assert_eq!(reads.validation_rows, 62);
    assert_eq!(reads.full_rows, 0);
}

#[test]
fn should_read_further_neighbourhoods_only_when_the_hop_limit_allows() {
    // Arrange
    let _guard = VariantGuard::set(INDEXED);
    let links = (1..5)
        .map(|id| fast(id, id, id + 1, 1))
        .collect::<Vec<Link>>();
    let world = Wg::new(5, &links);

    // Act
    let one_hop = measure(&world, 1, 5, "'weight', 1");
    let four_hops = measure(&world, 1, 5, "'weight', 4");

    // Assert
    assert!(one_hop.index_entries < four_hops.index_entries);
    assert_eq!(one_hop.validation_rows, four_hops.validation_rows);
}

#[test]
fn should_keep_the_peak_query_memory_independent_of_unrelated_edges() {
    // Arrange
    let peak = |ring: i64| {
        let world = ringed(3);
        for step in 0..ring {
            world.run(&format!(
                "INSERT INTO fast VALUES ({}, {}, {}, 1, 'x', 1.5)",
                100 + step,
                10 + step % 3,
                10 + (step + 1) % 3
            ));
        }
        assert!(world.call(1, 3, "'weight', 2").is_ok());
        let metrics = world.fixture.cassie.metrics();
        metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("peak")
    };

    // Act
    let small = peak(10);
    let large = peak(1500);

    // Assert
    assert!(large < 2 * small);
}

#[test]
fn should_count_the_edge_rows_of_only_the_selected_labels_in_the_validation_scan() {
    // Arrange
    let world = ringed(5);
    let statement = Wg::select(&format!(
        "'wg', {}, {}, 'weight', 1, 'out', ARRAY['fast'], NULL, 1",
        vertex(1),
        vertex(2)
    ));
    let before = counts(&world).validation_rows;

    // Act
    let outcome = world.fixture.execute(&statement);

    // Assert
    assert!(outcome.is_ok());
    assert_eq!(counts(&world).validation_rows - before, 7);
}
