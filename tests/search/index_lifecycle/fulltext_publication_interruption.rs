use super::support_fulltext_maintenance as maintenance;
use super::support_sql as support;
use maintenance::{
    assert_matches_clean_rebuild, create_online_table, fallback_total, ids, persisted_state, upsert,
};

use std::sync::{Arc, Barrier};
use std::time::Duration;

use cassie::app::Cassie;
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_fulltext_maintenance_failure_point, set_schema_write_commit_barriers,
    SchemaWritePausePoint,
};

const QUERIES: [&str; 3] = ["alpha", "beta", "gamma"];

fn pause_at(
    point: SchemaWritePausePoint,
    work: impl FnOnce() + Send + 'static,
) -> (
    std::thread::JoinHandle<()>,
    Arc<Barrier>,
    cassie::runtime::test_sync::TestBarrierCleanup,
) {
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let cleanup = cassie::runtime::test_sync::TestBarrierCleanup::arm([
        Arc::clone(&ready),
        Arc::clone(&resume),
    ]);
    set_schema_write_commit_barriers(
        Some(point),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let worker = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        work();
    });
    cassie::runtime::test_sync::wait_for_test_barrier(&ready, "fulltext publication ready");
    set_schema_write_commit_barriers(None, None, None);
    (worker, resume, cleanup)
}

fn read_on_another_thread<T: Send + 'static>(read: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || sender.send(read()).expect("send observation"));
    let observation = receiver
        .recv_timeout(Duration::from_secs(60))
        .expect("reader must not wait on the paused publication");
    let () = cassie::runtime::test_sync::join_test_thread(reader, "fulltext reader");
    observation
}

#[test]
fn should_serve_the_previous_consistent_result_while_postings_are_published() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let cassie = Arc::new(support::new_in_memory_cassie().expect("create Cassie"));
    create_online_table(&cassie, "pub_pause");
    upsert(&cassie, "pub_pause", 1, "alpha beta");
    let before = persisted_state(&cassie, "pub_pause");
    let writer_cassie = Arc::clone(&cassie);
    let (worker, resume, mut cleanup) = pause_at(
        SchemaWritePausePoint::FulltextPostingPublication,
        move || {
            upsert(&writer_cassie, "pub_pause", 2, "alpha gamma");
        },
    );
    let fallbacks = fallback_total(&cassie);
    let reader_cassie = Arc::clone(&cassie);

    // Act
    let during = read_on_another_thread(move || {
        (
            ids(&reader_cassie, "pub_pause", "alpha"),
            ids(&reader_cassie, "pub_pause", "gamma"),
            persisted_state(&reader_cassie, "pub_pause"),
        )
    });
    let fallbacks_during = fallback_total(&cassie);
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "fulltext publication resume");
    let () = cassie::runtime::test_sync::join_test_thread(worker, "fulltext writer");
    cleanup.disarm();

    // Assert
    assert_eq!(during.0, vec![1]);
    assert_eq!(during.1, Vec::<i64>::new());
    assert_eq!(
        during.2, before,
        "readers keep the previous published state"
    );
    assert_eq!(fallbacks_during, fallbacks, "no fallback while publishing");
    assert_eq!(ids(&cassie, "pub_pause", "alpha"), vec![1, 2]);
    assert_eq!(ids(&cassie, "pub_pause", "gamma"), vec![2]);
    assert_matches_clean_rebuild(&cassie, "pub_pause", &QUERIES);
}

#[test]
fn should_fall_back_with_a_label_while_an_interrupted_rebuild_is_paused_then_serve_after_retry() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let cassie = Arc::new(support::new_in_memory_cassie().expect("create Cassie"));
    create_online_table(&cassie, "pub_rebuild");
    upsert(&cassie, "pub_rebuild", 1, "alpha beta");
    let collection = "pub_rebuild";
    set_fulltext_maintenance_failure_point(true);
    upsert(&cassie, collection, 2, "alpha gamma");
    assert!(cassie
        .midge
        .has_fulltext_maintenance_debt(collection, &maintenance::index_name("pub_rebuild"))
        .expect("read fulltext debt"));
    let retry_cassie = Arc::clone(&cassie);
    let (worker, resume, mut cleanup) =
        pause_at(SchemaWritePausePoint::FulltextRebuildCommit, move || {
            retry_cassie
                .midge
                .retry_maintenance_debt()
                .expect("retry maintenance debt");
        });
    let fallbacks = fallback_total(&cassie);
    let reader_cassie = Arc::clone(&cassie);

    // Act
    let during = read_on_another_thread(move || ids(&reader_cassie, "pub_rebuild", "gamma"));
    let fallbacks_during = fallback_total(&cassie);
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "fulltext rebuild resume");
    let () = cassie::runtime::test_sync::join_test_thread(worker, "fulltext rebuild retry");
    cleanup.disarm();
    let after = ids(&cassie, "pub_rebuild", "gamma");

    // Assert
    assert_eq!(during, vec![2], "labelled exact fallback is still correct");
    assert!(fallbacks_during > fallbacks, "fallback is labelled");
    assert!(
        cassie.metrics()["search"]["retrieval_fallback_reasons"]["invalid_persisted_artifact"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(after, vec![2]);
    assert_eq!(
        fallback_total(&cassie),
        fallbacks_during,
        "index serves again"
    );
    assert!(!cassie
        .midge
        .has_fulltext_maintenance_debt(collection, &maintenance::index_name("pub_rebuild"))
        .expect("read cleared debt"));
    assert_matches_clean_rebuild(&cassie, "pub_rebuild", &QUERIES);
}

#[test]
fn should_recover_an_interrupted_publication_after_reopening_local_storage() {
    // Arrange
    let path = support::data_dir("fulltext_interrupted_publication");
    let table = "pub_reopen";
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        create_online_table(&cassie, table);
        upsert(&cassie, table, 1, "alpha beta");
        set_fulltext_maintenance_failure_point(true);
        upsert(&cassie, table, 2, "alpha gamma");
        assert!(cassie
            .midge
            .has_fulltext_maintenance_debt(table, &maintenance::index_name(table))
            .expect("read fulltext debt"));
    }

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    reopened.startup().expect("replay fulltext debt");
    let fallbacks = fallback_total(&reopened);
    let after = ids(&reopened, table, "gamma");

    // Assert
    assert_eq!(after, vec![2]);
    assert_eq!(fallback_total(&reopened), fallbacks, "index serves again");
    assert_matches_clean_rebuild(&reopened, table, &QUERIES);
    let _ = std::fs::remove_dir_all(path);
}
