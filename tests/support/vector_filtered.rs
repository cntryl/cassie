#![allow(dead_code)]
//! Tenant-filtered vector fixture, independent exact oracle and counter helpers (#1363).
//!
//! Layout: tenant `b` rows ("foreign") occupy the ranks nearest the query vector, so every
//! nearest-neighbor window starts with foreign rows. Tenant `a` rows ("eligible") continue the
//! same band, so the first eligible neighbor ranks after the foreign block. The oracle ranks by
//! the true distances, so rankings have no ties.

use std::collections::BTreeMap;

use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

/// Foreign rows that rank before any eligible row (larger than the first ANN window).
const NEAR_FOREIGN: usize = 1_000;
const JITTER_FREQUENCY: f32 = 1.7;
const GOLDEN_ANGLE: f32 = 2.399_963;
pub const TOTAL_ROWS: usize = 4_000;
pub const QUERY: [f32; 2] = [1.0, 0.0];
pub const QUERY_LITERAL: &str = "[1,0]";
/// Largest first ANN window used for a ten-row page (`OFFSET + LIMIT = 10`).
pub const FIRST_WINDOW_FOR_TEN: usize = 160;
/// Declared recall@k floor for approximate profiles (`docs/performance-contracts.md`).
pub const RECALL_FLOOR: f64 = 0.90;
pub const DISTANCE_TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexKind {
    Hnsw,
    IvfFlat,
    None,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub label: String,
    pub tenant: &'static str,
    pub n: i64,
    pub embedding: [f32; 2],
}

fn count_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).expect("count fits in u16"))
}

