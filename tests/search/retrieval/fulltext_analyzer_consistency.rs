use std::fmt::Write;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::types::{Value, Vector};

fn run(cassie: &Cassie, sql: &str) -> Vec<Vec<Value>> {
    run_with(cassie, sql, vec![])
}

fn run_with(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect("statement should execute")
        .rows()
        .to_vec()
}

fn scored_ids(rows: &[Vec<Value>]) -> Vec<(String, f64)> {
    rows.iter()
        .map(|row| {
            let (Value::String(id), Value::Float64(score)) = (&row[0], &row[1]) else {
                panic!("expected id and score columns");
            };
            (id.clone(), *score)
        })
        .collect()
}

fn hybrid_fixture() -> Cassie {
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    seed_hybrid_fixture(cassie)
}

fn hybrid_fixture_with_local_provider() -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 2,
    });
    let cassie = Cassie::new_in_memory_with_config(config).unwrap();
    cassie.startup().unwrap();
    run(
        &cassie,
        "CREATE TABLE ia (id TEXT NOT NULL, body TEXT, grp TEXT, embedding VECTOR(2))",
    );
    let mut rows = String::new();
    for index in 0..10 {
        writeln!(rows, "a{index:02},a{index:02},alpha,A,\"[1,0]\"").expect("write seeded row");
    }
    rows.push_str("m_alpha,m_alpha,alpha,B,\"[1,0]\"\nz_beta,z_beta,beta,B,\"[1,0]\"\n");
    copy_hybrid_fixture_rows(&cassie, &rows);
    run(
        &cassie,
        "CREATE INDEX ia_body_idx ON ia USING fulltext (body)",
    );
    cassie
}

fn copy_hybrid_fixture_rows(cassie: &Cassie, rows: &str) {
    let statement = cassie::sql::ast::CopyStatement {
        table: "ia".to_string(),
        columns: ["_id", "id", "body", "grp", "embedding"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        format: cassie::sql::ast::CopyFormat::Csv,
        header: false,
    };
    cassie
        .copy_from_csv_stdin(
            &cassie.create_session("tester", None),
            &statement,
            rows.as_bytes(),
        )
        .expect("copy rows with stable internal identities");
}

fn seed_hybrid_fixture(cassie: Cassie) -> Cassie {
    cassie.startup().unwrap();
    run(
        &cassie,
        "CREATE TABLE ia (id TEXT NOT NULL, body TEXT, grp TEXT, embedding VECTOR(2))",
    );
    let mut rows = (0..10)
        .map(|index| (format!("a{index:02}"), "alpha", "A"))
        .collect::<Vec<_>>();
    rows.push(("m_alpha".to_string(), "alpha", "B"));
    rows.push(("z_beta".to_string(), "beta", "B"));
    for (id, body, grp) in rows {
        run_with(
            &cassie,
            "INSERT INTO ia (id, body, grp, embedding) VALUES ($1, $2, $3, $4)",
            vec![
                Value::String(id),
                Value::String(body.to_string()),
                Value::String(grp.to_string()),
                Value::Vector(Vector::new(vec![1.0, 0.0])),
            ],
        );
    }
    run(
        &cassie,
        "CREATE INDEX ia_body_idx ON ia USING fulltext (body)",
    );
    cassie
}

#[test]
fn should_highlight_accent_folded_matches_in_snippet() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            cassie.startup().unwrap();
            run(&cassie, "CREATE TABLE snip_accent (id INT NOT NULL, body TEXT)");
            run(
                &cassie,
                "INSERT INTO snip_accent (id, body) VALUES (1, 'Cafe Bar'), (2, 'Caf\u{e9} Bar')",
            );
            run(
                &cassie,
                "CREATE INDEX snip_accent_ft ON snip_accent USING FULLTEXT (body) WITH (accent_folding = 'true')",
            );

            // Act
            let rows = run(
                &cassie,
                "SELECT id, search(body, 'cafe') AS m, snippet(body, 'cafe') AS s FROM snip_accent ORDER BY id",
            );

            // Assert
            assert_eq!(rows[0][1], Value::Bool(true));
            assert_eq!(rows[0][2], Value::String("<mark>Cafe</mark> Bar".to_string()));
            assert_eq!(rows[1][1], Value::Bool(true));
            assert_eq!(
                rows[1][2],
                Value::String("<mark>Caf\u{e9}</mark> Bar".to_string())
            );
        });
}

