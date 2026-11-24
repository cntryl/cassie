//! `pg_class.relam` identity per relation kind (#817). Expected values come
//! from a PostgreSQL 18.6 oracle: heap tables and materialized views report
//! the `heap` access method, B-tree indexes report `btree`, and views,
//! sequences and composite types report no access method.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA am_identity",
    "CREATE TYPE am_identity.pair AS (left_side int, right_side text)",
    "CREATE TABLE am_identity.events (id INT PRIMARY KEY, label TEXT)",
    "CREATE INDEX am_events_label_idx ON am_identity.events (label)",
    "CREATE VIEW am_identity.event_labels AS SELECT label FROM am_identity.events",
    "CREATE SEQUENCE am_identity.event_ids",
    "CREATE MATERIALIZED VIEW am_identity.event_totals AS SELECT 1 AS total",
];

fn relam_of(fixture: &SqlFixture, relation: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT relam::bigint FROM pg_class WHERE oid = '{relation}'::regclass"
    ))
}

#[test]
fn should_report_the_heap_access_method_for_a_table() {
    // Arrange
    let fixture = sql_fixture("relam_table", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.events");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_report_the_heap_access_method_for_a_materialized_view() {
    // Arrange
    let fixture = sql_fixture("relam_materialized_view", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.event_totals");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_report_the_btree_access_method_for_an_index() {
    // Arrange
    let fixture = sql_fixture("relam_index", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.am_events_label_idx");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(403)]]);
}

#[test]
fn should_report_no_access_method_for_a_view() {
    // Arrange
    let fixture = sql_fixture("relam_view", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.event_labels");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_no_access_method_for_a_sequence() {
    // Arrange
    let fixture = sql_fixture("relam_sequence", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.event_ids");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_no_access_method_for_a_composite_type() {
    // Arrange
    let fixture = sql_fixture("relam_composite", SETUP);

    // Act
    let rows = relam_of(&fixture, "am_identity.pair");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_resolve_the_btree_access_method_row_of_an_index() {
    // Arrange
    let fixture = sql_fixture("relam_access_method_row", SETUP);

    // Act
    let rows = fixture.rows(
        "SELECT access_method.amname FROM pg_class AS relation \
         JOIN pg_am AS access_method ON access_method.oid = relation.relam \
         WHERE relation.oid = 'am_identity.am_events_label_idx'::regclass",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::String("btree".to_string())]]);
}
