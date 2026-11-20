//! Exact vector top-k cost contract (#1365): `ORDER BY <vector distance> LIMIT k` over a plain
//! table keeps a bounded heap, so its accounted query memory depends on `k`, the page of rows in
//! flight and the vector width, never on the number of rows scanned.

use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::runtime::{QueryCancellationHandle, QueryScoringTestBoundary};
use cassie::types::{Value, Vector};

const DIMS: usize = 384;
const K: usize = 20;
const TIED_ROW: usize = 40;
const TIED_STRIDE: usize = 50;
const AMPLE_BUDGET: usize = 1 << 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Storage {
    Heap,
    Columnar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    L2,
    Cosine,
    InnerProduct,
}

impl Metric {
    fn operator(self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
            Self::InnerProduct => "<#>",
        }
    }

    fn distance(self, left: &[f32], right: &[f32]) -> f64 {
        let dot: f64 = left
            .iter()
            .zip(right)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        match self {
            Self::L2 => left
                .iter()
                .zip(right)
                .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                .sum::<f64>()
                .sqrt(),
            Self::Cosine => {
                let norm = |vector: &[f32]| {
                    vector
                        .iter()
                        .map(|value| f64::from(*value).powi(2))
                        .sum::<f64>()
                        .sqrt()
                };
                1.0 - dot / (norm(left) * norm(right))
            }
            Self::InnerProduct => -dot,
        }
    }
}

fn next_value(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// Rows congruent to `TIED_ROW` modulo `TIED_STRIDE` share one vector, so a query equal to it
/// ties more than `K` rows at distance zero.
fn vector_for(row: usize) -> Vec<f32> {
    let source = if row % TIED_STRIDE == TIED_ROW {
        TIED_ROW
    } else {
        row
    };
    let mut state = u64::try_from(source).expect("row fits u64") * 0x9E37_79B9 + 0xABCD;
    (0..DIMS)
        .map(|_| {
            let step = i32::try_from(next_value(&mut state) % 2001).expect("step fits i32");
            f32::from(i16::try_from(step - 1000).expect("centered step fits i16")) / 1000.0
        })
        .collect()
}

fn tied_query() -> Vec<f32> {
    vector_for(TIED_ROW)
}

fn spread_query() -> Vec<f32> {
    vector_for(99_999)
}

fn row_id(row: usize) -> String {
    format!("r{row:05}")
}

fn tenant_of(row: usize) -> String {
    format!("t{}", row % 4)
}

fn config(budget: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: DIMS,
    });
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = budget;
    config
}

fn create_table(cassie: &Cassie, storage: Storage) {
    let session = cassie.create_session("tester", None);
    let using = match storage {
        Storage::Heap => "",
        Storage::Columnar => " USING columnar",
    };
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE TABLE items (id TEXT, tenant TEXT, title TEXT, embedding VECTOR({DIMS})){using}"
            ),
            vec![],
        )
        .expect("create table");
}

