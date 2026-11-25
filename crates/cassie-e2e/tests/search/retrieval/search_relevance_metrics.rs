//! #1368: nDCG@10, MRR@10 and the graded threshold evaluation on hand-computed examples.

use std::collections::BTreeMap;

use crate::support_search_relevance::{
    evaluate_held_out, mrr_at_k, ndcg_at_k, validate_judgments, Corpus, Document, Intent,
    IntentSet, Judgments, CUTOFF,
};

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn grades(values: &[(&str, u8)]) -> BTreeMap<String, u8> {
    values
        .iter()
        .map(|(id, grade)| ((*id).to_string(), *grade))
        .collect()
}

#[test]
fn should_compute_ndcg_from_the_gain_with_log_discount_definition() {
    // Arrange: gains are 0, 7, 3 at ranks 1..3; the ideal order is B (7) then C (3).
    let ranked = ids(&["A", "B", "C"]);
    let judged = grades(&[("A", 0), ("B", 3), ("C", 2)]);
    let dcg = 7.0 / 3.0_f64.log2() + 3.0 / 2.0;
    let ideal = 7.0 + 3.0 / 3.0_f64.log2();

    // Act
    let score = ndcg_at_k(&ranked, &judged, CUTOFF);

    // Assert
    assert!((score - dcg / ideal).abs() < 1e-12);
}

#[test]
fn should_score_a_perfect_ordering_as_one_with_no_relevant_document_as_zero() {
    // Arrange
    let judged = grades(&[("B", 3), ("C", 2)]);

    // Act
    let perfect = ndcg_at_k(&ids(&["B", "C", "X"]), &judged, CUTOFF);
    let missed = ndcg_at_k(&ids(&["X", "Y"]), &judged, CUTOFF);
    let nothing_graded = ndcg_at_k(&ids(&["X"]), &grades(&[]), CUTOFF);

    // Assert
    assert!((perfect - 1.0).abs() < 1e-12);
    assert!(missed.abs() < 1e-12);
    assert!(nothing_graded.abs() < 1e-12);
}

#[test]
fn should_count_duplicate_documents_only_at_their_first_rank_for_ndcg() {
    // Arrange: A has gain 3, B has gain 1, and the duplicate still occupies rank 2.
    let judged = grades(&[("A", 2), ("B", 1)]);
    let distinct = ids(&["A", "B"]);
    let repeated = ids(&["A", "A", "B"]);
    let ideal = 3.0 + 1.0 / 3.0_f64.log2();

    // Act
    let perfect = ndcg_at_k(&distinct, &judged, 2);
    let duplicate_at_cutoff = ndcg_at_k(&repeated, &judged, 2);
    let duplicate_before_unique_result = ndcg_at_k(&repeated, &judged, 3);

    // Assert
    assert!((perfect - 1.0).abs() < 1e-12);
    assert!((duplicate_at_cutoff - 3.0 / ideal).abs() < 1e-12);
    assert!((duplicate_before_unique_result - (3.0 + 1.0 / 2.0) / ideal).abs() < 1e-12);
}

#[test]
fn should_ignore_results_below_the_cutoff_when_computing_ndcg() {
    // Arrange
    let judged = grades(&[("B", 3)]);

    // Act
    let at_one = ndcg_at_k(&ids(&["X", "B"]), &judged, 1);

    // Assert
    assert!(at_one.abs() < 1e-12);
}

#[test]
fn should_use_the_first_document_graded_relevant_for_reciprocal_rank() {
    // Arrange: grade 1 is marginal and does not count; the first grade >= 2 is at rank 3.
    let judged = grades(&[("A", 1), ("C", 2), ("D", 3)]);

    // Act
    let third = mrr_at_k(&ids(&["A", "B", "C", "D"]), &judged, CUTOFF);
    let first = mrr_at_k(&ids(&["D"]), &judged, CUTOFF);
    let none = mrr_at_k(&ids(&["A", "B"]), &judged, CUTOFF);
    let beyond_cutoff = mrr_at_k(&ids(&["A", "B", "C"]), &judged, 2);

    // Assert
    assert!((third - 1.0 / 3.0).abs() < 1e-12);
    assert!((first - 1.0).abs() < 1e-12);
    assert!(none.abs() < 1e-12);
    assert!(beyond_cutoff.abs() < 1e-12);
}

