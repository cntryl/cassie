//! A full-text index boost scales a score; it never decides whether a row matches.

use super::fulltext_query_support::run;
use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

fn docs(options: &str, rows: &[&str]) -> Cassie {
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(&cassie, "CREATE TABLE docs (label TEXT, body TEXT)", vec![]);
    for (index, body) in rows.iter().enumerate() {
        run(
            &cassie,
            "INSERT INTO docs (label, body) VALUES ($1, $2)",
            vec![
                Value::String(format!("d{index}")),
                Value::String((*body).to_string()),
            ],
        );
    }
    run(
        &cassie,
        &format!("CREATE INDEX docs_body ON docs USING fulltext (body){options}"),
        vec![],
    );
    cassie
}

fn hits(cassie: &Cassie, term: &str) -> usize {
    run(
        cassie,
        "SELECT label FROM docs WHERE search(body, $1)",
        vec![Value::String(term.to_string())],
    )
    .len()
}

#[test]
fn should_match_row_when_boost_is_zero_on_filtered_read() {
    // Arrange
    let cassie = docs(" WITH (boost = '0')", &["alpha beta", "gamma"]);

    // Act
    let count = hits(&cassie, "alpha");

    // Assert
    assert_eq!(count, 1);
}

#[test]
fn should_match_row_when_boost_is_zero_on_row_evaluation() {
    // Arrange
    let cassie = docs(" WITH (boost = '0')", &["alpha beta", "gamma"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT label, search(body, 'alpha') AS hit FROM docs ORDER BY label",
        vec![],
    );

    // Assert
    assert_eq!(rows[0][1], Value::Bool(true));
    assert_eq!(rows[1][1], Value::Bool(false));
}

#[test]
fn should_match_row_when_boost_is_zero_in_top_k() {
    // Arrange
    let cassie = docs(" WITH (boost = '0')", &["alpha beta", "gamma"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT label FROM docs WHERE search(body, 'alpha') \
         ORDER BY search_score(body, 'alpha') DESC LIMIT 5",
        vec![],
    );

    // Assert
    assert_eq!(rows.len(), 1);
}

#[test]
fn should_match_row_when_boost_is_zero_in_filtered_top_k() {
    // Arrange
    let cassie = docs(" WITH (boost = '0')", &["alpha beta", "gamma"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT label FROM docs WHERE search(body, 'alpha') AND label <> 'zzz' \
         ORDER BY search_score(body, 'alpha') DESC LIMIT 5",
        vec![],
    );

    // Assert
    assert_eq!(rows.len(), 1);
}

#[test]
fn should_score_zero_for_matching_row_when_boost_is_zero() {
    // Arrange
    let cassie = docs(" WITH (boost = '0')", &["alpha beta"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT search_score(body, 'alpha') AS score FROM docs",
        vec![],
    );

    // Assert
    assert_eq!(rows[0][0], Value::Float64(0.0));
}
