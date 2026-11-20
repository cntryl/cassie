use super::support_fulltext_maintenance as maintenance;
use super::support_sql as support;
use maintenance::{
    assert_matches_clean_rebuild, assert_state_matches_clean_rebuild, create_online_sql_table,
    create_online_table, delete, fallback_total, ids, persisted_state, run, search_metric, upsert,
};

use cassie::app::Cassie;

const QUERIES: [&str; 6] = ["common", "rare", "alpha", "beta gamma", "zebra", "quokka"];

fn online_cassie(table: &str) -> Cassie {
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    create_online_table(&cassie, table);
    cassie
}

#[test]
fn should_match_a_clean_rebuild_after_mixed_single_document_writes() {
    // Arrange
    let cassie = online_cassie("inc_mixed");
    for id in 1..=12 {
        upsert(
            &cassie,
            "inc_mixed",
            id,
            &format!("common alpha document number{id} filler"),
        );
    }

    // Act
    upsert(&cassie, "inc_mixed", 3, "common beta gamma gamma");
    upsert(&cassie, "inc_mixed", 4, "alpha alpha alpha common");
    delete(&cassie, "inc_mixed", 5);
    delete(&cassie, "inc_mixed", 12);
    upsert(&cassie, "inc_mixed", 20, "rare zebra");
    upsert(&cassie, "inc_mixed", 21, "rare");
    upsert(&cassie, "inc_mixed", 20, "quokka only");
    delete(&cassie, "inc_mixed", 21);
    upsert(&cassie, "inc_mixed", 22, "");

    // Assert
    assert_eq!(ids(&cassie, "inc_mixed", "rare"), Vec::<i64>::new());
    assert_eq!(ids(&cassie, "inc_mixed", "quokka"), vec![20]);
    assert_matches_clean_rebuild(&cassie, "inc_mixed", &QUERIES);
}

#[test]
fn should_match_a_clean_rebuild_after_a_burst_of_common_term_edits() {
    // Arrange
    let cassie = online_cassie("inc_burst");
    for id in 1..=30 {
        upsert(&cassie, "inc_burst", id, "common steady words");
    }

    // Act
    for round in 0..4 {
        for id in 1..=30 {
            let body = if (id + round) % 3 == 0 {
                format!("common common alpha edit{round}")
            } else {
                format!("steady beta gamma edit{id}")
            };
            upsert(&cassie, "inc_burst", id, &body);
        }
    }
    for id in (1..=30).step_by(4) {
        delete(&cassie, "inc_burst", id);
    }

    // Assert
    assert_matches_clean_rebuild(&cassie, "inc_burst", &QUERIES);
}

#[test]
fn should_match_a_clean_rebuild_after_multi_document_commits() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    create_online_sql_table(&cassie, "inc_batch");
    run(
        &cassie,
        "INSERT INTO inc_batch (id, body) VALUES \
         (1, 'common alpha'), (2, 'common beta'), (3, 'common alpha beta'), \
         (4, 'rare gamma'), (5, 'common gamma gamma')",
        vec![],
    );

    // Act
    run(
        &cassie,
        "UPDATE inc_batch SET body = 'common alpha alpha batch' WHERE id <> 4",
        vec![],
    );
    run(&cassie, "DELETE FROM inc_batch WHERE id IN (2, 5)", vec![]);
    run(
        &cassie,
        "INSERT INTO inc_batch (id, body) VALUES (6, 'zebra'), (7, 'zebra common')",
        vec![],
    );

    // Assert
    assert_state_matches_clean_rebuild(&cassie, "inc_batch");
}

#[test]
fn should_maintain_the_index_without_falling_back_to_row_scans() {
    // Arrange
    let cassie = online_cassie("inc_served");
    upsert(&cassie, "inc_served", 1, "alpha beta");
    let fallbacks = fallback_total(&cassie);
    let rebuilds = search_metric(&cassie, "fulltext_rebuilds_total");

    // Act
    upsert(&cassie, "inc_served", 2, "alpha gamma");
    upsert(&cassie, "inc_served", 1, "delta");
    let found = ids(&cassie, "inc_served", "alpha");

    // Assert
    assert_eq!(found, vec![2]);
    assert_eq!(fallback_total(&cassie), fallbacks);
    assert_eq!(
        search_metric(&cassie, "fulltext_rebuilds_total"),
        rebuilds,
        "single-document writes must not rebuild the index"
    );
}

#[test]
fn should_keep_exact_corpus_totals_across_updates_and_deletes() {
    // Arrange
    let cassie = online_cassie("inc_totals");
    upsert(&cassie, "inc_totals", 1, "one two three");
    upsert(&cassie, "inc_totals", 2, "one");
    upsert(&cassie, "inc_totals", 3, "");

    // Act
    upsert(
        &cassie,
        "inc_totals",
        2,
        "one two three four five six seven",
    );
    delete(&cassie, "inc_totals", 1);
    let state = persisted_state(&cassie, "inc_totals");

    // Assert
    assert_eq!(state.total_documents, 2);
    assert_eq!(state.documents_with_text, 2);
    assert_eq!(state.total_document_length, 7);
    assert!((state.average_document_length - 3.5).abs() < f64::EPSILON);
    assert_matches_clean_rebuild(&cassie, "inc_totals", &QUERIES);
}

