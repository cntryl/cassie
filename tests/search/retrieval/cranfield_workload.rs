//! #1368: network-free checks for the Cranfield evaluation. The collection is never committed (no
//! published license); these tests cover the manifest, the parsers on tiny hand-written fixtures in
//! the Cranfield format, the relevance-level mapping and the independent BM25 reference. The
//! download and full evaluation live in `benches/relevance_cranfield.rs`.

use std::collections::{BTreeMap, BTreeSet};

use cassie::types::Value;

use crate::support_cranfield::{
    grade_for_level, load_manifest, parse_documents, parse_qrels, parse_queries, CranDocument,
};
use crate::support_cranfield_reference::{analyze, compare, Reference, B, K1};
use crate::support_search_relevance::{mrr_at_k_with_threshold, ndcg_at_k, CUTOFF};

const DOCUMENTS: &str = ".I 1\n.T\nwing flutter in a slipstream\n.A\nsmith,j.\n.B\nj. test. 1, 1950, 1.\n.W\nwing flutter in a slipstream .\n  the wing was\ntested in a tunnel .\n.I 2\n.T\nheat transfer\n.W\nheat transfer across a plate .\n";
const QUERIES: &str = ".I 001\n.W\nwhat is wing flutter\n.I 004\n.W\nheat transfer\nacross plates .\n.I 009\n.W\nplate\n";
const QRELS: &str = "1 1 1\n1 2 4\n2 2 2\n3 1 -1\n";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn should_describe_an_unredistributed_collection_pinned_by_digest() {
    // Arrange
    let expected_version = 1;

    // Act
    let manifest = load_manifest();

    // Assert
    assert_eq!(manifest.version, expected_version);
    assert!(manifest.source.url.ends_with("/cran.tar.gz"));
    assert_eq!(manifest.source.archive_sha256.len(), 64);
    assert!(manifest
        .source
        .archive_sha256
        .chars()
        .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()));
    assert!(manifest.source.archive_bytes > 0);
    assert!(!manifest.redistribution.redistributed);
    assert!(manifest
        .redistribution
        .reason
        .contains("no published license"));
    assert!(manifest.citation.contains("Cleverdon"));
    let members: BTreeSet<_> = [
        &manifest.source.members.documents,
        &manifest.source.members.queries,
        &manifest.source.members.qrels,
    ]
    .into_iter()
    .collect();
    assert_eq!(members.len(), 3, "member file names are distinct");
}

#[test]
fn should_state_an_evaluation_contract_that_matches_the_reference_implementation() {
    // Arrange
    let manifest = load_manifest();

    // Act
    let contract = &manifest.evaluation;

    // Assert
    assert_eq!(contract.cutoff, CUTOFF);
    assert!((contract.bm25.k1 - K1).abs() < f64::EPSILON);
    assert!((contract.bm25.b - B).abs() < f64::EPSILON);
    assert!(contract.score_tolerance > 0.0 && contract.score_tolerance <= 1e-6);
    assert!(contract.sql.contains("search_score(body, $1)"));
    assert!(contract
        .sql
        .contains("ORDER BY score DESC, _id ASC LIMIT 10"));
    assert!(contract.contract.contains("independent BM25 reference"));
    assert!(contract.contract.contains("not thresholded"));
    assert_eq!(manifest.counts.documents, 1400);
    assert_eq!(manifest.counts.queries, 225);
    assert!(manifest.counts.qrel_lines > manifest.counts.queries);
}

#[test]
fn should_keep_the_documented_level_mapping_consistent_with_the_code() {
    // Arrange
    let manifest = load_manifest();
    let conventions = &manifest.conventions;

    // Act
    let documented: BTreeSet<_> = conventions.relevance_levels.keys().collect();
    let mapped: BTreeSet<_> = conventions.level_to_grade.keys().collect();

    // Assert
    assert_eq!(documented, mapped, "every documented level has a grade");
    for (level, grade) in &conventions.level_to_grade {
        let level: i32 = level.parse().expect("numeric level");
        assert_eq!(grade_for_level(level), Some(*grade), "level {level}");
    }
    assert_eq!(grade_for_level(0), None);
    assert_eq!(grade_for_level(5), None);
}

#[test]
fn should_grade_a_stronger_level_higher_with_no_interest_as_unjudged() {
    // Arrange
    let levels = [1, 2, 3, 4];

    // Act
    let grades: Vec<u8> = levels
        .iter()
        .map(|level| grade_for_level(*level).expect("defined level"))
        .collect();

    // Assert
    assert!(grades.windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(grade_for_level(-1), Some(0));
    assert_eq!(load_manifest().conventions.mrr_relevant_grade, 2);
}

#[test]
fn should_not_count_historical_interest_toward_mrr_but_count_it_toward_ndcg() {
    // Arrange: level 4 (grade 1) is a historical reference; level 3 (grade 2) is useful background.
    let judged: BTreeMap<String, u8> = [("old", 1), ("useful", 2)]
        .into_iter()
        .map(|(id, grade)| (id.to_string(), grade))
        .collect();
    let ranked = strings(&["old", "useful"]);
    let threshold = load_manifest().conventions.mrr_relevant_grade;

    // Act
    let mrr = mrr_at_k_with_threshold(&ranked, &judged, CUTOFF, threshold);
    let ndcg = ndcg_at_k(&ranked, &judged, CUTOFF);

    // Assert
    assert!((mrr - 0.5).abs() < 1e-12, "first relevant is at rank 2");
    assert!(
        ndcg > 0.0 && ndcg < 1.0,
        "the ideal order puts useful first"
    );
}

#[test]
fn should_parse_documents_with_continuation_lines_joined() {
    // Arrange
    let input = DOCUMENTS;

    // Act
    let documents = parse_documents(input);

    // Assert
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].id, "1");
    assert_eq!(documents[0].title, "wing flutter in a slipstream");
    assert_eq!(documents[0].author, "smith,j.");
    assert_eq!(documents[0].bib, "j. test. 1, 1950, 1.");
    assert_eq!(
        documents[0].text,
        "wing flutter in a slipstream .\n  the wing was\ntested in a tunnel ."
    );
    assert_eq!(documents[1].id, "2");
    assert_eq!(documents[1].author, "");
    assert_eq!(
        documents[1].indexed_text(),
        "heat transfer heat transfer across a plate ."
    );
}

