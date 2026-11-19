use super::sql_ann_metrics::{counter, oracle, ranking_sql, seed_rows, Metric};
use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

const TOP_K: usize = 20;
const RECALL_FLOOR: f64 = 0.90;

/// Deterministic modular three-dimensional fixture: row `i` is
/// `[(7i mod 101), (13i mod 103), (29i mod 107)]` centred and scaled to
/// `[-1, 1)`. The moduli are pairwise coprime, so rows are distinct.
fn modular_corpus(rows: usize) -> Vec<(String, [f32; 3])> {
    let component = |value: usize, modulus: usize| {
        let value = f32::from(u16::try_from(value % modulus).expect("small residue"));
        let modulus = f32::from(u16::try_from(modulus).expect("small modulus"));
        (value / modulus) * 2.0 - 1.0
    };
    (0..rows)
        .map(|index| {
            (
                format!("row-{index:05}"),
                [
                    component(index * 7, 101),
                    component(index * 13, 103),
                    component(index * 29, 107),
                ],
            )
        })
        .collect()
}

const QUERIES: [[f32; 3]; 10] = [
    [1.0, 0.2, 0.0],
    [-0.5, 1.0, 0.3],
    [0.3, 0.3, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 0.1, -0.4],
    [0.1, -1.0, 0.6],
    [0.7, -0.2, 0.2],
    [-0.2, -0.8, -0.5],
    [0.9, 0.4, -0.7],
    [-0.6, 0.6, 0.9],
];

fn measure(algorithm: &str, options: &str, rows: usize) {
    for metric in [Metric::L2, Metric::Cosine, Metric::Dot] {
        with_fixture("sql_ann_metric_recall", |cassie, session| {
            let corpus = modular_corpus(rows);
            seed_rows(cassie, session, "ann_recall", (metric, options), &corpus);
            let mut recalls = Vec::new();
            for query in QUERIES {
                let expected = oracle(metric, &query, false, &corpus);
                let exact = expected[..TOP_K]
                    .iter()
                    .map(|(label, _)| label.as_str())
                    .collect::<Vec<_>>();

                // Act
                let result = sql(
                    cassie,
                    session,
                    &ranking_sql("ann_recall", metric, &query, &format!("LIMIT {TOP_K}")),
                );

                // Assert
                let found = result
                    .rows()
                    .iter()
                    .filter(|row| matches!(&row[0], Value::String(label) if exact.contains(&label.as_str())))
                    .count();
                assert_eq!(result.rows().len(), TOP_K);

                recalls.push(
                    f64::from(u32::try_from(found).expect("count"))
                        / f64::from(u32::try_from(TOP_K).expect("k")),
                );
            }
            assert_eq!(
                counter(cassie, &format!("{algorithm}_executions")),
                u64::try_from(QUERIES.len()).expect("query count"),
                "every measured query must be served by the ANN index"
            );
            assert_eq!(counter(cassie, &format!("{algorithm}_fallbacks")), 0);

            let mean = recalls.iter().sum::<f64>()
                / f64::from(u32::try_from(recalls.len()).expect("count"));
            let worst = recalls.iter().copied().fold(1.0, f64::min);
            assert!(
                mean >= RECALL_FLOOR,
                "{algorithm} {} mean recall below the floor",
                metric.name()
            );
            eprintln!(
                "RECALL algorithm={algorithm} metric={} rows={rows} k={TOP_K} mean={mean:.4} worst={worst:.4} floor={RECALL_FLOOR} per_query={recalls:?}",
                metric.name()
            );
        });
    }
}

#[test]
fn should_measure_ivfflat_recall_at_the_smallest_declared_profile() {
    // Arrange
    let options =
        "index_type = ivfflat, lists = 16, probes = 4, training_sample_size = 1024, training_seed = 42";

    // Act
    // Assert
    measure("ivfflat", options, 1_024);
}

#[test]
fn should_measure_hnsw_recall_at_the_smallest_declared_profile() {
    // Arrange
    let options = "index_type = hnsw, m = 32, ef_construction = 256, ef_search = 256";

    // Act
    // Assert
    // The declared 10,000-row profile takes about a minute to build, so the
    // suite default is the 1,024-row profile; set the variable to run it.
    let rows = std::env::var("CASSIE_ANN_RECALL_HNSW_ROWS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_024);
    measure("hnsw", options, rows);
}
