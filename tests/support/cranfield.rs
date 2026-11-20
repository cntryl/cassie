#![allow(dead_code)]
//! Cranfield collection manifest and parsers (#1368), shared by the network-free tests and the
//! opt-in runner `benches/relevance_cranfield.rs`.
//!
//! The parsers mirror `ir_datasets` v0.5.11 `cranfield.py` (MIT). The collection itself is never
//! committed: it has no published license. See `tests/corpus/search_relevance/cranfield/`.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub source: Source,
    pub redistribution: Redistribution,
    pub citation: String,
    pub counts: Counts,
    pub conventions: Conventions,
    pub evaluation: EvaluationContract,
}

#[derive(Debug, Deserialize)]
pub struct Source {
    pub url: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub members: Members,
}

#[derive(Debug, Deserialize)]
pub struct Members {
    pub documents: String,
    pub queries: String,
    pub qrels: String,
}

#[derive(Debug, Deserialize)]
pub struct Redistribution {
    pub redistributed: bool,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct Counts {
    pub documents: usize,
    pub queries: usize,
    pub qrel_lines: usize,
}

#[derive(Debug, Deserialize)]
pub struct Conventions {
    pub relevance_levels: BTreeMap<String, String>,
    pub level_to_grade: BTreeMap<String, u8>,
    pub mrr_relevant_grade: u8,
}

#[derive(Debug, Deserialize)]
pub struct EvaluationContract {
    pub cutoff: usize,
    pub sql: String,
    pub bm25: Bm25Contract,
    pub score_tolerance: f64,
    pub contract: String,
    pub configurations: Vec<AnalyzerConfiguration>,
}

/// One analyzer configuration the evaluation runs, with its pinned no-regression floors.
#[derive(Debug, Deserialize)]
pub struct AnalyzerConfiguration {
    pub name: String,
    pub index_options: String,
    pub stemming: bool,
    pub min_mean_ndcg_at_10: f64,
    pub min_mean_mrr_at_10: f64,
}

#[derive(Debug, Deserialize)]
pub struct Bm25Contract {
    pub k1: f64,
    pub b: f64,
}

/// Loads the committed manifest.
pub fn load_manifest() -> Manifest {
    serde_json::from_str(include_str!(
        "../corpus/search_relevance/cranfield/manifest.json"
    ))
    .expect("cranfield manifest.json is well formed")
}

/// Grade assigned to Cranfield relevance level `level`, or `None` for a level the collection does
/// not define. Level 1 (complete answer) is the strongest and maps to the highest grade; level -1
/// (no interest) maps to 0, the same as an unjudged document.
pub fn grade_for_level(level: i32) -> Option<u8> {
    match level {
        1 => Some(4),
        2 => Some(3),
        3 => Some(2),
        4 => Some(1),
        -1 => Some(0),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CranDocument {
    pub id: String,
    pub title: String,
    pub author: String,
    pub bib: String,
    pub text: String,
}

impl CranDocument {
    /// The indexed text: title and abstract, the `ir_datasets` `default_text`.
    pub fn indexed_text(&self) -> String {
        format!("{} {}", self.title, self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CranQuery {
    /// 1-based position in the queries file; this is the number `cranqrel` uses.
    pub number: usize,
    /// The `.I` value written in the queries file, which skips numbers.
    pub source_id: String,
    pub text: String,
}

/// Splits `input` at lines starting with `.I `; each group is the header remainder followed by the
/// lines up to the next header.
fn records(input: &str) -> Vec<Vec<&str>> {
    let mut groups: Vec<Vec<&str>> = Vec::new();
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix(".I ") {
            groups.push(vec![rest]);
        } else if let Some(group) = groups.last_mut() {
            group.push(line);
        }
    }
    groups
}

fn field_for_marker(line: &str) -> Option<&'static str> {
    [
        (".T", "title"),
        (".A", "author"),
        (".B", "bib"),
        (".W", "text"),
    ]
    .into_iter()
    .find(|(marker, _)| line.starts_with(marker))
    .map(|(_, field)| field)
}

/// Parses documents. A marker line (`.T`, `.A`, `.B`, `.W`) switches field; every other line
/// belongs to the current field, starting with the id from the `.I` header.
pub fn parse_documents(input: &str) -> Vec<CranDocument> {
    records(input)
        .into_iter()
        .map(|lines| {
            let mut fields: BTreeMap<&str, String> = BTreeMap::new();
            let mut field = "id";
            for (index, line) in lines.iter().enumerate() {
                if let Some(next) = field_for_marker(line).filter(|_| index > 0) {
                    field = next;
                } else {
                    let entry = fields.entry(field).or_default();
                    entry.push_str(line);
                    entry.push('\n');
                }
            }
            let take = |name: &str| {
                fields
                    .get(name)
                    .map_or_else(String::new, |v| v.trim().to_string())
            };
            CranDocument {
                id: take("id"),
                title: take("title"),
                author: take("author"),
                bib: take("bib"),
                text: take("text"),
            }
        })
        .collect()
}

/// Parses queries. The number is the 1-based position, not the `.I` value, because `cranqrel`
/// refers to queries by position.
pub fn parse_queries(input: &str) -> Vec<CranQuery> {
    records(input)
        .into_iter()
        .enumerate()
        .map(|(position, lines)| {
            let mut text = String::new();
            let mut in_text = false;
            for line in lines.iter().skip(1) {
                if line.starts_with(".W") {
                    in_text = true;
                } else if in_text {
                    text.push_str(line);
                    text.push('\n');
                }
            }
            CranQuery {
                number: position + 1,
                source_id: lines[0].trim().to_string(),
                text: text.trim().to_string(),
            }
        })
        .collect()
}

/// Graded judgments per query number: document id to grade. Lines that are not exactly three
/// columns, or whose level is undefined, are errors.
pub type Qrels = BTreeMap<usize, BTreeMap<String, u8>>;

/// # Errors
///
/// Returns a message naming the offending line.
pub fn parse_qrels(input: &str) -> Result<Qrels, String> {
    let mut qrels = Qrels::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let columns: Vec<&str> = line.split_whitespace().collect();
        let [query, document, level] = columns[..] else {
            return Err(format!(
                "qrels line {} has {} columns, expected 3",
                index + 1,
                columns.len()
            ));
        };
        let query: usize = query
            .parse()
            .map_err(|_| format!("qrels line {} has a non-numeric query", index + 1))?;
        let level: i32 = level
            .parse()
            .map_err(|_| format!("qrels line {} has a non-numeric level", index + 1))?;
        let grade = grade_for_level(level)
            .ok_or_else(|| format!("qrels line {} has undefined level {level}", index + 1))?;
        qrels
            .entry(query)
            .or_default()
            .insert(document.to_string(), grade);
    }
    Ok(qrels)
}
