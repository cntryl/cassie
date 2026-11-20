#![allow(dead_code)]
//! Judged-relevance workload types, nDCG@10 / MRR@10 and the graded threshold evaluation.
//! See `tests/corpus/search_relevance/v1/README.md` for the human-facing protocol.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// Cutoff for both metrics.
pub const CUTOFF: usize = 10;
/// Minimum mean nDCG@10 and MRR@10 on held-out answerable intents. Fixed during qualification.
pub const NDCG_FLOOR: f64 = 0.80;
pub const MRR_FLOOR: f64 = 0.80;
/// A document counts as relevant for MRR at this grade or above.
pub const RELEVANT_GRADE: u8 = 2;
pub const MAX_GRADE: u8 = 3;

#[derive(Debug, Deserialize)]
pub struct Corpus {
    pub version: u32,
    pub documents: Vec<Document>,
}

#[derive(Debug, Deserialize)]
pub struct Document {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct IntentSet {
    pub version: u32,
    pub corpus_version: u32,
    pub intents: Vec<Intent>,
}

#[derive(Debug, Deserialize)]
pub struct Intent {
    pub id: String,
    pub split: String,
    pub kind: String,
    pub query: String,
    pub authored_intended_doc_ids: Vec<String>,
    #[serde(default)]
    pub authored_decoy_doc_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Judgments {
    pub version: u32,
    pub corpus_version: u32,
    pub status: String,
    pub graders: Vec<String>,
    pub grades: BTreeMap<String, BTreeMap<String, u8>>,
}

pub fn load_corpus() -> Corpus {
    serde_json::from_str(include_str!("../corpus/search_relevance/v1/corpus.json"))
        .expect("corpus.json is well formed")
}

pub fn load_intents() -> IntentSet {
    serde_json::from_str(include_str!("../corpus/search_relevance/v1/intents.json"))
        .expect("intents.json is well formed")
}

pub fn load_judgments() -> Judgments {
    serde_json::from_str(include_str!("../corpus/search_relevance/v1/judgments.json"))
        .expect("judgments.json is well formed")
}

fn gain(grade: u8) -> f64 {
    f64::from(2_u32.pow(u32::from(grade))) - 1.0
}

fn discount(rank_from_one: usize) -> f64 {
    ((rank_from_one + 1) as f64).log2().recip()
}

/// nDCG@k with gain `2^grade - 1`; ungraded documents have grade 0. Zero when nothing is graded
/// relevant (an unanswerable ideal).
pub fn ndcg_at_k(ranked: &[String], grades: &BTreeMap<String, u8>, k: usize) -> f64 {
    let dcg: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .map(|(index, id)| gain(grades.get(id).copied().unwrap_or(0)) * discount(index + 1))
        .sum();
    let mut ideal: Vec<u8> = grades.values().copied().collect();
    ideal.sort_unstable_by(|left, right| right.cmp(left));
    let ideal_dcg: f64 = ideal
        .iter()
        .take(k)
        .enumerate()
        .map(|(index, grade)| gain(*grade) * discount(index + 1))
        .sum();
    if ideal_dcg == 0.0 {
        0.0
    } else {
        dcg / ideal_dcg
    }
}

/// Reciprocal rank of the first document graded `RELEVANT_GRADE` or above within the top `k`.
pub fn mrr_at_k(ranked: &[String], grades: &BTreeMap<String, u8>, k: usize) -> f64 {
    ranked
        .iter()
        .take(k)
        .position(|id| grades.get(id).copied().unwrap_or(0) >= RELEVANT_GRADE)
        .map_or(0.0, |index| 1.0 / (index + 1) as f64)
}

/// Checks a judgments file against the workload. An `unjudged` file must be empty.
pub fn validate_judgments(
    judgments: &Judgments,
    corpus: &Corpus,
    intents: &IntentSet,
) -> Result<(), String> {
    if judgments.corpus_version != corpus.version || judgments.corpus_version != intents.corpus_version
    {
        return Err("judgments, intents and corpus must share one corpus version".to_string());
    }
    match judgments.status.as_str() {
        "unjudged" => {
            if judgments.grades.is_empty() && judgments.graders.is_empty() {
                Ok(())
            } else {
                Err("an unjudged file must not contain graders or grades".to_string())
            }
        }
        "judged" => validate_judged(judgments, corpus, intents),
        other => Err(format!("unknown judgments status '{other}'")),
    }
}

fn validate_judged(
    judgments: &Judgments,
    corpus: &Corpus,
    intents: &IntentSet,
) -> Result<(), String> {
    let graders: BTreeSet<_> = judgments.graders.iter().filter(|g| !g.trim().is_empty()).collect();
    if graders.len() < 2 {
        return Err("a judged file needs at least two distinct named graders".to_string());
    }
    let documents: BTreeSet<_> = corpus.documents.iter().map(|doc| doc.id.as_str()).collect();
    for intent in intents.intents.iter().filter(|i| i.split != "no_hit") {
        let Some(grades) = judgments.grades.get(&intent.id) else {
            return Err(format!("intent {} has no grades", intent.id));
        };
        if let Some(unknown) = grades.keys().find(|id| !documents.contains(id.as_str())) {
            return Err(format!("intent {} grades unknown document {unknown}", intent.id));
        }
        if grades.values().any(|grade| *grade > MAX_GRADE) {
            return Err(format!("intent {} has a grade above {MAX_GRADE}", intent.id));
        }
        if intent.split == "held_out" && !grades.values().any(|grade| *grade >= RELEVANT_GRADE) {
            return Err(format!("held-out intent {} has no relevant document", intent.id));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct IntentScore {
    pub intent_id: String,
    pub ndcg: f64,
    pub mrr: f64,
}

#[derive(Debug)]
pub struct Evaluation {
    pub per_intent: Vec<IntentScore>,
    pub mean_ndcg: f64,
    pub mean_mrr: f64,
}

impl Evaluation {
    pub fn meets_floors(&self) -> bool {
        self.mean_ndcg >= NDCG_FLOOR && self.mean_mrr >= MRR_FLOOR
    }
}

/// Scores the held-out answerable intents. `results` maps intent id to the ranked ids returned.
pub fn evaluate_held_out(
    intents: &IntentSet,
    judgments: &Judgments,
    results: &BTreeMap<String, Vec<String>>,
) -> Result<Evaluation, String> {
    let mut per_intent = Vec::new();
    for intent in intents.intents.iter().filter(|i| i.split == "held_out") {
        let grades = judgments
            .grades
            .get(&intent.id)
            .ok_or_else(|| format!("held-out intent {} has no grades", intent.id))?;
        let ranked = results
            .get(&intent.id)
            .ok_or_else(|| format!("no results recorded for {}", intent.id))?;
        per_intent.push(IntentScore {
            intent_id: intent.id.clone(),
            ndcg: ndcg_at_k(ranked, grades, CUTOFF),
            mrr: mrr_at_k(ranked, grades, CUTOFF),
        });
    }
    if per_intent.is_empty() {
        return Err("no held-out intents to evaluate".to_string());
    }
    let count = per_intent.len() as f64;
    Ok(Evaluation {
        mean_ndcg: per_intent.iter().map(|s| s.ndcg).sum::<f64>() / count,
        mean_mrr: per_intent.iter().map(|s| s.mrr).sum::<f64>() / count,
        per_intent,
    })
}
