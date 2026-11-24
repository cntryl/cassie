use super::fulltext_synonyms::{ids, reloptions, run, scores, seed, seed_docs, SYNONYMS};
use super::support_sql as support;
use support::*;

use std::sync::{Arc, Barrier};
use std::time::Duration;

use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, SchemaWritePausePoint, StorageFamily,
};
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

const STEMMED: &str = " WITH (stemming = 'english')";

/// Pauses an `ALTER INDEX ... SET (synonyms)` after it rebuilt the new generation in
/// its transaction and before it commits. Returns the paused worker and the resume barrier.
fn pause_alter_before_commit(
    cassie: &Arc<Cassie>,
    sql: &'static str,
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
        Some(SchemaWritePausePoint::FulltextAnalyzerChange),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let worker_cassie = Arc::clone(cassie);
    let worker = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        run(&worker_cassie, sql, vec![]);
    });
    cassie::runtime::test_sync::wait_for_test_barrier(&ready, "fulltext analyzer change ready");
    set_schema_write_commit_barriers(None, None, None);
    (worker, resume, cleanup)
}

#[test]
fn should_show_readers_one_generation_while_synonyms_are_published() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let cassie = Arc::new(support::new_in_memory_cassie().expect("create Cassie"));
    seed(&cassie, "iso_gen", STEMMED);
    let (worker, resume, mut cleanup) = pause_alter_before_commit(
        &cassie,
        "ALTER INDEX iso_gen_ft SET (synonyms = 'car,auto')",
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let reader_cassie = Arc::clone(&cassie);
    let reader = std::thread::spawn(move || {
        tx.send((
            ids(&reader_cassie, "iso_gen", "auto"),
            reloptions(&reader_cassie, "iso_gen_ft"),
        ))
        .expect("send reader observation");
    });

    // Act
    let during = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("reader must not wait on the paused publisher");
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "fulltext analyzer change resume");
    let () = cassie::runtime::test_sync::join_test_thread(worker, "alter worker");
    let () = cassie::runtime::test_sync::join_test_thread(reader, "reader");
    cleanup.disarm();
    let after = (
        ids(&cassie, "iso_gen", "auto"),
        reloptions(&cassie, "iso_gen_ft"),
    );

    // Assert
    assert_eq!(during.0, vec![2]);
    assert_eq!(during.1, "{stemming=english}");
    assert_eq!(after.0, vec![1, 2]);
    assert!(after.1.contains("analyzer_version=2"), "{}", after.1);
    assert!(after.1.contains("synonyms=car,auto"), "{}", after.1);
}

#[test]
fn should_reanalyze_a_document_updated_during_the_synonym_switch() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let cassie = Arc::new(support::new_in_memory_cassie().expect("create Cassie"));
    seed(&cassie, "upd_switch", STEMMED);
    let (worker, resume, mut cleanup) = pause_alter_before_commit(
        &cassie,
        "ALTER INDEX upd_switch_ft SET (synonyms = 'car,auto')",
    );
    let writer_cassie = Arc::clone(&cassie);
    let writer = std::thread::spawn(move || {
        run(
            &writer_cassie,
            "UPDATE upd_switch SET body = 'Auto repair' WHERE id = 3",
            vec![],
        );
    });
    let final_docs: [(i64, &str); 4] = [
        (1, "Cars for sale"),
        (2, "New auto parts"),
        (3, "Auto repair"),
        (4, "Caf\u{e9} menu"),
    ];

    // Act
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "fulltext analyzer change resume");
    let () = cassie::runtime::test_sync::join_test_thread(worker, "alter worker");
    cleanup.disarm();
    writer.join().expect("join update writer");
    seed_docs(
        &cassie,
        "upd_fresh",
        &final_docs,
        " WITH (stemming = 'english', synonyms = 'car,auto')",
    );
    let found = ids(&cassie, "upd_switch", "car");
    let snippet = run(
        &cassie,
        "SELECT snippet(body, 'car') FROM upd_switch WHERE search(body, 'car') AND id = 3",
        vec![],
    );

    // Assert
    assert_eq!(found, vec![1, 2, 3]);
    assert_eq!(
        scores(&cassie, "upd_switch", "car"),
        scores(&cassie, "upd_fresh", "car")
    );
    assert_eq!(
        snippet[0][0],
        Value::String("<mark>Auto</mark> repair".to_string())
    );
}

