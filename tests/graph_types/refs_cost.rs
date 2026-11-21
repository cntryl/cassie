//! The read cost of the `cassie_graph` reference constructors (#1355): a reference reads the
//! one index entry and row its key names, not the mapped table. Reads are counted through
//! the storage layer's graph-reference read counter, never timed.

use crate::fixture::{started, GraphFixture};

fn people(count: i64) -> GraphFixture {
    let values = (1..=count)
        .map(|id| format!("({id}, 'p{id}')"))
        .collect::<Vec<_>>()
        .join(", ");
    started(
        "graph_refs_cost",
        &[
            "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
            "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)",
            &format!("INSERT INTO people VALUES {values}"),
            &format!("INSERT INTO follows VALUES (1, {}, {count})", count - 1),
            "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name)) \
             EDGE TABLES (follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) \
             DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))",
        ],
    )
}

fn scan_entries(fixture: &GraphFixture) -> u64 {
    fixture.cassie.midge.graph_reference_reads_for_diagnostics()
}

/// Storage scan entries one evaluation of `sql` adds.
fn entries_of(fixture: &GraphFixture, sql: &str) -> u64 {
    let before = scan_entries(fixture);
    let _ = fixture.rows(sql);
    scan_entries(fixture) - before
}

/// A reference to the last vertex of a table of `count` rows: a scan reads every row first.
fn vertex_of(count: i64) -> String {
    format!(
        "SELECT (cassie_graph.vertex_ref('social', 'person', \
         ARRAY[cassie_graph.key_part({count}::bigint)])).element_kind"
    )
}

const EDGE: &str = "SELECT (cassie_graph.edge_ref('social', 'knows', \
                    ARRAY[cassie_graph.key_part(1::bigint)])).element_kind";

#[test]
fn should_read_the_same_entries_for_a_vertex_reference_when_the_table_grows() {
    // Arrange
    let small = people(20);
    let large = people(400);

    // Act
    let few = entries_of(&small, &vertex_of(20));
    let many = entries_of(&large, &vertex_of(400));

    // Assert
    assert_eq!(few, many);
}

#[test]
fn should_read_two_rows_by_key_for_a_vertex_reference() {
    // Arrange
    let fixture = people(400);

    // Act
    let reads = entries_of(&fixture, &vertex_of(400));

    // Assert
    assert_eq!(reads, 2);
}

#[test]
fn should_read_three_rows_by_key_for_an_edge_reference() {
    // Arrange
    let fixture = people(400);

    // Act
    let reads = entries_of(&fixture, EDGE);

    // Assert
    assert_eq!(reads, 6);
}

#[test]
fn should_read_the_same_entries_for_an_edge_reference_when_the_vertex_table_grows() {
    // Arrange
    let small = people(20);
    let large = people(400);

    // Act
    let few = entries_of(&small, EDGE);
    let many = entries_of(&large, EDGE);

    // Assert
    assert_eq!(few, many);
}