#[test]
fn should_highlight_accent_folded_matches_in_filtered_fulltext_read_snippet() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            cassie.startup().unwrap();
            run(&cassie, "CREATE TABLE snip_read (id INT NOT NULL, body TEXT)");
            run(
                &cassie,
                "INSERT INTO snip_read (id, body) VALUES (2, 'Caf\u{e9} Bar')",
            );
            run(
                &cassie,
                "CREATE INDEX snip_read_ft ON snip_read USING FULLTEXT (body) WITH (accent_folding = 'true')",
            );

            // Act
            let rows = run(
                &cassie,
                "SELECT id, snippet(body, 'cafe') AS s, search_score(body, 'cafe') AS score FROM snip_read WHERE search(body, 'cafe')",
            );

            // Assert
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0][1],
                Value::String("<mark>Caf\u{e9}</mark> Bar".to_string())
            );
        });
}

#[test]
fn should_not_highlight_case_mismatch_when_case_folding_is_disabled() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            cassie.startup().unwrap();
            run(&cassie, "CREATE TABLE snip_case (id INT NOT NULL, body TEXT)");
            run(
                &cassie,
                "INSERT INTO snip_case (id, body) VALUES (1, 'Alpha beta')",
            );
            run(
                &cassie,
                "CREATE INDEX snip_case_ft ON snip_case USING FULLTEXT (body) WITH (case_folding = 'false')",
            );

            // Act
            let rows = run(
                &cassie,
                "SELECT search(body, 'alpha') AS m, snippet(body, 'alpha') AS s FROM snip_case",
            );

            // Assert
            assert_eq!(rows[0][0], Value::Bool(false));
            assert_eq!(rows[0][1], Value::String("Alpha beta".to_string()));
        });
}

#[test]
fn should_keep_fulltext_scores_stable_after_filtered_hybrid_query() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let scored = "SELECT _id, search_score(body, 'alpha beta') AS score FROM ia ORDER BY score DESC LIMIT 2";
            let cold = hybrid_fixture();
            let baseline = scored_ids(&run(&cold, scored));
            let warmed = hybrid_fixture();

            // Act
            run(
                &warmed,
                "SELECT _id, hybrid_score(search_score(body, 'alpha beta'), vector_score(embedding, '[1,0]')) AS hs FROM ia WHERE grp = 'B' ORDER BY hs DESC LIMIT 1",
            );
            let after_hybrid = scored_ids(&run(&warmed, scored));

            // Assert
            assert_eq!(baseline.len(), 2);
            assert_eq!(after_hybrid.len(), baseline.len());
            assert!(baseline[0].1 > baseline[1].1);
            for (before, after) in baseline.iter().zip(&after_hybrid) {
                assert!((before.1 - after.1).abs() < 1e-12);
            }
        });
}

