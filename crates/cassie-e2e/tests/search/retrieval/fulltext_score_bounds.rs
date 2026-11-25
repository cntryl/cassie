//! Extreme BM25 parameters never produce a non-finite score.

use super::fulltext_query_support::run;
use super::support_sql as support;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
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

#[test]
fn should_floor_sparse_corpus_bm25_normalization_until_average_exceeds_one() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    run(&cassie, "CREATE TABLE docs (label TEXT, body TEXT)", vec![]);
    run(
        &cassie,
        "INSERT INTO docs VALUES ('d0', 'alpha'), ('d1', ''), ('d2', ''), ('d3', '')",
        vec![],
    );
    let row_score = || {
        let rows = run(
            &cassie,
            "SELECT label, search_score(body, 'alpha') FROM docs",
            vec![],
        );
        rows.into_iter()
            .find(|row| matches!(&row[0], Value::String(label) if label == "d0"))
            .expect("find alpha row")
    };
    let indexed_score = || {
        let posting_reads = super::fulltext_query_support::metric(&cassie, "posting_reads_total");
        let rows = run(
            &cassie,
            "SELECT label, search_score(body, 'alpha') AS score FROM docs WHERE search(body, 'alpha')",
            vec![],
        );
        assert!(
            super::fulltext_query_support::metric(&cassie, "posting_reads_total") > posting_reads
        );
        assert_eq!(rows.len(), 1);
        rows.into_iter().next().expect("find alpha row")
    };

    // Act
    let sparse_scan = row_score();
    run(
        &cassie,
        "CREATE INDEX docs_body ON docs USING fulltext(body)",
        vec![],
    );
    let sparse_indexed = indexed_score();
    run(
        &cassie,
        "UPDATE docs SET body = 'beta gamma' WHERE label <> 'd0'",
        vec![],
    );
    let dense_scan = row_score();
    let dense_indexed = indexed_score();

    // Assert
    // In both corpora N=4, df=1, tf=1 and the matching row has one token.
    // Effective average 1 makes its saturation factor 1. Average 7/4 makes
    // its length normalization 19/28 and its saturation factor 154/127.
    let idf = (10.0_f64 / 3.0).ln();
    for (row, expected) in [
        (sparse_scan, idf),
        (sparse_indexed, idf),
        (dense_scan, idf * 154.0 / 127.0),
        (dense_indexed, idf * 154.0 / 127.0),
    ] {
        assert_eq!(row[0], Value::String("d0".to_string()));
        let Value::Float64(score) = row[1] else {
            panic!("expected a BM25 score");
        };
        assert!(
            (score - expected).abs() < 1e-12,
            "actual {score}, expected {expected}"
        );
    }
}
