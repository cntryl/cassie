//! Small logical-plan/statistics fixtures shared by planner and cost observations.

use super::fixture_cassie::catalog::{CollectionCardinalityStats, FieldCardinalityStats};
use super::fixture_cassie::planner::logical::{self, LogicalPlan};
use super::fixture_cassie::sql::{binder::BoundStatement, parse_statement};
use std::collections::{BTreeMap, HashMap};

pub(super) type Stats = HashMap<String, CollectionCardinalityStats>;

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
