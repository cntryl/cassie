//! #1368: runs the frozen relevance workload through Cassie SQL. Ranked results and the graded
//! evaluation are driven by `tests/corpus/search_relevance/v1`; judgments are human input.

use std::collections::{BTreeMap, BTreeSet};

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_search_relevance::{
    evaluate_held_out, load_corpus, load_intents, load_judgments, validate_judgments, Intent,
    CUTOFF, MRR_FLOOR, NDCG_FLOOR,
};

const RANKED_SQL: &str = "SELECT _id, search_score(body, $1) AS score FROM relevance_docs \
    WHERE search(body, $1) ORDER BY score DESC, _id ASC LIMIT 10";

fn relevance_cassie() -> Cassie {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let root = cassie.create_session("root", None);
    cassie
        .execute_sql(&root, "CREATE TABLE relevance_docs (body TEXT)", vec![])
        .expect("create table");
    let collection = crate::support_sql::canonical_test_collection(&cassie, "relevance_docs");
    for document in load_corpus().documents {
        cassie
            .midge
            .put_document(
                &collection,
                Some(document.id),
                serde_json::json!({ "body": document.text }),
            )
            .expect("write document");
    }
    cassie
        .execute_sql(
            &root,
            "CREATE INDEX relevance_docs_fts ON relevance_docs USING fulltext (body)",
            vec![],
        )
        .expect("create index");
    cassie
}

fn ranked_ids(cassie: &Cassie, intent: &Intent) -> Vec<String> {
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            RANKED_SQL,
            vec![Value::String(intent.query.clone())],
        )
        .unwrap_or_else(|error| panic!("intent {}: {error}", intent.id))
        .rows()
        .iter()
        .map(|row| match &row[0] {
            Value::String(id) => id.clone(),
            _ => panic!("intent {} returned a non-text id", intent.id),
        })
        .collect()
}

fn run_all_intents(cassie: &Cassie) -> BTreeMap<String, Vec<String>> {
    load_intents()
        .intents
        .iter()
        .map(|intent| (intent.id.clone(), ranked_ids(cassie, intent)))
        .collect()
}

#[test]
fn should_freeze_a_workload_with_the_required_intent_shape() {
    // Arrange
    let corpus = load_corpus();
    let intents = load_intents();
    let documents: BTreeSet<_> = corpus.documents.iter().map(|d| d.id.as_str()).collect();

    // Act
    let count = |split: &str| intents.intents.iter().filter(|i| i.split == split).count();
    let kinds: BTreeSet<_> = intents.intents.iter().map(|i| i.kind.as_str()).collect();
    let unique: BTreeSet<_> = intents.intents.iter().map(|i| i.id.as_str()).collect();

    // Assert
    assert_eq!(corpus.version, 1);
    assert_eq!(intents.corpus_version, corpus.version);
    assert!(intents.intents.len() >= 20, "at least 20 distinct intents");
    assert_eq!(unique.len(), intents.intents.len(), "intent ids are unique");
    assert!(count("held_out") >= 10, "at least ten held-out answerable intents");
    assert!(count("no_hit") >= 1, "a separate no-hit set");
    for kind in ["exact_identifier", "description", "repeated_terms", "ambiguous", "no_hit"] {
        assert!(kinds.contains(kind), "missing intent kind {kind}");
    }
    assert!(intents
        .intents
        .iter()
        .any(|intent| !intent.authored_decoy_doc_ids.is_empty()));
    for intent in &intents.intents {
        for id in intent
            .authored_intended_doc_ids
            .iter()
            .chain(&intent.authored_decoy_doc_ids)
        {
            assert!(documents.contains(id.as_str()), "{} names unknown {id}", intent.id);
        }
        assert_eq!(
            intent.split == "no_hit",
            intent.authored_intended_doc_ids.is_empty(),
            "only no-hit intents have no intended document ({})",
            intent.id
        );
    }
}

#[test]
fn should_return_zero_hits_for_every_no_hit_intent() {
    // Arrange
    let cassie = relevance_cassie();
    let intents = load_intents();

    // Act
    let hits: Vec<_> = intents
        .intents
        .iter()
        .filter(|intent| intent.split == "no_hit")
        .map(|intent| (intent.id.clone(), ranked_ids(&cassie, intent).len()))
        .collect();

    // Assert
    assert!(!hits.is_empty());
    for (id, count) in hits {
        assert_eq!(count, 0, "no-hit intent {id} must return zero hits");
    }
}

