#![allow(dead_code)]
//! Two-tenant ranked search fixture, independent corpus-wide BM25 oracle and a pgwire client.
//!
//! The oracle restates the documented BM25 contract with no Cassie code: statistics (document
//! count, document frequency, average length) always cover the whole collection, never only the
//! rows a tenant or status predicate admits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::NoTls;

pub const TOTAL_DOCUMENTS: usize = 200;
const K1: f64 = 1.2;
const B: f64 = 0.75;
pub const QUERY: &str = "harbor cobalt";
/// Score and vector tolerance between engine floats and the independent f64 oracle.
pub const SCORE_TOLERANCE: f64 = 1e-9;
pub const HYBRID_TOLERANCE: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct TenantDoc {
    pub id: String,
    pub tenant: &'static str,
    pub status: &'static str,
    pub body: String,
    pub embedding: [f64; 2],
}

impl TenantDoc {
    pub fn tokens(&self) -> Vec<&str> {
        self.body.split_whitespace().collect()
    }

    pub fn eligible(&self) -> bool {
        self.tenant == "a" && self.status == "open"
    }
}

/// Builds the versioned two-tenant corpus. The first `eligible` documents are tenant `a` / `open`
/// (the reader's eligible set); the next 20 are `a` / `closed`, then 20 `b` / `open`, and the rest
/// `b` / `closed`. Tenant `b` repeats the query terms more often, so it dominates global
/// popularity and the nearest vectors, while tenant `a` shares every term.
pub fn tenant_corpus(eligible: usize) -> Vec<TenantDoc> {
    (0..TOTAL_DOCUMENTS)
        .map(|index| {
            let (tenant, status) = if index < eligible {
                ("a", "open")
            } else if index < eligible + 20 {
                ("a", "closed")
            } else if index < eligible + 40 {
                ("b", "open")
            } else {
                ("b", "closed")
            };
            let popular = tenant == "b";
            let harbor = if index % 5 == 4 {
                0
            } else if popular {
                3 + index % 3
            } else {
                1 + index % 3
            };
            let cobalt = usize::from(if popular {
                index % 2 == 0
            } else {
                index % 4 == 0
            });
            let length = 12 + (index % 2) * 2;
            let mut words = vec!["harbor"; harbor];
            words.extend(vec!["cobalt"; cobalt]);
            while words.len() < length {
                words.push("pad");
            }
            let embedding = if popular {
                [1.0, 0.0]
            } else {
                [0.5 + 0.125 * (index % 4) as f64, 0.25]
            };
            TenantDoc {
                id: format!("d{index:03}"),
                tenant,
                status,
                body: words.join(" "),
                embedding,
            }
        })
        .collect()
}

/// Corpus-wide BM25 by the documented formula, summed over distinct query terms.
pub fn bm25_scores(corpus: &[TenantDoc], terms: &[&str]) -> BTreeMap<String, f64> {
    let total = corpus.len() as f64;
    let lengths: Vec<f64> = corpus.iter().map(|doc| doc.tokens().len() as f64).collect();
    let average = (lengths.iter().sum::<f64>() / total).max(1.0);
    let mut scores = BTreeMap::new();
    for term in terms.iter().collect::<BTreeSet<_>>() {
        let df = corpus
            .iter()
            .filter(|doc| doc.tokens().contains(term))
            .count() as f64;
        if df == 0.0 {
            continue;
        }
        let idf = ((total - df + 0.5) / (df + 0.5) + 1.0).ln();
        for (doc, length) in corpus.iter().zip(&lengths) {
            let tf = doc.tokens().iter().filter(|token| *token == term).count() as f64;
            if tf > 0.0 {
                let norm = tf + K1 * (1.0 - B + B * length / average);
                *scores.entry(doc.id.clone()).or_insert(0.0) += idf * tf * (K1 + 1.0) / norm;
            }
        }
    }
    scores
}

