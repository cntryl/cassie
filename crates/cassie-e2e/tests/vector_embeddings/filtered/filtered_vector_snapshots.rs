//! #1363: filtered vector results follow the session snapshot through update, delete, rollback
//! and a lowered query-memory budget, and never return a forbidden row.

use cassie::app::CassieError;

use crate::support_vector_filtered::{
    assert_exact_page, assert_valid_page, corpus_for, function_sql, is_tenant_a, measured, open,
    oracle_page, recall, uncached_config, Fixture, IndexKind, RECALL_FLOOR,
};

const TOTAL: usize = 1_000;
const ELIGIBLE: usize = 400;

fn nearest_eligible(rows: &[crate::support_vector_filtered::Row], rank: usize) -> String {
    oracle_page(rows, is_tenant_a, rank + 1, 0)[rank].0.clone()
}

fn first_page(fixture: &Fixture) -> Vec<(String, f64)> {
    fixture.page(&function_sql("tenant = 'a'", 10, 0))
}

#[test]
fn should_exclude_changed_rows_after_a_committed_update_plus_delete() {
    // Arrange
    let mut rows = corpus_for(IndexKind::Hnsw, TOTAL, ELIGIBLE);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let moved = nearest_eligible(&rows, 0);
    let deleted = nearest_eligible(&rows, 1);
    let baseline = first_page(&fixture);

    // Act
    fixture.exec(&format!(
        "UPDATE docs SET tenant = 'b' WHERE label = '{moved}'"
    ));
    fixture.exec(&format!("DELETE FROM docs WHERE label = '{deleted}'"));
    rows.retain(|row| row.label != deleted);
    for row in &mut rows {
        if row.label == moved {
            row.tenant = "b";
        }
    }
    let (actual, delta) = measured(&fixture, &function_sql("tenant = 'a'", 10, 0));

    // Assert
    assert!(baseline.iter().any(|(label, _)| *label == moved));
    assert!(baseline.iter().any(|(label, _)| *label == deleted));
    let expected = oracle_page(&rows, is_tenant_a, 10, 0);
    assert_eq!(actual.len(), 10);
    assert_valid_page(&actual, &rows, is_tenant_a, "after update and delete");
    assert!(recall(&actual, &expected) >= RECALL_FLOOR);
    assert!(
        actual
            .iter()
            .all(|(label, _)| *label != moved && *label != deleted),
        "a moved or deleted row must not appear"
    );
    assert!(
        delta.filtered_ann + delta.standalone_bounded_exact == 1,
        "the filtered strategy served the page: {delta:?}"
    );
}

#[test]
fn should_keep_snapshots_isolated_across_a_rolled_back_transaction() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL, ELIGIBLE);
    let fixture = open(rows.clone(), IndexKind::Hnsw, uncached_config());
    let moved = nearest_eligible(&rows, 0);
    let mut moved_rows = rows.clone();
    for row in &mut moved_rows {
        if row.label == moved {
            row.tenant = "b";
        }
    }
    let other = fixture.cassie.create_session("root", None);
    let sql = function_sql("tenant = 'a'", 10, 0);

    // Act
    fixture.exec("BEGIN");
    fixture.exec(&format!(
        "UPDATE docs SET tenant = 'b' WHERE label = '{moved}'"
    ));
    let (inside, inside_delta) = measured(&fixture, &sql);
    let concurrent = fixture
        .cassie
        .execute_sql(&other, &sql, vec![])
        .expect("concurrent session read");
    fixture.exec("ROLLBACK");
    let (after, after_delta) = measured(&fixture, &sql);

    // Assert
    assert_exact_page(
        &inside,
        &oracle_page(&moved_rows, is_tenant_a, 10, 0),
        "transaction sees its own move through the labelled exact fallback",
    );
    assert_eq!(
        inside_delta.filtered_ann + inside_delta.standalone_bounded_exact,
        0,
        "a transaction overlay never uses the committed-index strategy"
    );
    assert_eq!(
        fixture.cassie.metrics()["vector"]["last_fallback_reason"],
        "transaction-overlay-exact",
        "the overlay fallback is labelled"
    );
    let concurrent_first = match &concurrent.rows()[0][0] {
        cassie::types::Value::String(label) => label.clone(),
        other => panic!("unexpected label {other:?}"),
    };
    assert_eq!(
        concurrent_first, moved,
        "another session still sees the committed snapshot"
    );
    assert_valid_page(&after, &rows, is_tenant_a, "after rollback");
    assert!(after.iter().any(|(label, _)| *label == moved));
    assert!(recall(&after, &oracle_page(&rows, is_tenant_a, 10, 0)) >= RECALL_FLOOR);
    assert_eq!(
        after_delta.filtered_ann + after_delta.standalone_bounded_exact,
        1
    );
}

#[test]
fn should_fail_with_a_resource_limit_rather_than_underfill_given_a_lowered_budget() {
    // Arrange
    let rows = corpus_for(IndexKind::Hnsw, TOTAL, ELIGIBLE);
    let expected = oracle_page(&rows, is_tenant_a, 10, 0);
    let mut outcomes = Vec::new();

    // Act
    for budget in [2 * 1_024, 16 * 1_024, 128 * 1_024, 16 * 1_024 * 1_024] {
        let mut config = uncached_config();
        config.limits.query_memory_budget_bytes = budget;
        let fixture = open(rows.clone(), IndexKind::Hnsw, config);
        let result = fixture
            .cassie
            .execute_sql(
                &fixture.session,
                &function_sql("tenant = 'a'", 10, 0),
                vec![],
            )
            .map(|page| {
                page.rows()
                    .iter()
                    .map(|row| match (&row[0], &row[1]) {
                        (cassie::types::Value::String(label), cassie::types::Value::Float64(d)) => {
                            (label.clone(), *d)
                        }
                        _ => panic!("unexpected row shape"),
                    })
                    .collect::<Vec<(String, f64)>>()
            });
        let accounted = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("accounted bytes");
        outcomes.push((budget, result, accounted));
    }

    // Assert
    for (budget, result, accounted) in &outcomes {
        assert_eq!(
            *accounted, 0,
            "budget {budget}: reservations must be released"
        );
        match result {
            Ok(actual) => {
                assert_eq!(actual.len(), 10, "budget {budget}: no underfilled success");
                assert_valid_page(actual, &rows, is_tenant_a, "within budget");
                assert!(recall(actual, &expected) >= RECALL_FLOOR);
            }
            Err(error) => assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "budget {budget}: a lowered budget fails with a resource limit"
            ),
        }
    }
    assert!(
        outcomes[0].1.is_err(),
        "the smallest budget must be rejected"
    );
    assert!(
        outcomes.last().is_some_and(|(_, result, _)| result.is_ok()),
        "the largest budget succeeds"
    );
}
