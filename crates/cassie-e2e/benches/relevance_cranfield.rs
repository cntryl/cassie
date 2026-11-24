//! Opt-in Cranfield relevance evaluation (#1368). Needs the network on first run, so it is not
//! part of `cargo test`. Run with `cargo bench --locked --bench relevance_cranfield`.
//!
//! Loads the 1,400 Cranfield abstracts into an in-memory Cassie table with a full-text index,
//! runs every query through SQL `search_score` top-10, scores nDCG@10 and MRR@10 against the human
//! judgments, and compares each page with an independent BM25 reference. The process exits
//! non-zero when Cassie disagrees with the reference beyond the manifest's tolerance.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[path = "../tests/support/cranfield.rs"]
mod cranfield;
#[path = "support/cranfield_fetch.rs"]
mod cranfield_fetch;
#[path = "../tests/support/cranfield_reference.rs"]
mod cranfield_reference;
#[path = "../tests/support/relevance_metrics.rs"]
mod relevance_metrics;

const OUTPUT_DIR: &str = "target/relevance/cranfield";

struct Workload {
    documents: Vec<cranfield::CranDocument>,
    queries: Vec<cranfield::CranQuery>,
    qrels: cranfield::Qrels,
}

fn load_workload(manifest: &cranfield::Manifest) -> Result<Workload, String> {
    let collection = cranfield_fetch::ensure_collection(manifest, Path::new(OUTPUT_DIR))?;
    let workload = Workload {
        documents: cranfield::parse_documents(&collection.documents),
        queries: cranfield::parse_queries(&collection.queries),
        qrels: cranfield::parse_qrels(&collection.qrels)?,
    };
    let counts = &manifest.counts;
    let qrel_lines: usize = workload.qrels.values().map(BTreeMap::len).sum();
    if workload.documents.len() != counts.documents
        || workload.queries.len() != counts.queries
        || qrel_lines != counts.qrel_lines
    {
        return Err(format!(
            "parsed {} documents, {} queries and {qrel_lines} qrel lines; the manifest expects {}, {} and {}",
            workload.documents.len(),
            workload.queries.len(),
            counts.documents,
            counts.queries,
            counts.qrel_lines
        ));
    }
    Ok(workload)
}

fn open_cassie(documents: &[cranfield::CranDocument], index_options: &str) -> Cassie {
    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    let config = CassieRuntimeConfig::from_env().expect("runtime config");
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, "CREATE TABLE docs (body TEXT)", vec![])
        .expect("create table");
    let collection = cassie
        .catalog
        .get_schema("docs")
        .map_or_else(|| "docs".to_string(), |schema| schema.collection);
    for document in documents {
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
            &format!("CREATE INDEX docs_fts ON docs USING fulltext (body){index_options}"),
            vec![],
        )
        .expect("create full-text index");
    cassie
}

fn ranked_page(cassie: &Cassie, sql: &str, query: &cranfield::CranQuery) -> Vec<(String, f64)> {
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, sql, vec![Value::String(query.text.clone())])
        .unwrap_or_else(|error| panic!("query {}: {error}", query.number))
        .rows()
        .iter()
        .map(|row| match (&row[0], &row[1]) {
            (Value::String(id), Value::Float64(score)) => (id.clone(), *score),
            _ => panic!("query {} returned an unexpected row shape", query.number),
        })
        .collect()
}

fn ids(page: &[(String, f64)]) -> Vec<String> {
    page.iter().map(|(id, _)| id.clone()).collect()
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / relevance_metrics::count_f64(values.len())
}

/// Measured outcome of one analyzer configuration.
struct ConfigurationRun {
    summary: serde_json::Value,
    per_query: Vec<serde_json::Value>,
    full_agreement: usize,
    cassie_ndcg: f64,
    cassie_mrr: f64,
}