#[test]
fn should_keep_analyzers_isolated_between_databases() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let postgres = cassie.create_session("tester", Some("postgres".to_string()));
    let in_session = |session: &CassieSession, sql: &str| {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("statement should execute")
            .rows()
            .to_vec()
    };
    in_session(&postgres, "CREATE DATABASE tenant_two");
    let tenant = cassie.create_session("tester", Some("tenant_two".to_string()));
    for session in [&postgres, &tenant] {
        in_session(session, "CREATE TABLE kb (id INT NOT NULL, body TEXT)");
        in_session(
            session,
            "INSERT INTO kb (id, body) VALUES (1, 'Cars for sale')",
        );
        in_session(
            session,
            "INSERT INTO kb (id, body) VALUES (2, 'New auto parts')",
        );
        in_session(
            session,
            "CREATE INDEX kb_ft ON kb USING fulltext (body) WITH (stemming = 'english')",
        );
    }
    let matches = |session: &CassieSession| {
        in_session(
            session,
            "SELECT id FROM kb WHERE search(body, 'auto') ORDER BY id",
        )
    };

    // Act
    in_session(&tenant, "ALTER INDEX kb_ft SET (synonyms = 'car,auto')");

    // Assert
    assert_eq!(matches(&postgres), vec![vec![Value::Int64(2)]]);
    assert_eq!(
        matches(&tenant),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    let options = |session: &CassieSession| {
        in_session(
            session,
            "SELECT reloptions FROM pg_class WHERE relname = 'kb_ft'",
        )
    };
    assert_eq!(
        options(&postgres),
        vec![vec![Value::String("{stemming=english}".to_string())]]
    );
}

fn metadata_artifact(cassie: &Cassie, table: &str, index: &str) -> (Vec<u8>, Vec<u8>) {
    let prefix = cassie
        .midge
        .fulltext_artifact_prefix_for_diagnostics(table, index)
        .expect("fulltext artifact prefix");
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("fulltext artifacts")
        .into_iter()
        .find(|(_, value)| value.starts_with(b"FTM1"))
        .expect("fulltext metadata artifact")
}

fn replace_data(cassie: &Cassie, key: Vec<u8>, value: Vec<u8>) {
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("data transaction");
    tx.put(key, value, None).expect("replace artifact");
    tx.commit(WriteOptions::sync()).expect("commit replacement");
}

fn seed_persisted_synonym_index(path: &str, table: &str) -> Cassie {
    let cassie = Cassie::new_with_data_dir(path).expect("create Cassie");
    cassie.startup().expect("startup");
    seed(&cassie, table, STEMMED);
    run(
        &cassie,
        &format!("ALTER INDEX {table}_ft SET ({SYNONYMS})"),
        vec![],
    );
    cassie
}

