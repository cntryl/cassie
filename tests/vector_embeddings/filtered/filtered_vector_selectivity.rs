//! #1363: filtered vector top-k at 1%, 10% and 50% selectivity for HNSW and IVFFlat.
//!
//! Every fixture places all of the nearest-neighbor first window on the foreign tenant and ranks
//! the first eligible row after every foreign row, so a strategy that trusts the first ANN batch
//! returns an underfilled page. Results are checked against an independent filtered oracle. The
//! subset is reranked exactly when it is at most `max(32 * (OFFSET + LIMIT), 256)` rows, which
//! labels `bounded-exact`; larger subsets use filtered ANN windows, an approximate profile with
//! the 0.90 recall@k floor from `docs/performance-contracts.md`.

use crate::support_vector_filtered::{
    assert_exact_page, assert_valid_page, corpus_for, function_sql, is_tenant_a, measured,
    nearest_labels, open, operator_sql, oracle_page, recall, uncached_config, IndexKind, Row,
    FIRST_WINDOW_FOR_TEN, RECALL_FLOOR, TOTAL_ROWS,
};

const PAGES: [(usize, usize); 3] = [(10, 0), (10, 10), (20, 17)];

fn bounded_exact_expected(eligible: usize, limit: usize, offset: usize) -> bool {
    eligible <= ((limit + offset) * 32).max(256)
}

fn assert_first_window_is_foreign(rows: &[Row]) {
    let foreign = nearest_labels(rows, FIRST_WINDOW_FOR_TEN)
        .iter()
        .all(|label| {
            rows.iter()
                .find(|row| &row.label == label)
                .is_some_and(|row| row.tenant == "b")
        });
    assert!(
        foreign,
        "fixture invariant: the first ANN window holds no eligible row"
    );
}

fn assert_selectivity(eligible: usize, index: IndexKind) {
    // Arrange
    let rows = corpus_for(index, TOTAL_ROWS, eligible);
    assert_first_window_is_foreign(&rows);
    let fixture = open(rows.clone(), index, uncached_config());

    for (limit, offset) in PAGES {
        for (shape, sql) in [
            ("operator", operator_sql("tenant = 'a'", limit, offset)),
            ("function", function_sql("tenant = 'a'", limit, offset)),
        ] {
            let context =
                format!("{index:?} {eligible} eligible {shape} limit {limit} offset {offset}");
            let expected = oracle_page(&rows, is_tenant_a, limit, offset);

            // Act
            let (actual, delta) = measured(&fixture, &sql);

            // Assert
            assert_eq!(
                actual.len(),
                expected.len(),
                "{context}: the window must fill"
            );
            assert_valid_page(&actual, &rows, is_tenant_a, &context);
            assert!(
                delta.fallbacks.is_empty(),
                "{context}: unexpected fallback {:?}",
                delta.fallbacks
            );
            assert!(
                delta.row_fetches < u64::try_from(TOTAL_ROWS).unwrap(),
                "{context}: fetched {} rows, an unrestricted exact scan reads {TOTAL_ROWS}",
                delta.row_fetches
            );
            if index == IndexKind::None || bounded_exact_expected(eligible, limit, offset) {
                assert_exact_page(&actual, &expected, &context);
                assert_eq!(delta.standalone_bounded_exact, 1, "{context}");
                assert_eq!(delta.filtered_ann, 0, "{context}");
                assert_eq!(
                    delta.row_fetches,
                    u64::try_from(eligible).unwrap(),
                    "{context}: bounded exact reads exactly the scalar subset"
                );
            } else {
                assert!(
                    recall(&actual, &expected) >= RECALL_FLOOR,
                    "{context}: recall {} below the {RECALL_FLOOR} floor: got {:?} expected {:?}",
                    recall(&actual, &expected),
                    &actual[..3.min(actual.len())],
                    &expected[..3.min(expected.len())]
                );
                assert_eq!(delta.filtered_ann, 1, "{context}");
                assert_eq!(delta.standalone_bounded_exact, 0, "{context}");
                assert!(
                    delta.ann_windows >= 2,
                    "{context}: the first window was all foreign, so it must have grown"
                );
                assert!(
                    delta.eligible_candidates >= u64::try_from(limit + offset).unwrap(),
                    "{context}: eligible candidates must cover the requested window"
                );
                let served = match index {
                    IndexKind::Hnsw => delta.hnsw_executions,
                    IndexKind::IvfFlat => delta.ivfflat_executions,
                    IndexKind::None => 0,
                };
                assert_eq!(
                    served, 1,
                    "{context}: the matching ANN counter must advance"
                );
            }
        }
    }
}

#[test]
fn should_serve_one_percent_tenant_pages_exactly_from_the_scalar_subset_with_hnsw() {
    assert_selectivity(40, IndexKind::Hnsw);
}

#[test]
fn should_serve_ten_percent_tenant_pages_from_filtered_ann_windows_with_hnsw() {
    assert_selectivity(400, IndexKind::Hnsw);
}

#[test]
fn should_serve_half_tenant_pages_from_filtered_ann_windows_with_hnsw() {
    assert_selectivity(2_000, IndexKind::Hnsw);
}

#[test]
fn should_serve_one_percent_tenant_pages_exactly_from_the_scalar_subset_with_ivfflat() {
    assert_selectivity(40, IndexKind::IvfFlat);
}

#[test]
fn should_serve_ten_percent_tenant_pages_from_filtered_ann_windows_with_ivfflat() {
    assert_selectivity(400, IndexKind::IvfFlat);
}

#[test]
fn should_serve_half_tenant_pages_from_filtered_ann_windows_with_ivfflat() {
    assert_selectivity(2_000, IndexKind::IvfFlat);
}

#[test]
fn should_serve_tenant_pages_exactly_from_the_scalar_subset_given_no_vector_index() {
    assert_selectivity(400, IndexKind::None);
}

#[test]
fn should_read_far_fewer_rows_than_an_unrestricted_exact_scan_given_one_percent_eligible() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL_ROWS, 40);
    let fixture = open(rows, IndexKind::Hnsw, uncached_config());

    // Act: `n` has no scalar index, so this is the labelled full exact fallback.
    let (_, scan) = measured(&fixture, &function_sql("n = 3", 10, 0));
    let (_, filtered) = measured(&fixture, &function_sql("tenant = 'a'", 10, 0));

    // Assert
    assert_eq!(
        scan.exact_scan_rows,
        u64::try_from(TOTAL_ROWS).unwrap(),
        "the control exact scan reads every row"
    );
    assert_eq!(scan.fallbacks.get("no-eligible-scalar-index"), Some(&1));
    assert_eq!(filtered.row_fetches, 40);
    assert!(filtered.row_fetches * 50 < scan.exact_scan_rows);
}
