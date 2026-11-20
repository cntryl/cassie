use super::support_sql as support;
use support::*;

use cassie::app::Cassie;
use cassie::types::Value;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn run(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect("statement should execute")
        .rows()
        .to_vec()
}

pub(super) fn run_error(cassie: &Cassie, sql: &str) -> String {
    let session = cassie.create_session("tester", None);
    match cassie.execute_sql(&session, sql, vec![]) {
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    }
}

const DOCS: [(i64, &str); 4] = [
    (1, "Cars for sale"),
    (2, "New auto parts"),
    (3, "Television listings"),
    (4, "Caf\u{e9} menu"),
];

pub(super) fn seed(cassie: &Cassie, table: &str, index_options: &str) {
    seed_docs(cassie, table, &DOCS, index_options);
}

pub(super) fn seed_docs(cassie: &Cassie, table: &str, docs: &[(i64, &str)], index_options: &str) {
    run(
        cassie,
        &format!("CREATE TABLE {table} (id INT NOT NULL, body TEXT)"),
        vec![],
    );
    for (id, body) in docs.iter().copied() {
        run(
            cassie,
            &format!("INSERT INTO {table} (id, body) VALUES ($1, $2)"),
            vec![Value::Int64(id), Value::String(body.to_string())],
        );
    }
    run(
        cassie,
        &format!("CREATE INDEX {table}_ft ON {table} USING fulltext (body){index_options}"),
        vec![],
    );
}

pub(super) fn ids(cassie: &Cassie, table: &str, query: &str) -> Vec<i64> {
    run(
        cassie,
        &format!("SELECT id FROM {table} WHERE search(body, $1) ORDER BY id"),
        vec![Value::String(query.to_string())],
    )
    .into_iter()
    .map(|row| match row[0] {
        Value::Int64(id) => id,
        _ => panic!("expected integer id"),
    })
    .collect()
}

pub(super) fn scores(cassie: &Cassie, table: &str, query: &str) -> Vec<(i64, f64)> {
    run(
        cassie,
        &format!(
            "SELECT id, search_score(body, $1) FROM {table} WHERE search(body, $1) ORDER BY id"
        ),
        vec![Value::String(query.to_string())],
    )
    .into_iter()
    .map(|row| match (&row[0], &row[1]) {
        (Value::Int64(id), Value::Float64(score)) => (*id, *score),
        _ => panic!("expected id and score"),
    })
    .collect()
}