/// The documented hybrid contract: `0.65 * bm25 + 0.35 * 1 / (1 + L2)`, rounded to 12 places.
pub fn hybrid_scores(
    corpus: &[TenantDoc],
    terms: &[&str],
    query_vector: [f64; 2],
) -> BTreeMap<String, f64> {
    let lexical = bm25_scores(corpus, terms);
    corpus
        .iter()
        .filter_map(|doc| {
            let text = lexical.get(&doc.id)?;
            let distance = ((doc.embedding[0] - query_vector[0]).powi(2)
                + (doc.embedding[1] - query_vector[1]).powi(2))
            .sqrt();
            let score = text * 0.65 + 0.35 / (1.0 + distance);
            Some((doc.id.clone(), (score * 1e12).round() / 1e12))
        })
        .collect()
}

/// Orders eligible scored documents by score descending then `_id` ascending and applies a page.
pub fn ranked_page(
    scores: &BTreeMap<String, f64>,
    corpus: &[TenantDoc],
    eligible: impl Fn(&TenantDoc) -> bool,
    limit: usize,
    offset: usize,
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = corpus
        .iter()
        .filter(|doc| eligible(doc))
        .filter_map(|doc| scores.get(&doc.id).map(|score| (doc.id.clone(), *score)))
        .collect();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().skip(offset).take(limit).collect()
}

/// Asserts IDs and order exactly and scores within `tolerance`.
pub fn assert_page_matches(actual: &[(String, f64)], expected: &[(String, f64)], tolerance: f64) {
    assert_eq!(
        actual.iter().map(|row| &row.0).collect::<Vec<_>>(),
        expected.iter().map(|row| &row.0).collect::<Vec<_>>(),
        "ids and order must equal the independent oracle"
    );
    for (got, want) in actual.iter().zip(expected) {
        assert!(
            (got.1 - want.1).abs() <= tolerance,
            "score for {} was {} but the oracle expects {}",
            got.0,
            got.1,
            want.1
        );
    }
}

/// Search counters read before and after one request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchCounters {
    pub retrieval_stage_queries: u64,
    pub candidates: u64,
    pub results: u64,
    pub posting_reads: u64,
    pub row_fetches: u64,
    pub row_scan_fallbacks: u64,
}

impl SearchCounters {
    pub fn read(cassie: &Cassie) -> Self {
        let metrics = cassie.metrics();
        let search = &metrics["search"];
        let field = |name: &str| search[name].as_u64().unwrap_or(0);
        Self {
            retrieval_stage_queries: field("retrieval_stage_queries_total"),
            candidates: field("candidate_count_total"),
            results: field("result_count_total"),
            posting_reads: field("posting_reads_total"),
            row_fetches: field("candidate_row_fetches_total"),
            row_scan_fallbacks: field("row_scan_fallback_total"),
        }
    }

    pub fn since(self, before: Self) -> Self {
        Self {
            retrieval_stage_queries: self.retrieval_stage_queries - before.retrieval_stage_queries,
            candidates: self.candidates - before.candidates,
            results: self.results - before.results,
            posting_reads: self.posting_reads - before.posting_reads,
            row_fetches: self.row_fetches - before.row_fetches,
            row_scan_fallbacks: self.row_scan_fallbacks - before.row_scan_fallbacks,
        }
    }
}

pub fn fallback_reason_count(cassie: &Cassie, reason: &str) -> u64 {
    cassie.metrics()["search"]["retrieval_fallback_reasons"][reason]
        .as_u64()
        .unwrap_or(0)
}

/// An owned-listener pgwire server over one Cassie plus a prepared-statement client.
pub struct WireFixture {
    pub cassie: Cassie,
    pub client: tokio_postgres::Client,
    server: tokio::task::JoinHandle<()>,
    connection: tokio::task::JoinHandle<()>,
}

