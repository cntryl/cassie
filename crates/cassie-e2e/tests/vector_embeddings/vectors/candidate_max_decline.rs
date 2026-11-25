//! A top-k window larger than the adaptive candidate maximum declines to the generic scan.
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const CANDIDATE_MAX: usize = 10;
const WINDOW: usize = CANDIDATE_MAX + 1;
const TOP_K: &str =
    "SELECT id, vector_distance(embedding, '[0,0,0]') AS distance FROM docs ORDER BY distance ASC";

fn with_small_candidate_max(test: impl FnOnce(&Cassie, &CassieSession)) {
    crate::support_read_equivalence::with_fixture_config(
        "candidate_max_decline",
        |config| {
            config.limits.adaptive_candidate_max = CANDIDATE_MAX;
            config.limits.adaptive_candidate_min = 1;
        },
        |cassie, session| {
            crate::support_read_equivalence::sql(
                cassie,
                session,
                "CREATE TABLE docs (content TEXT, embedding VECTOR(3))",
            );
            let values = (0..24)
                .map(|index| {
                    let step = f64::from(index) + 1.0;
                    format!("('row {index}', '[{}, {}, 0.25]')", step / 8.0, step / 16.0)
                })
                .collect::<Vec<_>>()
                .join(", ");
            crate::support_read_equivalence::sql(
                cassie,
                session,
                &format!("INSERT INTO docs (content, embedding) VALUES {values}"),
            );
            test(cassie, session);
        },
    );
}

fn brute_force_window(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    let all = crate::support_read_equivalence::sql(cassie, session, TOP_K);
    all.rows().iter().take(WINDOW).cloned().collect()
}

fn limited_window(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    let result = cassie
        .execute_sql(session, &format!("{TOP_K} LIMIT {WINDOW}"), vec![])
        .unwrap_or_else(|error| panic!("window query failed: {error}"));
    result.rows().to_vec()
}

#[test]
fn should_return_brute_force_rows_when_window_exceeds_candidate_max_without_index() {
    with_small_candidate_max(|cassie, session| {
        // Arrange
        let expected = brute_force_window(cassie, session);

        // Act
        let actual = limited_window(cassie, session);

        // Assert
        assert_eq!(actual.len(), WINDOW);
        assert_eq!(actual, expected);
    });
}

#[test]
fn should_return_brute_force_rows_when_window_exceeds_candidate_max_with_hnsw() {
    with_small_candidate_max(|cassie, session| {
        // Arrange
        let expected = brute_force_window(cassie, session);
        crate::support_read_equivalence::sql(
            cassie,
            session,
            "CREATE INDEX docs_hnsw ON docs USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 8, ef_construction = 32, ef_search = 32)",
        );

        // Act
        let actual = limited_window(cassie, session);

        // Assert
        assert_eq!(actual, expected);
    });
}

#[test]
fn should_return_brute_force_rows_when_window_exceeds_candidate_max_with_ivfflat() {
    with_small_candidate_max(|cassie, session| {
        // Arrange
        let expected = brute_force_window(cassie, session);
        crate::support_read_equivalence::sql(
            cassie,
            session,
            "CREATE INDEX docs_ivf ON docs USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 2, probes = 2, training_sample_size = 24, training_seed = 7)",
        );

        // Act
        let actual = limited_window(cassie, session);

        // Assert
        assert_eq!(actual, expected);
    });
}