fn tiny_workload() -> (Corpus, IntentSet) {
    let corpus = Corpus {
        version: 1,
        documents: ["real", "decoy"]
            .iter()
            .map(|id| Document {
                id: (*id).to_string(),
                text: (*id).to_string(),
            })
            .collect(),
    };
    let intents = IntentSet {
        version: 1,
        corpus_version: 1,
        intents: (0..2)
            .map(|index| Intent {
                id: format!("held-{index}"),
                split: "held_out".to_string(),
                kind: "description".to_string(),
                query: "query".to_string(),
                authored_intended_doc_ids: ids(&["real"]),
                authored_decoy_doc_ids: ids(&["decoy"]),
            })
            .collect(),
    };
    (corpus, intents)
}

fn judged(intents: &IntentSet) -> Judgments {
    Judgments {
        version: 1,
        corpus_version: 1,
        status: "judged".to_string(),
        graders: ids(&["grader-a", "grader-b"]),
        grades: intents
            .intents
            .iter()
            .map(|intent| (intent.id.clone(), grades(&[("real", 3), ("decoy", 0)])))
            .collect(),
    }
}

#[test]
fn should_pass_the_floors_given_the_graded_document_ranked_first() {
    // Arrange
    let (_, intents) = tiny_workload();
    let results: BTreeMap<_, _> = intents
        .intents
        .iter()
        .map(|intent| (intent.id.clone(), ids(&["real", "decoy"])))
        .collect();

    // Act
    let evaluation = evaluate_held_out(&intents, &judged(&intents), &results).expect("evaluation");

    // Assert
    assert!(evaluation.meets_floors());
    assert_eq!(evaluation.per_intent.len(), 2);
}

#[test]
fn should_fail_the_floors_given_a_judged_irrelevant_decoy_ranked_first() {
    // Arrange: a mathematically valid ranking that a human graded as wrong must fail.
    let (_, intents) = tiny_workload();
    let results: BTreeMap<_, _> = intents
        .intents
        .iter()
        .map(|intent| (intent.id.clone(), ids(&["decoy", "real"])))
        .collect();

    // Act
    let evaluation = evaluate_held_out(&intents, &judged(&intents), &results).expect("evaluation");

    // Assert
    assert!(!evaluation.meets_floors());
    assert!((evaluation.mean_mrr - 0.5).abs() < 1e-12);
}

#[test]
fn should_reject_judgments_that_miss_an_answerable_intent_or_name_fewer_than_two_graders() {
    // Arrange
    let (corpus, intents) = tiny_workload();
    let mut missing = judged(&intents);
    missing.grades.remove("held-1");
    let mut single_grader = judged(&intents);
    single_grader.graders = ids(&["grader-a"]);

    // Act
    let missing_error = validate_judgments(&missing, &corpus, &intents);
    let grader_error = validate_judgments(&single_grader, &corpus, &intents);

    // Assert
    assert!(missing_error.is_err());
    assert!(grader_error.is_err());
    assert!(validate_judgments(&judged(&intents), &corpus, &intents).is_ok());
}

#[test]
fn should_reject_an_unjudged_file_that_carries_grades() {
    // Arrange
    let (corpus, intents) = tiny_workload();
    let mut sneaky = judged(&intents);
    "unjudged".clone_into(&mut sneaky.status);

    // Act
    let result = validate_judgments(&sneaky, &corpus, &intents);

    // Assert
    assert!(result.is_err());
}
