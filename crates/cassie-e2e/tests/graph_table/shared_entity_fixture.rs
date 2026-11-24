//! Tenant-local composite-key people, documents and edges for the shared-entity tests (#1353).
//!
//! Two tenants reuse the same printed ids and names, so a join on a display value alone
//! merges them while a join on the typed vertex identity does not.

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config, SqlFixture};
use cassie::types::Value;

const SCHEMA: [&str; 6] = [
    "CREATE TABLE person (tenant TEXT NOT NULL, id BIGINT NOT NULL, name TEXT NOT NULL, \
     PRIMARY KEY (tenant, id))",
    "CREATE TABLE document (tenant TEXT NOT NULL, id BIGINT NOT NULL, title TEXT NOT NULL, \
     PRIMARY KEY (tenant, id))",
    "CREATE TABLE knows (id BIGINT PRIMARY KEY, src_tenant TEXT NOT NULL, src_id BIGINT NOT NULL, \
     dst_tenant TEXT NOT NULL, dst_id BIGINT NOT NULL)",
    "CREATE TABLE owns (id BIGINT PRIMARY KEY, person_tenant TEXT NOT NULL, \
     person_id BIGINT NOT NULL, doc_tenant TEXT NOT NULL, doc_id BIGINT NOT NULL)",
    "CREATE INDEX knows_src ON knows (src_tenant, src_id)",
    "CREATE INDEX knows_dst ON knows (dst_tenant, dst_id)",
];

const INDEXES: [&str; 2] = [
    "CREATE INDEX owns_person ON owns (person_tenant, person_id)",
    "CREATE INDEX owns_doc ON owns (doc_tenant, doc_id)",
];

/// Tenant `a`: people 1..5, parallel edges 1 -> 3 (ids 1 and 5) and a cycle 5 -> 1. Tenant `b`
/// reuses ids 1..4 and names, with its own 1 -> 3 and 2 -> 3.
const DATA: [&str; 5] = [
    "INSERT INTO person VALUES ('a', 1, 'ann'), ('a', 2, 'bob'), ('a', 3, 'cat'), \
     ('a', 4, 'dee'), ('a', 5, 'eve'), ('b', 1, 'ann'), ('b', 2, 'bob'), ('b', 3, 'cat'), \
     ('b', 4, 'dee')",
    "INSERT INTO document VALUES ('a', 1, 'spec'), ('a', 2, 'plan'), ('b', 1, 'spec'), \
     ('b', 2, 'plan')",
    "INSERT INTO knows VALUES (1, 'a', 1, 'a', 3), (2, 'a', 2, 'a', 3), (3, 'a', 1, 'a', 4), \
     (4, 'a', 2, 'a', 4), (5, 'a', 1, 'a', 3), (6, 'a', 3, 'a', 5), (7, 'a', 4, 'a', 5), \
     (8, 'a', 5, 'a', 1)",
    "INSERT INTO knows VALUES (101, 'b', 1, 'b', 3), (102, 'b', 2, 'b', 3), (103, 'b', 1, 'b', 2)",
    "INSERT INTO owns VALUES (21, 'a', 3, 'a', 1), (22, 'a', 5, 'a', 2), (23, 'b', 3, 'b', 1), \
     (24, 'b', 2, 'b', 2)",
];

const GRAPH: &str = "CREATE PROPERTY GRAPH social \
     VERTEX TABLES (\
       person AS p KEY (tenant, id) LABEL person PROPERTIES (tenant, id, name), \
       document AS d KEY (tenant, id) LABEL document PROPERTIES (tenant, id, title)) \
     EDGE TABLES (\
       knows AS k KEY (id) \
         SOURCE KEY (src_tenant, src_id) REFERENCES p (tenant, id) \
         DESTINATION KEY (dst_tenant, dst_id) REFERENCES p (tenant, id) \
         LABEL knows PROPERTIES (id), \
       owns AS o KEY (id) \
         SOURCE KEY (person_tenant, person_id) REFERENCES p (tenant, id) \
         DESTINATION KEY (doc_tenant, doc_id) REFERENCES d (tenant, id) \
         LABEL owns PROPERTIES (id))";

fn statements(indexed: bool) -> Vec<&'static str> {
    let mut all = SCHEMA.to_vec();
    if indexed {
        all.extend(INDEXES);
    }
    all.extend(DATA);
    all.push(GRAPH);
    all
}

/// The setup statements of the indexed social graph for the pgwire fixtures.
pub fn setup() -> Vec<(&'static str, Vec<Value>)> {
    statements(true)
        .into_iter()
        .map(|sql| (sql, Vec::new()))
        .collect()
}

/// The social graph with endpoint indexes on every edge table.
pub fn world() -> SqlFixture {
    sql_fixture("graph-shared-entity", &statements(true))
}

/// The social graph where the `owns` edge table has no endpoint index, so its mapping is
/// read exactly.
pub fn unindexed_world() -> SqlFixture {
    sql_fixture("graph-shared-entity-plain", &statements(false))
}

/// The indexed graph under explicit runtime limits.
pub fn limited_world(config: cassie::config::CassieRuntimeConfig) -> SqlFixture {
    sql_fixture_with_config("graph-shared-entity-limited", &statements(true), config)
}

/// Rows as sorted `|`-joined text, one string per row.
///
/// # Panics
///
/// Panics for a value that is neither an integer nor text.
pub fn cells(rows: Vec<Vec<Value>>) -> Vec<String> {
    let mut text = ordered(rows);
    text.sort();
    text
}

/// Rows as `|`-joined text in result order.
///
/// # Panics
///
/// Panics for a value that is neither an integer nor text.
pub fn ordered(rows: Vec<Vec<Value>>) -> Vec<String> {
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| match value {
                    Value::Int64(number) => number.to_string(),
                    Value::String(text) => text,
                    _ => panic!("expected an integer or text value"),
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

pub fn strings(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_owned()).collect()
}
