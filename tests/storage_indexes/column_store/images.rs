//! Logical database image export and restore of a mixed-mode database through
//! the public API (#1113): modes, values, constraints and derived indexes
//! survive, and the restored column tables still enforce their invariants.

use cassie::app::{Cassie, CassieSession};
use cassie::catalog::canonical_relation_name;
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;

const SOURCE_SETUP: &[&str] = &[
    "CREATE TABLE parents (id INT PRIMARY KEY, name TEXT NOT NULL UNIQUE) USING columnar",
    "CREATE TABLE children (id INT PRIMARY KEY, parent_id INT REFERENCES parents(id), qty INT CHECK (qty > 0))",
    "CREATE TABLE ledger (id INT PRIMARY KEY, parent_id INT REFERENCES parents(id), amount INT CHECK (amount >= 0), note TEXT) USING columnar",
    "CREATE INDEX ledger_amount ON ledger (amount)",
    "CREATE INDEX children_qty ON children (qty)",
    "INSERT INTO parents VALUES (1, 'alpha'), (2, 'beta')",
    "INSERT INTO children VALUES (10, 1, 5), (11, 2, 7)",
    "INSERT INTO ledger VALUES (100, 1, 50, 'first'), (101, 2, 70, NULL), (102, 1, 90, 'third')",
];

fn restored_pair() -> (Cassie, CassieSession) {
    let source = Cassie::new_in_memory_with_config(cassie::config::CassieRuntimeConfig::default())
        .expect("source");
    source.startup().expect("startup");
    let session = source.create_session("tester", None);
    for sql in SOURCE_SETUP {
        source
            .execute_sql(&session, sql, vec![])
            .unwrap_or_else(|error| panic!("setup failed: {sql}: {error}"));
    }
    let mut backup = source.begin_database_backup("postgres").expect("backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("backup chunk") {
        image.extend_from_slice(&chunk);
    }

    let target = Cassie::new_in_memory_with_config(cassie::config::CassieRuntimeConfig::default())
        .expect("target");
    target.startup().expect("startup");
    let mut restore = target.begin_database_restore("restored").expect("staging");
    for chunk in image.chunks(512) {
        restore.push_chunk(chunk).expect("validated chunk");
    }
    restore.finish().expect("publish");
    let restored = target.create_session("tester", Some("restored".to_owned()));
    (target, restored)
}

fn text_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<String>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
        .rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    Value::String(text) => text.clone(),
                    Value::Null => "NULL".to_owned(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn sql_state(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => panic!("statement should fail: {sql}"),
        Err(error) => error.sql_state().to_owned(),
    }
}

fn strings(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

#[test]
fn should_restore_authoritative_state_of_a_mixed_database() {
    // Arrange
    let (cassie, session) = restored_pair();

    // Act
    let modes = text_rows(
        &cassie,
        &session,
        "SELECT tablename, storage_mode FROM pg_catalog.pg_table_storage \
         WHERE tablename IN ('parents','children','ledger') ORDER BY tablename",
    );
    let joined = text_rows(
        &cassie,
        &session,
        "SELECT l.id::TEXT, p.name, c.qty::TEXT FROM ledger l \
         JOIN parents p ON p.id = l.parent_id \
         LEFT JOIN children c ON c.parent_id = p.id AND c.id = 10 ORDER BY l.id",
    );
    let indexed = text_rows(
        &cassie,
        &session,
        "SELECT id::TEXT, note FROM ledger WHERE amount >= 70 ORDER BY id",
    );

    // Assert
    assert_eq!(
        modes,
        strings(&[
            &["children", "row-store"],
            &["ledger", "column-store"],
            &["parents", "column-store"],
        ])
    );
    assert_eq!(
        joined,
        strings(&[
            &["100", "alpha", "5"],
            &["101", "beta", "NULL"],
            &["102", "alpha", "5"],
        ])
    );
    assert_eq!(indexed, strings(&[&["101", "NULL"], &["102", "third"]]));
}

#[test]
fn should_enforce_constraints_on_restored_column_tables() {
    // Arrange
    let (cassie, session) = restored_pair();

    // Act
    let duplicate_key = sql_state(&cassie, &session, "INSERT INTO parents VALUES (1, 'gamma')");
    let duplicate_unique = sql_state(&cassie, &session, "INSERT INTO parents VALUES (3, 'alpha')");
    let missing_parent = sql_state(
        &cassie,
        &session,
        "INSERT INTO ledger VALUES (103, 99, 1, 'orphan')",
    );
    let failed_check = sql_state(
        &cassie,
        &session,
        "INSERT INTO ledger VALUES (104, 1, -1, 'negative')",
    );
    let referenced_delete = sql_state(&cassie, &session, "DELETE FROM parents WHERE id = 1");
    let not_null = sql_state(&cassie, &session, "INSERT INTO parents VALUES (4, NULL)");

    // Assert
    assert_eq!(duplicate_key, "23505");
    assert_eq!(duplicate_unique, "23505");
    assert_eq!(missing_parent, "23503");
    assert_eq!(failed_check, "23514");
    assert_eq!(referenced_delete, "23503");
    assert_eq!(not_null, "23502");
}

#[test]
fn should_preserve_column_authority_when_mutating_a_restored_table() {
    // Arrange
    let (cassie, session) = restored_pair();
    let ledger = canonical_relation_name("restored", "public", "ledger");

    // Act
    cassie
        .execute_sql(
            &session,
            "INSERT INTO ledger VALUES (103, 2, 120, 'fourth')",
            vec![],
        )
        .expect("insert after restore");
    cassie
        .execute_sql(
            &session,
            "UPDATE ledger SET amount = 55 WHERE id = 100",
            vec![],
        )
        .expect("update after restore");
    let indexed = text_rows(
        &cassie,
        &session,
        "SELECT id::TEXT FROM ledger WHERE amount >= 55 ORDER BY id",
    );
    let report = cassie
        .midge
        .verify_projection_integrity(&ledger, true, true, true)
        .expect("integrity report");
    let blob_keys = cassie
        .midge
        .row_store_prefixes_for_diagnostics(&ledger)
        .expect("row-store prefixes")
        .into_iter()
        .flat_map(|prefix| {
            cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, &prefix)
                .expect("raw scan")
        })
        .count();

    // Assert
    assert_eq!(indexed, strings(&[&["100"], &["101"], &["102"], &["103"]]));
    assert!(report.checked_components.iter().any(|c| c == "column_rows"));
    assert_eq!(report.mismatch_count, 0, "{:?}", report.last_error);
    assert_eq!(blob_keys, 0);
}