#[test]
fn should_drop_the_old_hit_and_expose_the_new_term_when_the_only_document_is_replaced() {
    // Arrange
    let cassie = online_cassie("inc_fresh");
    upsert(&cassie, "inc_fresh", 1, "alpha filler");
    upsert(&cassie, "inc_fresh", 2, "zebra filler");
    upsert(&cassie, "inc_fresh", 3, "gamma filler");
    let fallbacks = fallback_total(&cassie);
    let documents = persisted_state(&cassie, "inc_fresh").total_documents;

    // Act
    upsert(&cassie, "inc_fresh", 2, "quokka filler");

    // Assert
    assert_eq!(ids(&cassie, "inc_fresh", "zebra"), Vec::<i64>::new());
    assert_eq!(ids(&cassie, "inc_fresh", "quokka"), vec![2]);
    assert_eq!(
        persisted_state(&cassie, "inc_fresh").total_documents,
        documents
    );
    assert!(!persisted_state(&cassie, "inc_fresh")
        .postings
        .contains_key("zebra"));
    assert_eq!(fallback_total(&cassie), fallbacks, "served from the index");
}

#[test]
fn should_serve_replaced_terms_from_the_index_after_a_cold_restart() {
    // Arrange
    let path = support::data_dir("fulltext_incremental_restart");
    let table = "inc_restart";
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        create_online_table(&cassie, table);
        upsert(&cassie, table, 1, "alpha filler");
        upsert(&cassie, table, 2, "zebra filler");
        upsert(&cassie, table, 3, "gamma filler");
        upsert(&cassie, table, 2, "quokka filler");
    }

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    reopened.startup().expect("restart Cassie");
    let fallbacks = fallback_total(&reopened);
    let old_hits = ids(&reopened, table, "zebra");
    let new_hits = ids(&reopened, table, "quokka");

    // Assert
    assert_eq!(old_hits, Vec::<i64>::new());
    assert_eq!(new_hits, vec![2]);
    assert_eq!(persisted_state(&reopened, table).total_documents, 3);
    assert_eq!(
        fallback_total(&reopened),
        fallbacks,
        "served from the index"
    );
    assert_matches_clean_rebuild(&reopened, table, &QUERIES);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_a_clean_rebuild_when_a_deleted_document_id_is_reinserted() {
    // Arrange
    let cassie = online_cassie("inc_repair");
    upsert(&cassie, "inc_repair", 1, "alpha beta");
    upsert(&cassie, "inc_repair", 2, "alpha");

    // Act
    delete(&cassie, "inc_repair", 1);
    upsert(&cassie, "inc_repair", 1, "beta");

    // Assert
    assert_eq!(ids(&cassie, "inc_repair", "alpha"), vec![2]);
    assert_eq!(ids(&cassie, "inc_repair", "beta"), vec![1]);
    assert_matches_clean_rebuild(&cassie, "inc_repair", &QUERIES);
}

#[test]
fn should_keep_corpus_totals_when_text_moves_between_null_empty_and_words() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    create_online_sql_table(&cassie, "inc_presence");
    run(
        &cassie,
        "INSERT INTO inc_presence (id, body) VALUES \
         (1, 'common alpha'), (2, NULL), (3, ''), (4, 'common beta'), (5, NULL)",
        vec![],
    );
    let rebuilds = search_metric(&cassie, "fulltext_rebuilds_total");

    // Act
    run(
        &cassie,
        "UPDATE inc_presence SET body = NULL WHERE id = 1",
        vec![],
    );
    run(
        &cassie,
        "UPDATE inc_presence SET body = 'rare gamma' WHERE id = 2",
        vec![],
    );
    run(
        &cassie,
        "UPDATE inc_presence SET body = 'common' WHERE id = 3",
        vec![],
    );
    run(
        &cassie,
        "UPDATE inc_presence SET body = '' WHERE id = 4",
        vec![],
    );
    run(&cassie, "DELETE FROM inc_presence WHERE id = 5", vec![]);
    let maintained_rebuilds = search_metric(&cassie, "fulltext_rebuilds_total");

    // Assert
    assert_eq!(
        maintained_rebuilds, rebuilds,
        "presence changes must be maintained without a rebuild"
    );
    let state = persisted_state(&cassie, "inc_presence");
    assert_eq!(state.total_documents, 4);
    assert_eq!(state.documents_with_text, 3);
    assert_state_matches_clean_rebuild(&cassie, "inc_presence");
}
