//! Extreme BM25 parameters never produce a non-finite score.

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

#[test]
fn should_return_finite_score_for_the_largest_k1() {
    // Arrange
    let cassie = docs(" WITH (k1 = '1e308', b = '0')", &["alpha beta", "gamma"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT search_score(body, 'alpha') AS score FROM docs WHERE search(body, 'alpha')",
        vec![],
    );

    // Assert
    assert!(matches!(rows[0][0], Value::Float64(score) if score.is_finite() && score > 0.0));
}

#[test]
fn should_return_finite_score_for_the_largest_boost() {
    // Arrange
    let cassie = docs(" WITH (boost = '1e308')", &["alpha beta", "gamma"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT search_score(body, 'alpha') AS score FROM docs WHERE search(body, 'alpha')",
        vec![],
    );

    // Assert
    assert!(matches!(rows[0][0], Value::Float64(score) if score.is_finite() && score > 0.0));
}
