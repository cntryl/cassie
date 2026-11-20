//! #1363: window fill, residual predicates, EXPLAIN labels and bound parameters of the
//! filtered vector strategy (`docs/vector-filtered-search-contract.md`).

use cassie::types::Value;

use crate::support_vector_filtered::{
    assert_exact_page, assert_valid_page, corpus_for, function_sql, is_tenant_a, measured,
    nearest_labels, open, operator_sql, oracle_page, recall, uncached_config, Counters, IndexKind,
    FIRST_WINDOW_FOR_TEN, RECALL_FLOOR, TOTAL_ROWS,
};

#[test]
fn should_fill_a_large_window_when_every_first_batch_candidate_is_foreign() {
    for index in [IndexKind::Hnsw, IndexKind::IvfFlat] {
        // Arrange
        let rows = corpus_for(index, TOTAL_ROWS, 2_000);
        let all_foreign = nearest_labels(&rows, FIRST_WINDOW_FOR_TEN)
            .iter()
            .all(|label| !is_tenant_a_label(&rows, label));
        assert!(all_foreign, "fixture invariant: first batch is foreign");
        let fixture = open(rows.clone(), index, uncached_config());
        let expected = oracle_page(&rows, is_tenant_a, 50, 0);
        let context = format!("{index:?}");

        // Act
        let (actual, delta) = measured(&fixture, &operator_sql("tenant = 'a'", 50, 0));

        // Assert: an underfilled page fails even though every returned row is eligible.
        assert_eq!(actual.len(), 50, "{context}: the window must be full");
        assert_valid_page(&actual, &rows, is_tenant_a, &context);
        assert!(recall(&actual, &expected) >= RECALL_FLOOR, "{context}");
        assert_eq!(delta.filtered_ann, 1, "{context}");
        assert!(delta.ann_windows >= 2, "{context}");
    }
}

fn is_tenant_a_label(rows: &[crate::support_vector_filtered::Row], label: &str) -> bool {
    rows.iter()
        .any(|row| row.label == label && is_tenant_a(row))
}

#[test]
fn should_return_every_eligible_row_when_the_page_is_larger_than_the_subset() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 40);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let expected = oracle_page(&rows, is_tenant_a, 100, 0);

    // Act
    let (actual, delta) = measured(&fixture, &operator_sql("tenant = 'a'", 100, 0));

    // Assert
    assert_eq!(actual.len(), 40);
    assert_exact_page(&actual, &expected, "page larger than the subset");
    assert_eq!(delta.standalone_bounded_exact, 1);
}

#[test]
fn should_recheck_a_residual_predicate_on_fetched_rows() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 400);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let half = |row: &crate::support_vector_filtered::Row| is_tenant_a(row) && row.n < 5;
    let sql = operator_sql("tenant = 'a' AND n < 5", 10, 0);
    let expected = oracle_page(&rows, half, 10, 0);

    // Act
    let (actual, delta) = measured(&fixture, &sql);

    // Assert
    assert_eq!(actual.len(), 10);
    assert_valid_page(&actual, &rows, half, "residual predicate");
    assert!(recall(&actual, &expected) >= RECALL_FLOOR);
    assert_eq!(delta.filtered_ann, 1);
    assert!(delta.fallbacks.is_empty(), "{:?}", delta.fallbacks);
}

#[test]
fn should_degrade_to_the_exact_subset_when_a_selective_residual_underfills_the_ann_window() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 400);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let selective = |row: &crate::support_vector_filtered::Row| is_tenant_a(row) && row.n == 3;
    let sql = operator_sql("tenant = 'a' AND n = 3", 10, 0);
    let expected = oracle_page(&rows, selective, 10, 0);

    // Act
    let (actual, delta) = measured(&fixture, &sql);

    // Assert
    assert_eq!(actual.len(), 10, "the page must not be underfilled");
    assert_exact_page(&actual, &expected, "selective residual");
    assert_eq!(delta.fallbacks.get("ann-window-underfilled"), Some(&1));
    assert_eq!(delta.standalone_bounded_exact, 1);
    assert_eq!(delta.filtered_ann, 0);
}

#[test]
fn should_report_each_filtered_vector_strategy_in_explain() {
    // Arrange
    let large = open(
        corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 400),
        IndexKind::Hnsw,
        uncached_config(),
    );
    let small = open(
        corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 40),
        IndexKind::Hnsw,
        uncached_config(),
    );
    let ivf = open(
        corpus_for(IndexKind::IvfFlat, TOTAL_ROWS, 400),
        IndexKind::IvfFlat,
        uncached_config(),
    );

    // Act
    let filtered_ann = large.explain(&operator_sql("tenant = 'a'", 10, 0));
    let bounded = small.explain(&operator_sql("tenant = 'a'", 10, 0));
    let ivf_ann = ivf.explain(&function_sql("tenant = 'a'", 10, 0));
    let full = large.explain(&operator_sql("n = 3", 10, 0));
    let bigger_page = large.explain(&operator_sql("tenant = 'a'", 10, 20));

    // Assert
    assert!(
        filtered_ann.contains("vector_ann=filtered-ann vector_ann_reason=adaptive-window vector_filter_subset=400 vector_ann_index=hnsw"),
        "{filtered_ann}"
    );
    assert!(
        bounded.contains("vector_ann=bounded-exact-subset vector_ann_reason=subset-within-exact-limit vector_filter_subset=40"),
        "{bounded}"
    );
    assert!(
        ivf_ann.contains("vector_ann=filtered-ann") && ivf_ann.contains("vector_ann_index=ivfflat"),
        "{ivf_ann}"
    );
    assert!(
        full.contains("vector_ann=structured-filter-exact vector_ann_reason=no-eligible-scalar-index vector_filter_subset=none"),
        "{full}"
    );
    assert!(
        bigger_page.contains("vector_ann=bounded-exact-subset"),
        "OFFSET + LIMIT of 30 raises the exact limit to 960: {bigger_page}"
    );
}

#[test]
fn should_serve_bound_parameters_through_the_filtered_strategy() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 400);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let before = Counters::read(&fixture.cassie);
    let expected = oracle_page(&rows, is_tenant_a, 10, 0);

    // Act
    let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT label, embedding <-> $1 AS distance FROM docs WHERE tenant = $2 \
             ORDER BY embedding <-> $1 LIMIT 10",
            vec![
                Value::String("[1,0]".to_string()),
                Value::String("a".to_string()),
            ],
        )
        .expect("bound filtered query");
    let delta = Counters::read(&fixture.cassie).since(&before);

    // Assert
    let actual: Vec<(String, f64)> = result
        .rows()
        .iter()
        .map(|row| match (&row[0], &row[1]) {
            (Value::String(label), Value::Float64(distance)) => (label.clone(), *distance),
            _ => panic!("unexpected row shape"),
        })
        .collect();
    assert_eq!(actual.len(), 10);
    assert!(recall(&actual, &expected) >= RECALL_FLOOR);
    assert_eq!(delta.filtered_ann, 1);
}