#[test]
fn should_rebuild_synonym_state_with_an_older_version_during_startup() {
    // Arrange
    use_local_storage();
    let path = data_dir("fulltext_synonym_old_version");
    let cassie = seed_persisted_synonym_index(&path, "syn_old");
    let (key, mut metadata) = metadata_artifact(&cassie, "syn_old", "syn_old_ft");
    metadata[4] = 1;
    replace_data(&cassie, key, metadata);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("rebuild fulltext sidecar");
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state("syn_old", "syn_old_ft")
        .expect("read rebuilt state")
        .expect("rebuilt state");

    // Assert
    assert_eq!(
        state.analyzer.synonyms.get("car"),
        Some(&"auto".to_string())
    );
    assert_eq!(ids(&restarted, "syn_old", "auto"), vec![1, 2]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_return_exact_results_when_synonym_metadata_is_corrupt() {
    // Arrange
    use_local_storage();
    let path = data_dir("fulltext_synonym_corrupt");
    let cassie = seed_persisted_synonym_index(&path, "syn_bad");
    let (key, mut metadata) = metadata_artifact(&cassie, "syn_bad", "syn_bad_ft");
    metadata.truncate(metadata.len() - 3);
    replace_data(&cassie, key, metadata);

    // Act
    let live = ids(&cassie, "syn_bad", "auto");
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("rebuild fulltext sidecar");
    let rebuilt = ids(&restarted, "syn_bad", "auto");
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state("syn_bad", "syn_bad_ft")
        .expect("read rebuilt state");

    // Assert
    assert_eq!(live, vec![1, 2]);
    assert_eq!(rebuilt, vec![1, 2]);
    assert!(state.is_some_and(|state| !state.analyzer.synonyms.is_empty()));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_stemming_results_after_reopen() {
    // Arrange
    use_local_storage();
    let path = data_dir("fulltext_stemming_reopen");
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        seed_docs(
            &cassie,
            "stem_reopen",
            &[
                (1, "Resetting passwords for locked accounts"),
                (2, "Connections and connected devices"),
            ],
            STEMMED,
        );
    }

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("startup");
    let password = ids(&restarted, "stem_reopen", "password");
    let connect = ids(&restarted, "stem_reopen", "connect");
    let snippet = run(
        &restarted,
        "SELECT snippet(body, 'connect') FROM stem_reopen WHERE search(body, 'connect')",
        vec![],
    );
    let state = restarted
        .midge
        .get_persisted_fulltext_index_state("stem_reopen", "stem_reopen_ft")
        .expect("read state")
        .expect("state");

    // Assert
    assert_eq!(password, vec![1]);
    assert_eq!(connect, vec![2]);
    assert_eq!(
        snippet[0][0],
        Value::String("<mark>Connections</mark> and <mark>connected</mark> devices".to_string())
    );
    assert_eq!(state.postings["connect"].len(), 1);
    assert_eq!(state.postings["password"].len(), 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_default_index_ranking_with_hand_computed_scores() {
    // Arrange
    // BM25 by hand with k1 = 1.2, b = 0.75, N = 3, df(alpha) = 2, avgdl = 8/3:
    // idf = ln((3 - 2 + 0.5) / (2 + 0.5) + 1) = ln 1.6
    // d1: tf 1, dl 2 -> ln 1.6 * 2.2 / (1 + 1.2 * (0.25 + 0.75 * 0.75))
    // d2: tf 2, dl 3 -> ln 1.6 * 4.4 / (2 + 1.2 * (0.25 + 0.75 * 1.125))
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(
        &cassie,
        "CREATE TABLE plain_rank (id INT NOT NULL, body TEXT)",
        vec![],
    );
    for (id, body) in [
        (1, "alpha beta"),
        (2, "alpha alpha gamma"),
        (3, "gamma delta epsilon"),
    ] {
        run(
            &cassie,
            "INSERT INTO plain_rank (id, body) VALUES ($1, $2)",
            vec![Value::Int64(id), Value::String(body.to_string())],
        );
    }
    run(
        &cassie,
        "CREATE INDEX plain_rank_ft ON plain_rank USING fulltext (body)",
        vec![],
    );

    // Act
    let mut ranked = scores(&cassie, "plain_rank", "alpha");
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));

    // Assert
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].0, 2);
    assert_eq!(ranked[1].0, 1);
    assert!((ranked[0].1 - 0.624_306_707_526_411_2).abs() < 1e-12);
    assert!((ranked[1].1 - 0.523_548_346_501_579).abs() < 1e-12);
    assert_eq!(reloptions(&cassie, "plain_rank_ft"), "");
}

#[test]
fn should_keep_one_catalog_entry_after_altering_an_index() {
    // Arrange
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    seed(&cassie, "one_entry", STEMMED);
    run(
        &cassie,
        "CREATE INDEX one_entry_id ON one_entry (id)",
        vec![],
    );

    // Act
    run(
        &cassie,
        "ALTER INDEX one_entry_ft SET (synonyms = 'car,auto')",
        vec![],
    );
    run(
        &cassie,
        "ALTER INDEX one_entry_id SET (fillfactor = 70)",
        vec![],
    );

    // Assert
    assert_eq!(cassie.catalog.list_indexes("one_entry").len(), 2);
    assert_eq!(
        cassie
            .catalog
            .all_indexes_snapshot()
            .iter()
            .filter(|index| index.name.starts_with("one_entry_"))
            .count(),
        2
    );
}