fn splitmix64(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Ranks (distance order from the query) held by eligible and by foreign rows.
fn rank_layout(total: usize, eligible: usize) -> (Vec<usize>, Vec<usize>) {
    let near = (total - eligible).min(NEAR_FOREIGN);
    // Eligible ranks beyond the foreign block are chosen by a fixed integer hash, so they are
    // spread without any regular structure that would correlate with the spiral arms.
    let mut positions: Vec<usize> = (near..total).collect();
    positions.sort_by_key(|position| splitmix64(*position as u64));
    let mut eligible_ranks: Vec<usize> = positions.into_iter().take(eligible).collect();
    eligible_ranks.sort_unstable();
    let foreign_ranks = (0..total)
        .filter(|rank| eligible_ranks.binary_search(rank).is_err())
        .collect();
    (eligible_ranks, foreign_ranks)
}

/// `IVFFlat` layout: a thin band around the unit-circle arc starting at the query. Rank `r` lies
/// at angle `0.0002 * (r + 1)` with a small radial jitter, so angular lists align with distance.
fn arc_point(rank: usize) -> [f32; 2] {
    let angle = 0.0002 * (count_f32(rank) + 1.0);
    let radius = 1.0 + 0.001 * (JITTER_FREQUENCY * count_f32(rank)).sin();
    [radius * angle.cos(), radius * angle.sin()]
}

/// HNSW layout: a uniform-density sunflower disc centred on the query. Rank `r` lies at radius
/// `0.5 * sqrt((r + 1) / TOTAL_ROWS)`, so distance order follows rank while the point density is
/// constant (a graph over a density gradient leaves the sparse outer region unreachable).
fn spiral_point(rank: usize) -> [f32; 2] {
    let radius = 0.5 * ((count_f32(rank) + 1.0) / count_f32(TOTAL_ROWS)).sqrt();
    let angle = GOLDEN_ANGLE * count_f32(rank);
    [1.0 + radius * angle.cos(), radius * angle.sin()]
}

/// `total` rows of which `eligible` belong to tenant `a`, spread evenly through the id space.
///
/// Nearest `NEAR_FOREIGN` ranks are all foreign, so no eligible row is within the first ANN
/// window; farther ranks interleave eligible and foreign rows evenly. The oracle ranks by the
/// true distances. See [`arc_point`] and [`spiral_point`] for the per-index geometry.
pub fn corpus_for(kind: IndexKind, total: usize, eligible: usize) -> Vec<Row> {
    let mut labelled: Vec<usize> = (0..total).collect();
    labelled.sort_by_key(|index| splitmix64(*index as u64 ^ 0xA5A5_0000));
    labelled.truncate(eligible);
    let eligible_labels: std::collections::BTreeSet<usize> = labelled.into_iter().collect();
    let (mut eligible_ranks, mut foreign_ranks) = rank_layout(total, eligible);
    // Insertion order must not correlate with distance: an HNSW graph built from vectors inserted
    // in distance order is pathologically poorly connected.
    eligible_ranks.sort_by_key(|rank| splitmix64(*rank as u64 ^ 0x5EED_0001));
    foreign_ranks.sort_by_key(|rank| splitmix64(*rank as u64 ^ 0x5EED_0002));
    let (mut eligible_seen, mut foreign_seen) = (0usize, 0usize);
    (0..total)
        .map(|index| {
            let is_eligible = eligible_labels.contains(&index);
            let (ordinal, rank) = if is_eligible {
                eligible_seen += 1;
                (eligible_seen - 1, eligible_ranks[eligible_seen - 1])
            } else {
                foreign_seen += 1;
                (foreign_seen - 1, foreign_ranks[foreign_seen - 1])
            };
            let embedding = if kind == IndexKind::IvfFlat {
                arc_point(rank)
            } else {
                spiral_point(rank)
            };
            Row {
                label: format!("v{index:05}"),
                tenant: if is_eligible { "a" } else { "b" },
                n: i64::try_from(ordinal % 10).expect("small residue"),
                embedding,
            }
        })
        .collect()
}

pub fn is_tenant_a(row: &Row) -> bool {
    row.tenant == "a"
}

fn l2(embedding: [f32; 2]) -> f64 {
    embedding
        .iter()
        .zip(QUERY)
        .map(|(left, right)| (f64::from(*left) - f64::from(right)).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Exact filtered ranking: distance ascending then label ascending, paged.
pub fn oracle_page(
    rows: &[Row],
    eligible: impl Fn(&Row) -> bool,
    limit: usize,
    offset: usize,
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = rows
        .iter()
        .filter(|row| eligible(row))
        .map(|row| (row.label.clone(), l2(row.embedding)))
        .collect();
    ranked.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.into_iter().skip(offset).take(limit).collect()
}

/// Labels of the `window` rows nearest to the query over the whole collection.
pub fn nearest_labels(rows: &[Row], window: usize) -> Vec<String> {
    oracle_page(rows, |_| true, window, 0)
        .into_iter()
        .map(|(label, _)| label)
        .collect()
}

pub fn uncached_config() -> CassieRuntimeConfig {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config
}

pub struct Fixture {
    pub cassie: Cassie,
    pub session: CassieSession,
    pub rows: Vec<Row>,
}

/// Builds the table, loads `rows`, then creates the scalar and vector indexes.
pub fn open(rows: Vec<Row>, index: IndexKind, config: CassieRuntimeConfig) -> Fixture {
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    crate::support_sql::install_vector_extension(&cassie);
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    let fixture = Fixture {
        cassie,
        session,
        rows,
    };
    fixture.exec("CREATE TABLE docs (label TEXT, tenant TEXT, n INT, embedding VECTOR(2))");
    let collection = crate::support_sql::canonical_test_collection(&fixture.cassie, "docs");
    let documents = fixture
        .rows
        .iter()
        .map(|row| {
            (
                Some(row.label.clone()),
                serde_json::json!({
                    "label": row.label,
                    "tenant": row.tenant,
                    "n": row.n,
                    "embedding": row.embedding,
                }),
            )
        })
        .collect();
    fixture
        .cassie
        .midge
        .put_fresh_documents(&collection, documents)
        .expect("seed corpus");
    fixture.exec("CREATE INDEX docs_tenant ON docs (tenant)");
    match index {
        IndexKind::Hnsw => fixture.exec(
            "CREATE INDEX docs_vec ON docs USING hnsw (embedding vector_l2_ops) \
             WITH (m = 16, ef_construction = 200)",
        ),
        IndexKind::IvfFlat => fixture.exec(
            "CREATE INDEX docs_vec ON docs USING ivfflat (embedding vector_l2_ops) \
             WITH (lists = 16)",
        ),
        IndexKind::None => {}
    }
    fixture
}

impl Fixture {
    pub fn exec(&self, sql: &str) {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    /// Runs a `(label, distance)` or `(_id, score)` page query.
    pub fn page(&self, sql: &str) -> Vec<(String, f64)> {
        let result = self
            .cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
        result
            .rows()
            .iter()
            .map(|row| {
                let Value::String(label) = &row[0] else {
                    panic!("first column must be text");
                };
                let Value::Float64(score) = row[1] else {
                    panic!("second column must be a float");
                };
                (label.clone(), score)
            })
            .collect()
    }

    pub fn explain(&self, sql: &str) -> String {
        let result = self
            .cassie
            .execute_sql(&self.session, &format!("EXPLAIN {sql}"), vec![])
            .unwrap_or_else(|error| panic!("EXPLAIN {sql}: {error}"));
        let Value::String(plan) = &result.rows()[0][0] else {
            panic!("plan must be text");
        };
        plan.clone()
    }
}

/// The pgvector operator shape over `WHERE`.
pub fn operator_sql(filter: &str, limit: usize, offset: usize) -> String {
    format!(
        "SELECT label, embedding <-> '{QUERY_LITERAL}' AS distance FROM docs WHERE {filter} \
         ORDER BY embedding <-> '{QUERY_LITERAL}' LIMIT {limit} OFFSET {offset}"
    )
}

/// The `vector_distance` function shape over `WHERE`.
pub fn function_sql(filter: &str, limit: usize, offset: usize) -> String {
    format!(
        "SELECT _id, vector_distance(embedding, '{QUERY_LITERAL}') AS score FROM docs \
         WHERE {filter} ORDER BY vector_distance(embedding, '{QUERY_LITERAL}') \
         LIMIT {limit} OFFSET {offset}"
    )
}

/// Filtered-vector and exact-scan counters read before and after one statement.
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub filtered_ann: u64,
    pub standalone_bounded_exact: u64,
    pub hybrid_bounded_exact: u64,
    pub ann_windows: u64,
    pub ann_candidates: u64,
    pub eligible_candidates: u64,
    pub row_fetches: u64,
    pub eligible_subset: u64,
    pub exact_scan_rows: u64,
    pub hnsw_executions: u64,
    pub ivfflat_executions: u64,
    pub hybrid_row_scan_fallbacks: u64,
    pub fallbacks: BTreeMap<String, u64>,
}

impl Counters {
    pub fn read(cassie: &Cassie) -> Self {
        let metrics = cassie.metrics();
        let filtered = &metrics["vector_filtered"];
        let field = |name: &str| filtered[name].as_u64().unwrap_or(0);
        let path = |name: &str| filtered["paths"][name].as_u64().unwrap_or(0);
        let fallbacks =
            filtered["fallback_reasons"]
                .as_object()
                .map_or_else(BTreeMap::new, |reasons| {
                    reasons
                        .iter()
                        .map(|(reason, count)| (reason.clone(), count.as_u64().unwrap_or(0)))
                        .collect()
                });
        Self {
            filtered_ann: path("standalone.filtered-ann"),
            standalone_bounded_exact: path("standalone.bounded-exact"),
            hybrid_bounded_exact: path("hybrid.bounded-exact"),
            ann_windows: field("ann_windows_total"),
            ann_candidates: field("ann_candidates_total"),
            eligible_candidates: field("eligible_candidates_total"),
            row_fetches: field("row_fetches_total"),
            eligible_subset: field("eligible_subset_total"),
            exact_scan_rows: metrics["vector"]["prefilter_input_candidate_count_total"]
                .as_u64()
                .unwrap_or(0),
            hnsw_executions: metrics["vector"]["hnsw_executions"].as_u64().unwrap_or(0),
            ivfflat_executions: metrics["vector"]["ivfflat_executions"]
                .as_u64()
                .unwrap_or(0),
            hybrid_row_scan_fallbacks: metrics["hybrid"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or(0),
            fallbacks,
        }
    }

    pub fn since(&self, before: &Self) -> Self {
        let mut fallbacks = self.fallbacks.clone();
        for (reason, count) in &mut fallbacks {
            *count -= before.fallbacks.get(reason).copied().unwrap_or(0);
        }
        fallbacks.retain(|_, count| *count > 0);
        Self {
            filtered_ann: self.filtered_ann - before.filtered_ann,
            standalone_bounded_exact: self.standalone_bounded_exact
                - before.standalone_bounded_exact,
            hybrid_bounded_exact: self.hybrid_bounded_exact - before.hybrid_bounded_exact,
            ann_windows: self.ann_windows - before.ann_windows,
            ann_candidates: self.ann_candidates - before.ann_candidates,
            eligible_candidates: self.eligible_candidates - before.eligible_candidates,
            row_fetches: self.row_fetches - before.row_fetches,
            eligible_subset: self.eligible_subset - before.eligible_subset,
            exact_scan_rows: self.exact_scan_rows - before.exact_scan_rows,
            hnsw_executions: self.hnsw_executions - before.hnsw_executions,
            ivfflat_executions: self.ivfflat_executions - before.ivfflat_executions,
            hybrid_row_scan_fallbacks: self.hybrid_row_scan_fallbacks
                - before.hybrid_row_scan_fallbacks,
            fallbacks,
        }
    }
}

/// Runs `sql` and returns the page plus the counters it produced.
pub fn measured(fixture: &Fixture, sql: &str) -> (Vec<(String, f64)>, Counters) {
    let before = Counters::read(&fixture.cassie);
    let page = fixture.page(sql);
    (page, Counters::read(&fixture.cassie).since(&before))
}

/// Fraction of the oracle's labels present in `actual`.
pub fn recall(actual: &[(String, f64)], expected: &[(String, f64)]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let found = expected
        .iter()
        .filter(|(label, _)| actual.iter().any(|(candidate, _)| candidate == label))
        .count();
    f64::from(u32::try_from(found).expect("count"))
        / f64::from(u32::try_from(expected.len()).expect("count"))
}

/// Every returned row is distinct, belongs to the filter, is ordered, and carries the exact
/// distance the independent oracle computes for that row.
pub fn assert_valid_page(
    actual: &[(String, f64)],
    rows: &[Row],
    eligible: impl Fn(&Row) -> bool,
    context: &str,
) {
    let mut seen = std::collections::BTreeSet::new();
    for (label, distance) in actual {
        assert!(seen.insert(label.clone()), "{context}: duplicate {label}");
        let row = rows
            .iter()
            .find(|row| &row.label == label)
            .unwrap_or_else(|| panic!("{context}: unknown row {label}"));
        assert!(eligible(row), "{context}: forbidden row {label}");
        assert!(
            (distance - l2(row.embedding)).abs() <= DISTANCE_TOLERANCE,
            "{context}: {label} distance {distance} is not the exact distance {}",
            l2(row.embedding)
        );
    }
    assert!(
        actual.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "{context}: distances must ascend"
    );
}

pub fn assert_exact_page(actual: &[(String, f64)], expected: &[(String, f64)], context: &str) {
    assert_eq!(
        actual.iter().map(|row| &row.0).collect::<Vec<_>>(),
        expected.iter().map(|row| &row.0).collect::<Vec<_>>(),
        "{context}: ids and order must equal the filtered oracle"
    );
    for (got, want) in actual.iter().zip(expected) {
        assert!(
            (got.1 - want.1).abs() <= DISTANCE_TOLERANCE,
            "{context}: {} distance {} != oracle {}",
            got.0,
            got.1,
            want.1
        );
    }
}