impl WireFixture {
    pub async fn start(cassie: Cassie) -> Self {
        let config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let server_cassie = cassie.clone();
        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run_with_listener(
                listener,
                Arc::new(server_cassie),
                config,
            )
            .await;
        });
        let mut client_config = tokio_postgres::Config::new();
        client_config.host("127.0.0.1");
        client_config.port(port);
        client_config.user("root");
        client_config.password(crate::support_sql::test_root_password());
        client_config.dbname("postgres");
        let (client, connection) = client_config.connect(NoTls).await.expect("connect");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });
        Self {
            cassie,
            client,
            server,
            connection,
        }
    }

    pub async fn stop(self) {
        drop(self.client);
        self.connection.abort();
        self.server.abort();
        let _ = self.connection.await;
        let _ = self.server.await;
    }

    pub async fn exec(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|error| {
                let detail = error
                    .as_db_error()
                    .map_or_else(|| error.to_string(), |db| db.message().to_string());
                panic!("{}: {detail}", sql.chars().take(120).collect::<String>())
            });
    }

    /// Creates `docs`, loads `corpus` and builds the full-text and scalar indexes.
    pub async fn load_corpus(&self, corpus: &[TenantDoc]) {
        load_corpus_in_process(&self.cassie, corpus);
    }

    /// Prepares `sql` with TEXT parameters followed by INT8 parameters, binds them over the
    /// extended protocol and returns `(_id, score)` rows.
    pub async fn scored(&self, sql: &str, texts: &[&str], numbers: &[i64]) -> Vec<(String, f64)> {
        let types: Vec<Type> = texts
            .iter()
            .map(|_| Type::TEXT)
            .chain(numbers.iter().map(|_| Type::INT8))
            .collect();
        let statement = self
            .client
            .prepare_typed(sql, &types)
            .await
            .unwrap_or_else(|error| panic!("prepare {sql}: {}", describe(&error)));
        let params: Vec<&(dyn ToSql + Sync)> = texts
            .iter()
            .map(|text| text as &(dyn ToSql + Sync))
            .chain(numbers.iter().map(|number| number as &(dyn ToSql + Sync)))
            .collect();
        self.client
            .query(&statement, &params)
            .await
            .unwrap_or_else(|error| panic!("execute {sql}: {}", describe(&error)))
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, f64>(1)))
            .collect()
    }
}

fn describe(error: &tokio_postgres::Error) -> String {
    error
        .as_db_error()
        .map_or_else(|| error.to_string(), |db| db.message().to_string())
}

/// Runs one statement through an in-process root session (no listener), for fixture phases that
/// must release storage before a reopen.
pub fn exec_in_process(cassie: &Cassie, sql: &str) {
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(&session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

/// Creates `docs`, loads `corpus` and builds the full-text and scalar indexes in process.
///
/// `_id` cannot be supplied through SQL, so documents are written with explicit identities to
/// make the `_id ASC` tie order deterministic and checkable.
pub fn load_corpus_in_process(cassie: &Cassie, corpus: &[TenantDoc]) {
    crate::support_sql::install_vector_extension(cassie);
    exec_in_process(
        cassie,
        "CREATE TABLE docs (tenant TEXT, status TEXT, body TEXT, embedding VECTOR(2))",
    );
    let collection = crate::support_sql::canonical_test_collection(cassie, "docs");
    for doc in corpus {
        cassie
            .midge
            .put_document(
                &collection,
                Some(doc.id.clone()),
                serde_json::json!({
                    "tenant": doc.tenant,
                    "status": doc.status,
                    "body": doc.body,
                    "embedding": doc.embedding,
                }),
            )
            .expect("write document");
    }
    exec_in_process(cassie, "CREATE INDEX docs_fts ON docs USING fulltext (body)");
    exec_in_process(cassie, "CREATE INDEX docs_tenant ON docs (tenant)");
    exec_in_process(cassie, "CREATE INDEX docs_status ON docs (status)");
}