#[test]
fn should_return_the_authored_intended_document_within_the_top_ten_for_answerable_intents() {
    // Arrange
    let cassie = relevance_cassie();

    // Act
    let results = run_all_intents(&cassie);

    // Assert: a fixture sanity check on the author's design, not a relevance judgment.
    for intent in load_intents().intents.iter().filter(|i| i.split != "no_hit") {
        let ranked = &results[&intent.id];
        assert!(!ranked.is_empty(), "{} returned nothing", intent.id);
        assert!(ranked.len() <= CUTOFF);
        assert!(
            intent
                .authored_intended_doc_ids
                .iter()
                .any(|id| ranked.contains(id)),
            "{} lost every authored intended document from the top {CUTOFF}",
            intent.id
        );
    }
}

#[test]
fn should_rank_the_keyword_stuffed_decoy_above_the_genuine_listing() {
    // Arrange
    let cassie = relevance_cassie();
    let intents = load_intents();
    let intent = intents
        .intents
        .iter()
        .find(|intent| !intent.authored_decoy_doc_ids.is_empty())
        .expect("a decoy intent");

    // Act
    let ranked = ranked_ids(&cassie, intent);
    let position = |id: &String| ranked.iter().position(|ranked_id| ranked_id == id);

    // Assert: exact BM25 ranks the stuffed decoy first, so a human grading it irrelevant makes
    // the intent fail the thresholds rather than pass on mathematical exactness alone.
    let decoy = position(&intent.authored_decoy_doc_ids[0]).expect("decoy is returned");
    let intended = position(&intent.authored_intended_doc_ids[0]).expect("intended is returned");
    assert!(decoy < intended, "decoy at {decoy}, intended at {intended}");
}

#[test]
fn should_write_per_query_results_and_report_the_qualification_state() {
    // Arrange
    let cassie = relevance_cassie();
    let corpus = load_corpus();
    let intents = load_intents();
    let judgments = load_judgments();

    // Act
    let results = run_all_intents(&cassie);
    let validation = validate_judgments(&judgments, &corpus, &intents);
    let evaluation = (judgments.status == "judged")
        .then(|| evaluate_held_out(&intents, &judgments, &results))
        .transpose();
    let qualification = match &evaluation {
        Ok(Some(evaluation)) if evaluation.meets_floors() => "qualified",
        Ok(Some(_)) => "failed_thresholds",
        Ok(None) => "pending_human_judgments",
        Err(_) => "invalid_judgments",
    };
    let report = serde_json::json!({
        "workload": "search_relevance_v1",
        "judgments_status": judgments.status,
        "qualification": qualification,
        "thresholds": { "ndcg_at_10": NDCG_FLOOR, "mrr_at_10": MRR_FLOOR },
        "per_query": intents.intents.iter().map(|intent| serde_json::json!({
            "intent": intent.id,
            "split": intent.split,
            "query": intent.query,
            "ranked_ids": results[&intent.id],
        })).collect::<Vec<_>>(),
        "held_out": evaluation.as_ref().ok().and_then(Option::as_ref).map(|e| serde_json::json!({
            "mean_ndcg_at_10": e.mean_ndcg,
            "mean_mrr_at_10": e.mean_mrr,
            "per_intent": e.per_intent.iter().map(|s| serde_json::json!({
                "intent": s.intent_id, "ndcg_at_10": s.ndcg, "mrr_at_10": s.mrr,
            })).collect::<Vec<_>>(),
        })),
    });
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("search_relevance_v1_results.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&report).expect("report json"))
        .expect("write per-query results");

    // Assert
    assert!(validation.is_ok(), "judgments file must be well formed");
    assert_eq!(results.len(), intents.intents.len());
    match qualification {
        "pending_human_judgments" => {
            assert_eq!(judgments.status, "unjudged");
            assert!(
                judgments.grades.is_empty(),
                "the template must stay empty until a human fills it"
            );
        }
        "qualified" => {}
        other => panic!("relevance qualification is {other}; see {}", path.display()),
    }
}