#[test]
fn should_number_queries_by_position_when_source_ids_skip() {
    // Arrange
    let input = QUERIES;

    // Act
    let queries = parse_queries(input);

    // Assert
    let numbers: Vec<usize> = queries.iter().map(|query| query.number).collect();
    let sources: Vec<&str> = queries
        .iter()
        .map(|query| query.source_id.as_str())
        .collect();
    assert_eq!(numbers, vec![1, 2, 3]);
    assert_eq!(sources, vec!["001", "004", "009"]);
    assert_eq!(queries[1].text, "heat transfer\nacross plates .");
}

#[test]
fn should_parse_qrels_into_grades_per_query_position() {
    // Arrange
    let input = QRELS;

    // Act
    let qrels = parse_qrels(input).expect("well-formed qrels");

    // Assert
    assert_eq!(qrels[&1]["1"], 4);
    assert_eq!(qrels[&1]["2"], 1);
    assert_eq!(qrels[&2]["2"], 3);
    assert_eq!(qrels[&3]["1"], 0, "level -1 is no interest");
}

#[test]
fn should_reject_malformed_qrels_lines() {
    // Arrange
    let wrong_columns = "1 2\n";
    let undefined_level = "1 2 7\n";

    // Act
    let columns = parse_qrels(wrong_columns).expect_err("two columns");
    let level = parse_qrels(undefined_level).expect_err("level 7");

    // Assert
    assert!(columns.contains("expected 3"));
    assert!(level.contains("undefined level 7"));
}

#[test]
fn should_analyze_like_the_documented_default_analyzer() {
    // Arrange
    let text = "The Wing-Flutter of A slipstream, 3D";

    // Act
    let tokens = analyze(text);

    // Assert
    assert_eq!(tokens, strings(&["wing", "flutter", "slipstream", "3d"]));
}

#[test]
fn should_score_the_reference_by_hand_computed_bm25() {
    // Arrange: N=2, "wing" in 1 document. Lengths 2 and 1, average 1.5.
    let reference = Reference::new(&[
        ("a".to_string(), "wing wing".to_string()),
        ("b".to_string(), "plate".to_string()),
    ]);
    let idf = ((2.0_f64 - 1.0 + 0.5) / (1.0 + 0.5) + 1.0).ln();
    let norm = 2.0 + K1 * (1.0 - B + B * 2.0 / 1.5);
    let expected = idf * 2.0 * (K1 + 1.0) / norm;

    // Act
    let top = reference.top("Wing", CUTOFF);

    // Assert
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].0, "a");
    assert!((top[0].1 - expected).abs() < 1e-12);
}

#[test]
fn should_agree_with_cassie_on_a_tiny_cranfield_format_collection() {
    // Arrange
    let documents: Vec<CranDocument> = parse_documents(DOCUMENTS);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, "CREATE TABLE docs (body TEXT)", vec![])
        .expect("create table");
    let collection = crate::support_sql::canonical_test_collection(&cassie, "docs");
    for document in &documents {
        cassie
            .midge
            .put_document(
                &collection,
                Some(document.id.clone()),
                serde_json::json!({ "body": document.indexed_text() }),
            )
            .expect("write document");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX docs_fts ON docs USING fulltext (body)",
            vec![],
        )
        .expect("create index");
    let manifest = load_manifest();
    let reference = Reference::new(
        &documents
            .iter()
            .map(|document| (document.id.clone(), document.indexed_text()))
            .collect::<Vec<_>>(),
    );

    for query in parse_queries(QUERIES) {
        // Act
        let actual: Vec<(String, f64)> = cassie
            .execute_sql(
                &session,
                &manifest.evaluation.sql,
                vec![Value::String(query.text.clone())],
            )
            .expect("ranked query")
            .rows()
            .iter()
            .map(|row| match (&row[0], &row[1]) {
                (Value::String(id), Value::Float64(score)) => (id.clone(), *score),
                _ => panic!("query {} returned an unexpected row shape", query.number),
            })
            .collect();
        let agreement = compare(
            &actual,
            &reference.top(&query.text, manifest.evaluation.cutoff),
            manifest.evaluation.score_tolerance,
        );

        // Assert
        assert!(agreement.is_full(), "query {} disagrees", query.number);
    }
}