/// Heap rows are loaded without query accounting, so a fixture's peak starts near zero.
fn heap_cassie(rows: usize, budget: usize) -> Cassie {
    let cassie = Cassie::new_in_memory_with_config(config(budget)).expect("create Cassie");
    crate::support_sql::install_vector_extension(&cassie);
    create_table(&cassie, Storage::Heap);
    let documents = (0..rows)
        .map(|row| {
            (
                Some(row_id(row)),
                serde_json::json!({
                    "id": row_id(row),
                    "tenant": tenant_of(row),
                    "title": format!("title {row}"),
                    "embedding": vector_for(row),
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("items", documents)
        .expect("load heap rows");
    cassie
}

/// Column tables load through SQL, one row per statement so the load peak stays small.
fn columnar_cassie(rows: usize) -> Cassie {
    let cassie = Cassie::new_in_memory_with_config(config(AMPLE_BUDGET)).expect("create Cassie");
    crate::support_sql::install_vector_extension(&cassie);
    create_table(&cassie, Storage::Columnar);
    let session = cassie.create_session("tester", None);
    for row in 0..rows {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO items (id, tenant, title, embedding) VALUES ($1, $2, $3, $4)",
                vec![
                    Value::String(row_id(row)),
                    Value::String(tenant_of(row)),
                    Value::String(format!("title {row}")),
                    Value::Vector(Vector::new(vector_for(row))),
                ],
            )
            .expect("insert column row");
    }
    cassie
}

fn peak_memory(cassie: &Cassie) -> u64 {
    cassie.metrics()["query"]["peak_accounted_memory_bytes"]
        .as_u64()
        .expect("peak accounted memory")
}

fn query_sql(metric: Metric, descending: bool, filtered: bool) -> String {
    let predicate = if filtered { " WHERE tenant = $2" } else { "" };
    let direction = if descending { " DESC" } else { "" };
    format!(
        "SELECT id, title FROM items{predicate} ORDER BY embedding {} $1{direction} LIMIT {K}",
        metric.operator()
    )
}

fn params(query: &[f32], filtered: bool) -> Vec<Value> {
    let mut params = vec![Value::Vector(Vector::new(query.to_vec()))];
    if filtered {
        params.push(Value::String("t1".to_string()));
    }
    params
}

/// Independent oracle: distance ascending (or descending), then storage id ascending.
fn brute_force(
    rows: usize,
    metric: Metric,
    query: &[f32],
    descending: bool,
    filtered: bool,
) -> Vec<String> {
    let mut scored = (0..rows)
        .filter(|row| !filtered || tenant_of(*row) == "t1")
        .map(|row| (metric.distance(&vector_for(row), query), row_id(row)))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        let distance = left.0.total_cmp(&right.0);
        let distance = if descending {
            distance.reverse()
        } else {
            distance
        };
        distance.then_with(|| left.1.cmp(&right.1))
    });
    scored.into_iter().take(K).map(|(_, id)| id).collect()
}

fn returned_ids(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<String> {
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(&session, sql, params)
        .expect("exact vector query");
    result
        .rows()
        .iter()
        .map(|row| row[0].as_str().expect("id column").to_string())
        .collect()
}

/// Rows the storage page holds while it is scored: about 1 MiB of decoded vectors, at least one.
fn page_rows(dims: usize) -> usize {
    ((1usize << 20) / (dims * 512)).clamp(1, 256)
}

/// The peak accounted bytes an exact top-k may reach: the page in flight at about 512 decoded
/// bytes per component, the `k` fetched result rows at about 64 bytes per component plus row
/// overhead, the heap entries, and a fixed allowance for the query vector, cursor keys and schema.
/// It contains no term in the number of rows scanned.
fn peak_bound(k: usize, dims: usize) -> u64 {
    let page = page_rows(dims) * dims * 512;
    let fetched = k * (dims * 64 + 4096);
    let heap = k * 128;
    let allowance = 256 * 1024;
    u64::try_from(page + fetched + heap + allowance).expect("bound fits u64")
}

mod results {
    use super::*;

    /// One query's label, SQL result and brute-force expectation.
    type Comparison = (String, Vec<String>, Vec<String>);

    fn compare_with_brute_force(cassie: &Cassie, rows: usize) -> Vec<Comparison> {
        let mut comparisons = Vec::new();
        for metric in [Metric::L2, Metric::Cosine, Metric::InnerProduct] {
            for (label, query) in [("tied", tied_query()), ("spread", spread_query())] {
                for filtered in [false, true] {
                    let actual = returned_ids(
                        cassie,
                        &query_sql(metric, false, filtered),
                        params(&query, filtered),
                    );
                    comparisons.push((
                        format!("{metric:?} {label} query, filtered={filtered}"),
                        actual,
                        brute_force(rows, metric, &query, false, filtered),
                    ));
                }
            }
        }
        let actual = returned_ids(
            cassie,
            &query_sql(Metric::L2, true, false),
            params(&spread_query(), false),
        );
        comparisons.push((
            "descending L2".to_string(),
            actual,
            brute_force(rows, Metric::L2, &spread_query(), true, false),
        ));
        comparisons
    }

    #[test]
    fn should_match_brute_force_top_k_including_ties_on_heap_tables() {
        // Arrange
        let rows = 600;
        let cassie = heap_cassie(rows, AMPLE_BUDGET);

        // Act
        let comparisons = compare_with_brute_force(&cassie, rows);

        // Assert
        for (label, actual, expected) in comparisons {
            assert_eq!(actual, expected, "heap {label}");
        }
    }

    #[test]
    fn should_match_brute_force_top_k_including_ties_on_columnar_tables() {
        // Arrange
        let rows = 300;
        let cassie = columnar_cassie(rows);

        // Act
        let comparisons = compare_with_brute_force(&cassie, rows);

        // Assert
        for (label, actual, expected) in comparisons {
            assert_eq!(actual, expected, "columnar {label}");
        }
    }

    #[test]
    fn should_break_distance_ties_by_ascending_row_identity() {
        // Arrange
        let rows = 600;
        let cassie = heap_cassie(rows, AMPLE_BUDGET);
        let tied_ids = (0..rows)
            .filter(|row| row % TIED_STRIDE == TIED_ROW)
            .map(row_id)
            .take(K)
            .collect::<Vec<_>>();

        // Act
        let actual = returned_ids(
            &cassie,
            &query_sql(Metric::L2, false, false),
            params(&tied_query(), false),
        );

        // Assert
        assert!(rows / TIED_STRIDE >= K - 8, "fixture ties many rows");
        assert_eq!(
            actual[..tied_ids.len().min(K)],
            tied_ids[..tied_ids.len().min(K)]
        );
    }
}

mod shapes {
    use super::*;

    #[test]
    fn should_apply_offset_after_exact_ranking() {
        // Arrange
        let rows = 300;
        let cassie = heap_cassie(rows, AMPLE_BUDGET);
        let expected = brute_force(rows, Metric::L2, &spread_query(), false, false);

        // Act
        let actual = returned_ids(
            &cassie,
            "SELECT id, title FROM items ORDER BY embedding <-> $1 LIMIT 10 OFFSET 5",
            params(&spread_query(), false),
        );

        // Assert
        assert_eq!(actual, expected[5..15]);
    }

    #[test]
    fn should_order_null_vector_rows_after_scored_rows_when_fewer_than_k_are_scored() {
        // Arrange
        let cassie = Cassie::new_in_memory_with_config(config(AMPLE_BUDGET)).expect("Cassie");
        crate::support_sql::install_vector_extension(&cassie);
        create_table(&cassie, Storage::Heap);
        let documents = (0..30)
            .map(|row| {
                let embedding = if row < 5 {
                    serde_json::json!(vector_for(row))
                } else {
                    serde_json::Value::Null
                };
                (
                    Some(row_id(row)),
                    serde_json::json!({
                        "id": row_id(row),
                        "tenant": tenant_of(row),
                        "title": format!("title {row}"),
                        "embedding": embedding,
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents("items", documents)
            .expect("load rows");

        // Act
        let actual = returned_ids(
            &cassie,
            &query_sql(Metric::L2, false, false),
            params(&spread_query(), false),
        );

        // Assert
        let scored = brute_force(5, Metric::L2, &spread_query(), false, false);
        assert_eq!(actual.len(), K);
        assert_eq!(actual[..5], scored[..]);
    }

    #[test]
    fn should_order_null_vector_rows_first_for_a_descending_distance() {
        // Arrange
        let cassie = Cassie::new_in_memory_with_config(config(AMPLE_BUDGET)).expect("Cassie");
        crate::support_sql::install_vector_extension(&cassie);
        create_table(&cassie, Storage::Heap);
        let documents = (0..60)
            .map(|row| {
                let embedding = if row % 20 == 3 {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(vector_for(row))
                };
                (
                    Some(row_id(row)),
                    serde_json::json!({
                        "id": row_id(row),
                        "tenant": tenant_of(row),
                        "title": format!("title {row}"),
                        "embedding": embedding,
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents("items", documents)
            .expect("load rows");
        let sql = query_sql(Metric::L2, true, false);
        let explicit = sql.replace(" DESC", " DESC NULLS FIRST");

        // Act
        let actual = returned_ids(&cassie, &sql, params(&spread_query(), false));
        let expected = returned_ids(&cassie, &explicit, params(&spread_query(), false));

        // Assert
        assert_eq!(actual, expected);
        assert_eq!(actual[..3], [row_id(3), row_id(23), row_id(43)]);
    }
}

mod memory {
    use super::*;

    fn run_unfiltered(cassie: &Cassie) -> Vec<String> {
        returned_ids(
            cassie,
            &query_sql(Metric::L2, false, false),
            params(&spread_query(), false),
        )
    }

    #[test]
    fn should_bound_peak_query_memory_independent_of_row_count() {
        // Arrange
        let bound = peak_bound(K, DIMS);
        let mut peaks = Vec::new();

        // Act
        for rows in [500, 2_000] {
            let cassie = heap_cassie(rows, AMPLE_BUDGET);
            let unfiltered = run_unfiltered(&cassie);
            let filtered = returned_ids(
                &cassie,
                &query_sql(Metric::L2, false, true),
                params(&spread_query(), true),
            );
            let metrics = cassie.metrics();
            peaks.push((
                rows,
                peak_memory(&cassie),
                unfiltered,
                filtered,
                metrics["query"]["current_accounted_memory_bytes"].clone(),
            ));
        }

        // Assert
        for (rows, peak, unfiltered, filtered, current) in peaks {
            assert!(
                peak <= bound,
                "{rows} rows peaked at {peak} accounted bytes, above the {bound} byte bound"
            );
            assert_eq!(
                unfiltered,
                brute_force(rows, Metric::L2, &spread_query(), false, false)
            );
            assert_eq!(
                filtered,
                brute_force(rows, Metric::L2, &spread_query(), false, true)
            );
            assert_eq!(current, 0, "{rows} rows released their accounted memory");
        }
    }

    #[test]
    fn should_bound_peak_query_memory_on_columnar_tables() {
        // Arrange
        let rows = 300;
        let cassie = columnar_cassie(rows);
        let load_peak = peak_memory(&cassie);

        // Act
        let ids = run_unfiltered(&cassie);
        let peak = peak_memory(&cassie);

        // Assert
        assert_eq!(
            ids,
            brute_force(rows, Metric::L2, &spread_query(), false, false)
        );
        assert!(
            peak <= peak_bound(K, DIMS),
            "column scan peaked at {peak} accounted bytes after a {load_peak} byte load"
        );
    }

    #[test]
    fn should_enforce_the_measured_need_as_the_exact_vector_memory_budget() {
        // Arrange
        let rows = 500;
        let measuring = heap_cassie(rows, AMPLE_BUDGET);
        let expected = run_unfiltered(&measuring);
        let needed = usize::try_from(peak_memory(&measuring)).expect("peak fits usize");
        drop(measuring);
        let at_need = heap_cassie(rows, needed);
        let below_need = heap_cassie(rows, needed - 1);

        // Act
        let accepted = run_unfiltered(&at_need);
        let session = below_need.create_session("tester", None);
        let rejected = below_need.execute_sql(
            &session,
            &query_sql(Metric::L2, false, false),
            params(&spread_query(), false),
        );

        // Assert
        assert_eq!(accepted, expected);
        assert!(
            u64::try_from(needed).expect("need fits u64") <= peak_bound(K, DIMS),
            "the measured need of {needed} bytes stays within the row-count independent bound"
        );
        let error = rejected.expect_err("a budget below the need is rejected");
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert!(error.to_string().contains("query memory budget"));
        assert_eq!(
            below_need.metrics()["query"]["current_accounted_memory_bytes"],
            0
        );
    }
}

mod cancellation {
    use super::*;

    #[test]
    fn should_return_no_partial_output_when_canceled_while_scoring_candidates() {
        // Arrange
        let cassie = Arc::new(heap_cassie(120, AMPLE_BUDGET));
        let before = cassie.metrics();
        let mut cancellation = QueryCancellationHandle::new();
        let (selected, resume) =
            cancellation.pause_after_scored_candidate(QueryScoringTestBoundary::ExactVector);
        let query_cancellation = cancellation.clone();
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            let session = query_cassie.create_session("tester", None);
            let result = query_cassie.execute_sql_with_cancellation(
                &session,
                &query_sql(Metric::L2, false, false),
                params(&spread_query(), false),
                &query_cancellation,
            );
            let after = query_cassie.metrics();
            let recovery = query_cassie.execute_sql(
                &session,
                &query_sql(Metric::L2, false, false),
                params(&spread_query(), false),
            );
            (result, after, recovery)
        });

        // Act
        let reached = selected.recv_timeout(Duration::from_secs(10));
        cancellation.cancel();
        let _ = resume.send(());
        let (result, after, recovery) = query.join().expect("query thread");

        // Assert
        reached.expect("a candidate was scored before cancellation");
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(after["runtime"]["running_queries"], 0);
        assert_eq!(
            after["query"]["rows_returned_total"],
            before["query"]["rows_returned_total"]
        );
        assert_eq!(recovery.expect("same-session recovery").rows().len(), K);
    }
}