pub(super) fn reloptions(cassie: &Cassie, index: &str) -> String {
    let rows = run(
        cassie,
        &format!("SELECT reloptions FROM pg_class WHERE relname = '{index}'"),
        vec![],
    );
    match rows.first().map(|row| &row[0]) {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

pub(super) const SYNONYMS: &str = "synonyms = 'car,auto;tv,television;caf\u{e9},bistro'";

#[test]
fn should_match_synonym_groups_through_prepared_queries() {
    // Arrange
    let cassie = new_in_memory_cassie().expect("create Cassie");
    seed(
        &cassie,
        "syn_kb",
        &format!(" WITH (stemming = 'english', {SYNONYMS})"),
    );

    // Act
    let auto = ids(&cassie, "syn_kb", "auto");
    let tv = ids(&cassie, "syn_kb", "tv");
    let bistro = ids(&cassie, "syn_kb", "bistro");
    let snippet = run(
        &cassie,
        "SELECT snippet(body, 'bistro') FROM syn_kb WHERE search(body, 'bistro')",
        vec![],
    );

    // Assert
    assert_eq!(auto, vec![1, 2]);
    assert_eq!(tv, vec![3]);
    assert_eq!(bistro, vec![4]);
    assert_eq!(
        snippet[0][0],
        Value::String("<mark>Caf\u{e9}</mark> menu".to_string())
    );
}

#[test]
fn should_not_apply_synonyms_to_indexes_without_the_option() {
    // Arrange
    let cassie = new_in_memory_cassie().expect("create Cassie");
    seed(&cassie, "plain_kb", "");

    // Act
    let auto = ids(&cassie, "plain_kb", "auto");

    // Assert
    assert_eq!(auto, vec![2]);
    assert_eq!(reloptions(&cassie, "plain_kb_ft"), "");
}

#[test]
fn should_keep_two_differently_configured_indexes_isolated() {
    // Arrange
    let cassie = new_in_memory_cassie().expect("create Cassie");
    seed(
        &cassie,
        "iso_a",
        " WITH (stemming = 'english', synonyms = 'car,auto')",
    );
    seed(
        &cassie,
        "iso_b",
        " WITH (stemming = 'english', synonyms = 'car,truck')",
    );

    // Act
    let a_auto = ids(&cassie, "iso_a", "auto");
    let b_auto = ids(&cassie, "iso_b", "auto");
    let b_truck = ids(&cassie, "iso_b", "truck");

    // Assert
    assert_eq!(a_auto, vec![1, 2]);
    assert_eq!(b_auto, vec![2]);
    assert_eq!(b_truck, vec![1]);
}

#[test]
fn should_publish_synonym_change_with_fresh_statistics() {
    // Arrange
    let cassie = new_in_memory_cassie().expect("create Cassie");
    seed(&cassie, "alter_kb", " WITH (stemming = 'english')");
    seed(
        &cassie,
        "fresh_kb",
        &format!(" WITH (stemming = 'english', {SYNONYMS})"),
    );
    let before = ids(&cassie, "alter_kb", "auto");
    let version_before = reloptions(&cassie, "alter_kb_ft");

    // Act
    run(
        &cassie,
        &format!("ALTER INDEX alter_kb_ft SET ({SYNONYMS})"),
        vec![],
    );

    // Assert
    assert_eq!(before, vec![2]);
    assert_eq!(version_before, "{stemming=english}");
    assert_eq!(ids(&cassie, "alter_kb", "auto"), vec![1, 2]);
    assert_eq!(
        scores(&cassie, "alter_kb", "auto"),
        scores(&cassie, "fresh_kb", "auto")
    );
    let options = reloptions(&cassie, "alter_kb_ft");
    assert!(options.contains("analyzer_version=2"), "{options}");
}

#[test]
fn should_leave_published_generation_untouched_when_synonym_change_is_invalid() {
    // Arrange
    let cassie = new_in_memory_cassie().expect("create Cassie");
    seed(
        &cassie,
        "bad_kb",
        " WITH (stemming = 'english', synonyms = 'car,auto')",
    );

    // Act
    let directional = run_error(
        &cassie,
        "ALTER INDEX bad_kb_ft SET (synonyms = 'car=>auto')",
    );
    let overlap = run_error(
        &cassie,
        "ALTER INDEX bad_kb_ft SET (synonyms = 'car,auto;auto,tv')",
    );
    run(
        &cassie,
        "CREATE TABLE bad_kb2 (id INT NOT NULL, body TEXT)",
        vec![],
    );
    let at_create = run_error(
        &cassie,
        "CREATE INDEX bad_kb2_ft ON bad_kb2 USING fulltext (body) WITH (synonyms = 'solo')",
    );

    // Assert
    assert!(directional.contains("=>"), "{directional}");
    assert!(overlap.contains("more than one group"), "{overlap}");
    assert!(at_create.contains("at least two"), "{at_create}");
    assert_eq!(ids(&cassie, "bad_kb", "auto"), vec![1, 2]);
    assert!(!reloptions(&cassie, "bad_kb_ft").contains("analyzer_version"));
}

#[test]
fn should_keep_altered_synonym_analyzer_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("fulltext_synonym_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            seed(&cassie, "restart_kb", " WITH (stemming = 'english')");
            run(
                &cassie,
                "ALTER INDEX restart_kb_ft SET (synonyms = 'car,auto')",
                vec![],
            );
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let found = ids(&restarted, "restart_kb", "auto");
        let options = reloptions(&restarted, "restart_kb_ft");

        // Assert
        assert_eq!(found, vec![1, 2]);
        assert!(options.contains("analyzer_version=2"), "{options}");
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reanalyze_documents_written_while_synonyms_are_published() {
    // Arrange
    let cassie = Arc::new(new_in_memory_cassie().expect("create Cassie"));
    run(
        &cassie,
        "CREATE TABLE race_kb (id INT NOT NULL, body TEXT)",
        vec![],
    );
    run(
        &cassie,
        "CREATE INDEX race_kb_ft ON race_kb USING fulltext (body) WITH (stemming = 'english')",
        vec![],
    );
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer = {
        let cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            for id in 0..40_i64 {
                run(
                    &cassie,
                    "INSERT INTO race_kb (id, body) VALUES ($1, $2)",
                    vec![Value::Int64(id), Value::String(format!("car number {id}"))],
                );
                if id == 0 {
                    started_tx.send(()).expect("signal first write");
                }
            }
            done_tx.send(()).expect("signal writer done");
        })
    };

    // Act
    started_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("writer should start");
    run(
        &cassie,
        "ALTER INDEX race_kb_ft SET (synonyms = 'car,auto')",
        vec![],
    );
    done_rx
        .recv_timeout(Duration::from_secs(120))
        .expect("writer should finish without deadlock");
    writer.join().expect("writer thread");

    // Assert
    assert_eq!(ids(&cassie, "race_kb", "auto").len(), 40);
    assert_eq!(ids(&cassie, "race_kb", "car").len(), 40);
    assert_eq!(ids(&cassie, "race_kb", "number").len(), 40);
}