fn run_configuration(
    manifest: &cranfield::Manifest,
    workload: &Workload,
    configuration: &cranfield::AnalyzerConfiguration,
) -> ConfigurationRun {
    let contract = &manifest.evaluation;
    let cutoff = contract.cutoff;
    let relevant_grade = manifest.conventions.mrr_relevant_grade;
    let cassie = open_cassie(&workload.documents, &configuration.index_options);
    let reference = cranfield_reference::Reference::with_stemming(
        &workload
            .documents
            .iter()
            .map(|document| (document.id.clone(), document.indexed_text()))
            .collect::<Vec<_>>(),
        configuration.stemming,
    );
    let empty = BTreeMap::new();

    let mut per_query = Vec::new();
    let (mut cassie_ndcg, mut cassie_mrr) = (Vec::new(), Vec::new());
    let (mut reference_ndcg, mut reference_mrr) = (Vec::new(), Vec::new());
    let (mut full_agreement, mut max_difference) = (0_usize, 0.0_f64);
    for query in &workload.queries {
        let grades = workload.qrels.get(&query.number).unwrap_or(&empty);
        let actual = ranked_page(&cassie, &contract.sql, query);
        let expected = reference.top(&query.text, cutoff);
        let agreement = cranfield_reference::compare(&actual, &expected, contract.score_tolerance);
        full_agreement += usize::from(agreement.is_full());
        max_difference = max_difference.max(agreement.max_score_difference);
        let metrics = |page: &[(String, f64)]| {
            let ranked = ids(page);
            (
                relevance_metrics::ndcg_at_k(&ranked, grades, cutoff),
                relevance_metrics::mrr_at_k_with_threshold(&ranked, grades, cutoff, relevant_grade),
            )
        };
        let (ndcg, mrr) = metrics(&actual);
        let (reference_n, reference_m) = metrics(&expected);
        cassie_ndcg.push(ndcg);
        cassie_mrr.push(mrr);
        reference_ndcg.push(reference_n);
        reference_mrr.push(reference_m);
        per_query.push(serde_json::json!({
            "configuration": configuration.name,
            "query": query.number,
            "source_id": query.source_id,
            "cassie_top": ids(&actual),
            "reference_top": ids(&expected),
            "same_ids": agreement.same_ids,
            "max_score_difference": agreement.max_score_difference,
            "ndcg_at_10": ndcg,
            "mrr_at_10": mrr,
            "reference_ndcg_at_10": reference_n,
            "reference_mrr_at_10": reference_m,
            "judged_count": grades.values().filter(|grade| **grade > 0).count(),
            "relevant_count": grades.values().filter(|grade| **grade >= relevant_grade).count(),
        }));
    }
    let (mean_ndcg, mean_mrr) = (mean(&cassie_ndcg), mean(&cassie_mrr));
    let summary = serde_json::json!({
        "configuration": configuration.name,
        "index_options": configuration.index_options,
        "queries": workload.queries.len(),
        "cutoff": cutoff,
        "cassie": { "mean_ndcg_at_10": mean_ndcg, "mean_mrr_at_10": mean_mrr },
        "reference": {
            "mean_ndcg_at_10": mean(&reference_ndcg),
            "mean_mrr_at_10": mean(&reference_mrr),
        },
        "floors": {
            "min_mean_ndcg_at_10": configuration.min_mean_ndcg_at_10,
            "min_mean_mrr_at_10": configuration.min_mean_mrr_at_10,
        },
        "queries_with_full_agreement": full_agreement,
        "max_score_difference": max_difference,
        "score_tolerance": contract.score_tolerance,
    });
    ConfigurationRun {
        summary,
        per_query,
        full_agreement,
        cassie_ndcg: mean_ndcg,
        cassie_mrr: mean_mrr,
    }
}

/// Tier 1 parity and tier 2 floors for one configuration.
fn configuration_failures(
    configuration: &cranfield::AnalyzerConfiguration,
    run: &ConfigurationRun,
    queries: usize,
) -> Vec<String> {
    let mut failures = Vec::new();
    let name = &configuration.name;
    if run.full_agreement != queries {
        failures.push(format!(
            "{name}: Cassie matched the independent BM25 reference on {} of {queries} queries",
            run.full_agreement
        ));
    }
    if run.cassie_ndcg < configuration.min_mean_ndcg_at_10 {
        failures.push(format!(
            "{name}: mean nDCG@10 {:.6} is below the pinned floor {}",
            run.cassie_ndcg, configuration.min_mean_ndcg_at_10
        ));
    }
    if run.cassie_mrr < configuration.min_mean_mrr_at_10 {
        failures.push(format!(
            "{name}: mean MRR@10 {:.6} is below the pinned floor {}",
            run.cassie_mrr, configuration.min_mean_mrr_at_10
        ));
    }
    failures
}

fn main() -> ExitCode {
    let manifest = cranfield::load_manifest();
    let workload = match load_workload(&manifest) {
        Ok(workload) => workload,
        Err(error) => {
            eprintln!("cranfield evaluation could not start: {error}");
            return ExitCode::FAILURE;
        }
    };
    let queries = workload.queries.len();
    let mut summaries = Vec::new();
    let mut per_query = Vec::new();
    let mut failures = Vec::new();
    let mut means = BTreeMap::new();
    for configuration in &manifest.evaluation.configurations {
        let run = run_configuration(&manifest, &workload, configuration);
        failures.extend(configuration_failures(configuration, &run, queries));
        means.insert(
            configuration.name.clone(),
            (run.cassie_ndcg, run.cassie_mrr),
        );
        summaries.push(run.summary);
        per_query.extend(run.per_query);
    }
    // Tier 3: the stemming configuration must not rank worse than the default.
    if let (Some(default), Some(stemmed)) = (means.get("default"), means.get("english_stemming")) {
        if stemmed.0 < default.0 || stemmed.1 < default.1 {
            failures.push(format!(
                "english_stemming (nDCG@10 {:.6}, MRR@10 {:.6}) scores below default (nDCG@10 {:.6}, MRR@10 {:.6})",
                stemmed.0, stemmed.1, default.0, default.1
            ));
        }
    }

    let summary = serde_json::json!({
        "configurations": summaries,
        "archive_sha256": manifest.source.archive_sha256,
        "failures": failures,
    });
    let results = serde_json::json!({ "summary": summary, "per_query": per_query });
    let path = Path::new(OUTPUT_DIR).join("results.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&results).expect("serialize results"),
    )
    .expect("write results");

    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("serialize summary")
    );
    println!("per-query results: {}", path.display());
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        for failure in &failures {
            eprintln!("FAIL: {failure}");
        }
        ExitCode::FAILURE
    }
}
