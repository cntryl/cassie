//! Shared fixtures for incremental full-text maintenance suites.
//!
//! Collections are created through the storage API with explicit string document
//! ids so reads use the `_id` scored top-k shape that the persisted index serves.

use cassie::app::Cassie;
use cassie::midge::adapter::fulltext_retrieval::PersistedFulltextIndexState;
use cassie::types::Value;

use crate::support_executor as executor;

pub fn run(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect("statement should execute")
        .rows()
        .to_vec()
}

pub fn index_name(table: &str) -> String {
    format!("{table}_ft")
}

/// Creates a collection with its full-text index first, so every later write is
/// maintained against an online index.
pub fn create_online_table(cassie: &Cassie, table: &str) {
    executor::create_text_collection(cassie, table, &["id", "body"]);
    executor::put_fulltext_index(cassie, table, &index_name(table), "body", &[]);
    cassie.catalog.register_index(executor::fulltext_index(
        table,
        &index_name(table),
        "body",
        &[],
    ));
}

/// Creates a SQL table with its full-text index first.
pub fn create_online_sql_table(cassie: &Cassie, table: &str) {
    run(
        cassie,
        &format!("CREATE TABLE {table} (id INT NOT NULL, body TEXT)"),
        vec![],
    );
    run(
        cassie,
        &format!("CREATE INDEX {table}_ft ON {table} USING fulltext (body)"),
        vec![],
    );
}

/// Inserts or replaces the document with id `id`.
pub fn upsert(cassie: &Cassie, table: &str, id: i64, body: &str) {
    cassie
        .midge
        .put_document(
            table,
            Some(id.to_string()),
            serde_json::json!({ "body": body }),
        )
        .expect("put document");
}

pub fn delete(cassie: &Cassie, table: &str, id: i64) {
    cassie
        .midge
        .delete_document(table, &id.to_string())
        .expect("delete document");
}

fn doc_number(value: &Value) -> i64 {
    match value {
        Value::String(id) => id.parse().expect("numeric document id"),
        other => panic!("expected a string document id: {other:?}"),
    }
}

/// Ids of the hits, in the query shape served by the persisted index.
pub fn ids(cassie: &Cassie, table: &str, query: &str) -> Vec<i64> {
    let mut found = run(
        cassie,
        &format!(
            "SELECT _id, search_score(body, $1) AS score FROM {table} \
             WHERE search(body, $1) ORDER BY score DESC LIMIT 1000"
        ),
        vec![Value::String(query.to_string())],
    )
    .iter()
    .map(|row| doc_number(&row[0]))
    .collect::<Vec<_>>();
    found.sort_unstable();
    found
}

/// Ranked hits with their scores and snippets, ordered by id. Scores come from
/// the persisted-index query shape; snippets need the row text and use the row shape.
pub fn ranked(cassie: &Cassie, table: &str, query: &str) -> Vec<(i64, String, String)> {
    let snippets = run(
        cassie,
        &format!("SELECT _id, snippet(body, $1) FROM {table} WHERE search(body, $1)"),
        vec![Value::String(query.to_string())],
    )
    .iter()
    .map(|row| match &row[1] {
        Value::String(snippet) => (doc_number(&row[0]), snippet.clone()),
        other => panic!("expected a snippet: {other:?}"),
    })
    .collect::<std::collections::BTreeMap<_, _>>();
    let mut hits = run(
        cassie,
        &format!(
            "SELECT _id, search_score(body, $1) AS score FROM {table} \
             WHERE search(body, $1) ORDER BY score DESC LIMIT 1000"
        ),
        vec![Value::String(query.to_string())],
    )
    .iter()
    .map(|row| match &row[1] {
        Value::Float64(score) => {
            let id = doc_number(&row[0]);
            (
                id,
                format!("{score:.12}"),
                snippets.get(&id).cloned().unwrap_or_default(),
            )
        }
        other => panic!("expected a score: {other:?}"),
    })
    .collect::<Vec<_>>();
    hits.sort();
    hits
}

fn collection_of(cassie: &Cassie, table: &str) -> String {
    cassie
        .catalog
        .get_schema(table)
        .map_or_else(|| table.to_string(), |schema| schema.collection)
}

pub fn persisted_state(cassie: &Cassie, table: &str) -> PersistedFulltextIndexState {
    cassie
        .midge
        .get_persisted_fulltext_index_state(&collection_of(cassie, table), &index_name(table))
        .expect("persisted full-text state is consistent")
        .expect("full-text index has persisted state")
}

pub fn force_rebuild(cassie: &Cassie, table: &str) {
    cassie
        .midge
        .rebuild_fulltext_index_for_diagnostics(&collection_of(cassie, table), &index_name(table))
        .expect("rebuild full-text index");
}

/// Asserts that the maintained persisted state equals an independent clean
/// rebuild: postings membership, document frequencies and corpus totals.
pub fn assert_state_matches_clean_rebuild(cassie: &Cassie, table: &str) {
    let maintained = persisted_state(cassie, table);
    force_rebuild(cassie, table);
    assert_eq!(
        maintained,
        persisted_state(cassie, table),
        "persisted state"
    );
}

/// Asserts that the live (incrementally maintained) index equals an independent
/// clean rebuild from the same committed rows: postings membership, document
/// frequencies, corpus totals, ranked hits and snippets.
pub fn assert_matches_clean_rebuild(cassie: &Cassie, table: &str, queries: &[&str]) {
    let maintained_state = persisted_state(cassie, table);
    let maintained = queries
        .iter()
        .map(|query| ranked(cassie, table, query))
        .collect::<Vec<_>>();
    force_rebuild(cassie, table);
    let rebuilt = queries
        .iter()
        .map(|query| ranked(cassie, table, query))
        .collect::<Vec<_>>();
    assert_eq!(
        maintained_state,
        persisted_state(cassie, table),
        "persisted state"
    );
    assert_eq!(maintained, rebuilt, "ranked hits and snippets");
}

pub fn search_metric(cassie: &Cassie, name: &str) -> u64 {
    cassie.metrics()["search"][name]
        .as_u64()
        .unwrap_or_else(|| panic!("search metric {name} is missing"))
}

pub fn fallback_total(cassie: &Cassie) -> u64 {
    search_metric(cassie, "row_scan_fallback_total")
}
