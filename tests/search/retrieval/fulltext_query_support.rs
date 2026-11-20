//! Shared fixtures for the `'query'`-syntax full-text suites.

use super::support_sql as support;

use cassie::app::{Cassie, CassieError};
use cassie::types::Value;

pub(super) fn run(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect("statement should execute")
        .rows()
        .to_vec()
}

pub(super) fn run_error(cassie: &Cassie, sql: &str, params: Vec<Value>) -> CassieError {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect_err("statement should fail")
}

/// A hand-labelled product corpus. Each label is the expected-ID vocabulary of
/// the query tables in the suites.
pub(super) const CORPUS: [(&str, &str); 13] = [
    ("p01", "red running shoes for trail"),
    ("p02", "red shoes on sale"),
    ("p03", "shoes red"),
    ("p04", "running red lightweight shoes"),
    ("p05", "blue running shoes"),
    ("p06", "shoe care kit"),
    ("p07", "shoelace red"),
    ("p08", "colour wheel poster"),
    ("p09", "color theory book"),
    ("p10", "Alpha-Beta testing guide"),
    ("p11", "beta alpha reversed"),
    ("p12", "alpha the beta"),
    ("p13", "Caf\u{e9} au lait, na\u{ef}ve ideas!"),
];

pub(super) fn corpus_cassie(index_options: &str) -> Cassie {
    let cassie = corpus_table();
    run(
        &cassie,
        &format!("CREATE INDEX products_ft ON products USING fulltext (body){index_options}"),
        vec![],
    );
    cassie
}

pub(super) fn corpus_table() -> Cassie {
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(
        &cassie,
        "CREATE TABLE products (label TEXT, body TEXT)",
        vec![],
    );
    for (label, body) in CORPUS {
        insert(&cassie, label, Some(body));
    }
    cassie
}

pub(super) fn insert(cassie: &Cassie, label: &str, body: Option<&str>) {
    run(
        cassie,
        "INSERT INTO products (label, body) VALUES ($1, $2)",
        vec![
            Value::String(label.to_string()),
            body.map_or(Value::Null, |body| Value::String(body.to_string())),
        ],
    );
}

fn labels_of(rows: Vec<Vec<Value>>) -> Vec<String> {
    let mut labels = rows
        .into_iter()
        .map(|row| match &row[0] {
            Value::String(label) => label.clone(),
            _ => panic!("expected a text label"),
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels
}

/// Indexed execution: the filtered-read fast path over the persisted index.
pub(super) fn indexed(cassie: &Cassie, query: &str) -> Vec<String> {
    labels_of(run(
        cassie,
        "SELECT label, search_score(body, $1, 'query') AS score FROM products \
         WHERE search(body, $1, 'query')",
        vec![Value::String(query.to_string())],
    ))
}

/// Indexed top-k execution over the persisted index.
pub(super) fn indexed_top_k(cassie: &Cassie, query: &str) -> Vec<(String, f64)> {
    let labels = run(cassie, "SELECT _id, label FROM products", vec![])
        .into_iter()
        .filter_map(|row| match (&row[0], &row[1]) {
            (Value::String(id), Value::String(label)) => Some((id.clone(), label.clone())),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    run(
        cassie,
        "SELECT _id, search_score(body, $1, 'query') AS score FROM products \
         WHERE search(body, $1, 'query') ORDER BY score DESC LIMIT 50",
        vec![Value::String(query.to_string())],
    )
    .into_iter()
    .map(|row| match (&row[0], &row[1]) {
        (Value::String(id), Value::Float64(score)) => (labels[id].clone(), *score),
        _ => panic!("expected an id and a score"),
    })
    .collect()
}

/// Authoritative re-evaluation: the row evaluator over every row, with no
/// retrieval fast path.
pub(super) fn authoritative(cassie: &Cassie, query: &str) -> Vec<String> {
    labels_of(
        run(
            cassie,
            "SELECT label, search(body, $1, 'query') AS hit FROM products",
            vec![Value::String(query.to_string())],
        )
        .into_iter()
        .filter(|row| row[1] == Value::Bool(true))
        .collect(),
    )
}

pub(super) fn authoritative_scores(cassie: &Cassie, query: &str) -> Vec<(String, f64)> {
    let mut scores = run(
        cassie,
        "SELECT label, search(body, $1, 'query') AS hit, search_score(body, $1, 'query') AS score \
         FROM products",
        vec![Value::String(query.to_string())],
    )
    .into_iter()
    .filter(|row| row[1] == Value::Bool(true))
    .map(|row| match (&row[0], &row[2]) {
        (Value::String(label), Value::Float64(score)) => (label.clone(), *score),
        _ => panic!("expected a label and a score"),
    })
    .collect::<Vec<_>>();
    scores.sort_by(|left, right| left.0.cmp(&right.0));
    scores
}

pub(super) fn metric(cassie: &Cassie, name: &str) -> u64 {
    cassie.metrics()["search"][name].as_u64().unwrap_or(0)
}

pub(super) fn expect(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|label| (*label).to_string()).collect()
}
