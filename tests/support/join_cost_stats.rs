//! Small logical-plan/statistics fixtures shared by planner and cost observations.

use super::fixture_cassie::catalog::{CollectionCardinalityStats, FieldCardinalityStats};
use super::fixture_cassie::planner::logical::{self, LogicalPlan};
use super::fixture_cassie::sql::{binder::BoundStatement, parse_statement};
use std::collections::{BTreeMap, HashMap};

pub(super) type Stats = HashMap<String, CollectionCardinalityStats>;

pub(super) const CORRELATED_ON_SQL: &str = "SELECT probe_b_a.id,probe_c_b.id,probe_a_c.id FROM probe_b_a JOIN probe_c_b ON probe_b_a.id=probe_c_b.a_id JOIN probe_a_c ON probe_b_a.id=probe_a_c.id AND EXISTS(SELECT 1 FROM probe_lookup WHERE probe_lookup.c_id=probe_a_c.id AND probe_lookup.b_id=probe_c_b.id) ORDER BY probe_c_b.id";

pub(super) fn plan(sql: &str) -> LogicalPlan {
    logical::plan(&BoundStatement {
        statement: parse_statement(sql).expect("synthetic planner SQL"),
        indexes: Vec::new(),
    })
    .expect("logical planner fixture")
}

pub(super) fn collection(rows: u64, fields: &[(&str, u64)]) -> CollectionCardinalityStats {
    CollectionCardinalityStats {
        row_count: rows,
        hydrated: true,
        fields: fields
            .iter()
            .map(|(name, distinct)| {
                (
                    (*name).to_string(),
                    FieldCardinalityStats {
                        non_null_count: rows,
                        distinct_count: *distinct,
                        sample_count: rows,
                        confidence: 100,
                        ..FieldCardinalityStats::default()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        ..CollectionCardinalityStats::default()
    }
}

pub(super) fn star_stats(distinct: bool) -> Stats {
    let ndvs = if distinct { [10, 100, 1_000] } else { [1; 3] };
    HashMap::from([
        ("a".to_string(), collection(10, &[("id", ndvs[0])])),
        ("b".to_string(), collection(100, &[("id", ndvs[1])])),
        (
            "c".to_string(),
            collection(1_000, &[("ak", ndvs[2]), ("bk", ndvs[2])]),
        ),
    ])
}

pub(super) fn correlated_on_stats() -> Stats {
    HashMap::from([
        ("probe_b_a".to_string(), collection(2, &[("id", 2)])),
        (
            "probe_c_b".to_string(),
            collection(3, &[("id", 3), ("a_id", 2)]),
        ),
        ("probe_a_c".to_string(), collection(1, &[("id", 1)])),
    ])
}