#[test]
fn should_score_hybrid_candidates_with_full_indexed_corpus_statistics() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = hybrid_fixture();
            let candidate_ids = run(&cassie, "SELECT _id FROM ia WHERE grp = 'B'")
                .into_iter()
                .filter_map(|row| match &row[0] {
                    Value::String(id) => Some(id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let whole_corpus = scored_ids(&run(
                &cassie,
                "SELECT _id, search_score(body, 'alpha beta') AS score FROM ia WHERE search(body, 'alpha beta') ORDER BY score DESC LIMIT 12",
            ));
            let vector_scores = scored_ids(&run(
                &cassie,
                "SELECT _id, vector_score(embedding, '[1,0]') AS score FROM ia WHERE grp = 'B'",
            ));
            let expected = whole_corpus
                .into_iter()
                .filter(|(id, _)| candidate_ids.contains(id))
                .map(|(id, text_score)| {
                    let vector_score = vector_scores
                        .iter()
                        .find(|(candidate_id, _)| candidate_id == &id)
                        .expect("whole-corpus result has a vector score")
                        .1;
                    (
                        id,
                        cassie::hybrid::hybrid_score(text_score, vector_score, None),
                    )
                })
                .collect::<Vec<_>>();
            let mut expected = expected;
            expected.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });

            // Act
            let before = cassie.metrics();
            let actual = scored_ids(&run(
                &cassie,
                "SELECT _id, hybrid_score(search_score(body, 'alpha beta'), vector_score(embedding, '[1,0]')) AS hs FROM ia WHERE grp = 'B' ORDER BY hs DESC LIMIT 12",
            ));
            let after = cassie.metrics();

            // Assert
            assert!(
                after["hybrid"]["retrieval_stage_queries_total"]
                    .as_u64()
                    .unwrap()
                    > before["hybrid"]["retrieval_stage_queries_total"]
                        .as_u64()
                        .unwrap()
            );
            assert_eq!(actual, expected);
        });
}

#[test]
fn should_use_persisted_corpus_statistics_for_bounded_hybrid_candidates() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = hybrid_fixture_with_local_provider();
            // Retain links across equal-vector clusters so this corpus-statistics
            // fixture deterministically exercises the persisted ANN path.
            run(
                &cassie,
                "CREATE INDEX ia_embedding_hnsw ON ia USING vector (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, m = 128, ef_construction = 128, ef_search = 32)",
            );
            let mut rows = String::new();
            for index in 0..100 {
                writeln!(
                    rows,
                    "unrelated_{index:03},unrelated_{index:03},unrelated,A,\"[100,100]\""
                )
                .expect("write unrelated row");
            }
            copy_hybrid_fixture_rows(&cassie, &rows);
            let whole_corpus = scored_ids(&run(
                &cassie,
                "SELECT _id, search_score(body, 'alpha beta') AS score FROM ia WHERE search(body, 'alpha beta') ORDER BY score DESC LIMIT 12",
            ));
            let vector_scores = scored_ids(&run(
                &cassie,
                "SELECT _id, vector_score(embedding, '[1,0]') AS score FROM ia WHERE grp = 'B'",
            ));

            // Act
            let before = cassie.metrics();
            let actual = scored_ids(&run(
                &cassie,
                "SELECT _id, hybrid_score(search_score(body, 'alpha beta'), vector_score(embedding, '[1,0]')) AS hs FROM ia WHERE grp = 'B' ORDER BY hs DESC LIMIT 12",
            ));
            let after = cassie.metrics();

            // Assert
            assert!(
                after["hybrid"]["ann_reads_total"].as_u64().unwrap()
                    > before["hybrid"]["ann_reads_total"].as_u64().unwrap()
            );
            assert!(
                after["hybrid"]["candidate_row_fetches_total"].as_u64().unwrap()
                    > before["hybrid"]["candidate_row_fetches_total"].as_u64().unwrap()
            );
            assert!(
                after["hybrid"]["candidate_row_fetches_total"]
                    .as_u64()
                    .unwrap()
                    - before["hybrid"]["candidate_row_fetches_total"]
                        .as_u64()
                        .unwrap()
                    < 112
            );
            assert_ne!(actual.len(), 0);
            for (id, actual_score) in actual {
                let text_score = whole_corpus
                    .iter()
                    .find(|(candidate_id, _)| candidate_id == &id)
                    .expect("bounded hybrid result belongs to full-text candidates")
                    .1;
                let vector_score = vector_scores
                    .iter()
                    .find(|(candidate_id, _)| candidate_id == &id)
                    .expect("bounded hybrid result has a vector score")
                    .1;
                let expected_score = cassie::hybrid::hybrid_score(text_score, vector_score, None);
                assert!((actual_score - expected_score).abs() < 1e-12);
            }
        });
}
