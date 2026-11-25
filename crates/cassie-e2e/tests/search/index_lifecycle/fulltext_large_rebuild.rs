use super::support_fulltext_maintenance as maintenance;
use super::support_sql as support;
use maintenance::{
    assert_state_matches_clean_rebuild, create_online_table, fallback_total, force_rebuild, ids,
    index_name, search_metric, upsert,
};

use cassie::app::Cassie;
use cassie::midge::adapter::set_fulltext_maintenance_failure_point;

/// Index-build transactions carry at most 1 MiB of index records, and a
/// document commit rebuilds in place only below the same size.
const STAGED_REBUILD_BOUND_BYTES: u64 = 1024 * 1024;
const DOCUMENTS: usize = 8_000;
const LOAD_BATCH: usize = 500;

fn body(document: usize) -> String {
    (0..12)
        .map(|word| format!("w{}", (document * 31 + word * 977) % 5_000))
        .collect::<Vec<_>>()
        .join(" ")
}

fn large_online_table(table: &str) -> Cassie {
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    create_online_table(&cassie, table);
    for start in (0..DOCUMENTS).step_by(LOAD_BATCH) {
        let documents = (start..start + LOAD_BATCH)
            .map(|document| {
                (
                    Some(document.to_string()),
                    serde_json::json!({ "body": body(document) }),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents(table, documents)
            .expect("load batch");
    }
    cassie
}

#[test]
fn should_publish_a_rebuild_larger_than_one_transaction_bound() {
    // Arrange
    let table = "large_staged";
    let cassie = large_online_table(table);
    let bytes_before = search_metric(&cassie, "fulltext_rebuild_index_bytes_written_total");

    // Act
    force_rebuild(&cassie, table);
    let rebuilt_bytes =
        search_metric(&cassie, "fulltext_rebuild_index_bytes_written_total") - bytes_before;
    let fallbacks = fallback_total(&cassie);
    let found = ids(&cassie, table, "w17");

    // Assert
    assert!(
        rebuilt_bytes > STAGED_REBUILD_BOUND_BYTES,
        "the fixture must span several bounded transactions: {rebuilt_bytes}"
    );
    assert_ne!(found.len(), 0);
    assert_eq!(fallback_total(&cassie), fallbacks, "the index serves reads");
}

#[test]
fn should_rebuild_a_large_index_after_the_commit_that_could_not_maintain_it() {
    // Arrange
    let table = "large_deferred";
    let cassie = large_online_table(table);
    set_fulltext_maintenance_failure_point(true);
    upsert(&cassie, table, 1, "stale publication");
    let rebuilds = search_metric(&cassie, "fulltext_rebuilds_total");

    // Act
    upsert(&cassie, table, 2, "fresh words");
    let debt = cassie
        .midge
        .has_fulltext_maintenance_debt(table, &index_name(table))
        .expect("read fulltext debt");
    let fallbacks = fallback_total(&cassie);
    let found = ids(&cassie, table, "fresh");

    // Assert
    assert!(!debt, "the deferred rebuild clears the debt");
    assert_eq!(
        search_metric(&cassie, "fulltext_rebuilds_total"),
        rebuilds + 1,
        "one rebuild after the commit"
    );
    assert_eq!(found, vec![2]);
    assert_eq!(fallback_total(&cassie), fallbacks, "the index serves reads");
    assert_state_matches_clean_rebuild(&cassie, table);
}
