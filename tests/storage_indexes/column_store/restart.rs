//! Local-storage restart of column-authoritative tables (#1113): mixed DML,
//! conversion and `DROP COLUMN` recover with clean storage verification, and
//! corruption written before a restart still fails reads with `XX001` after it.
//!
//! These fixtures need disk persistence, so each owns an isolated data directory.

use super::Env;
use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use std::path::PathBuf;

struct Store(PathBuf);

impl Store {
    fn new(label: &str) -> Self {
        crate::support_sql::use_local_storage();
        let path = PathBuf::from(crate::support_sql::data_dir(label));
        std::fs::create_dir_all(&path).expect("isolated data directory");
        Self(path)
    }

    fn open(&self) -> Env {
        let cassie = Cassie::new_with_data_dir(&self.0).expect("local store");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        Env { cassie, session }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            assert!(std::thread::panicking(), "fixture cleanup: {error}");
        }
    }
}

fn text_rows(env: &Env, sql: &str) -> Vec<Vec<String>> {
    env.rows(sql)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| match value {
                    Value::String(text) => text,
                    Value::Null => "NULL".to_owned(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn expected(rows: &[(&str, &str, &str)]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|(id, name, qty)| vec![(*id).to_owned(), (*name).to_owned(), (*qty).to_owned()])
        .collect()
}

fn read_sql(table: &str) -> String {
    format!("SELECT id::TEXT, name, qty::TEXT FROM {table} ORDER BY id")
}

fn build_mixed_database(env: &Env) {
    for (table, clause) in [
        ("col_t", " USING columnar"),
        ("conv_t", ""),
        ("back_t", " USING columnar"),
        ("heap_t", ""),
    ] {
        env.run(&format!(
            "CREATE TABLE {table} (id INT PRIMARY KEY, name TEXT, qty INT, extra TEXT){clause}"
        ));
        env.run(&format!(
            "INSERT INTO {table} VALUES (1,'one',10,'x'),(2,'two',20,'y'),(3,'three',30,'z'),(4,'four',40,NULL)"
        ));
        env.run(&format!("UPDATE {table} SET qty = qty + 1 WHERE id = 2"));
        env.run(&format!("DELETE FROM {table} WHERE id = 3"));
    }
    env.run("ALTER TABLE conv_t SET ACCESS METHOD columnar");
    env.run("ALTER TABLE back_t SET ACCESS METHOD heap");
    env.run("ALTER TABLE col_t DROP COLUMN extra");
    env.run("ALTER TABLE conv_t DROP COLUMN extra");
    for table in ["col_t", "conv_t"] {
        env.run(&format!(
            "INSERT INTO {table} (id, name, qty) VALUES (5,'five',50)"
        ));
    }
}

#[test]
fn should_recover_a_mixed_database_after_restart() {
    // Arrange
    let store = Store::new("columnar-mixed-recovery");
    {
        let env = store.open();
        build_mixed_database(&env);
    }

    // Act
    let env = store.open();
    let modes = text_rows(
        &env,
        "SELECT tablename, storage_mode FROM pg_catalog.pg_table_storage \
         WHERE tablename IN ('col_t','conv_t','back_t','heap_t') ORDER BY tablename",
    );
    let reports = ["col_t", "conv_t"].map(|table| {
        env.cassie
            .midge
            .verify_projection_integrity(table, false, true, true)
            .expect("integrity report")
    });

    // Assert
    let with_fifth = expected(&[
        ("1", "one", "10"),
        ("2", "two", "21"),
        ("4", "four", "40"),
        ("5", "five", "50"),
    ]);
    let without_fifth = expected(&[("1", "one", "10"), ("2", "two", "21"), ("4", "four", "40")]);
    assert_eq!(
        modes,
        [
            ["back_t", "row-store"],
            ["col_t", "column-store"],
            ["conv_t", "column-store"],
            ["heap_t", "row-store"],
        ]
        .map(|pair| pair.map(str::to_owned).to_vec())
        .to_vec()
    );
    assert_eq!(text_rows(&env, &read_sql("col_t")), with_fifth);
    assert_eq!(text_rows(&env, &read_sql("conv_t")), with_fifth);
    assert_eq!(text_rows(&env, &read_sql("back_t")), without_fifth);
    assert_eq!(text_rows(&env, &read_sql("heap_t")), without_fifth);
    env.assert_column_authority("col_t", 4, 3, "col_t after restart");
    env.assert_column_authority("conv_t", 4, 3, "conv_t after restart");
    assert_eq!(
        env.column_entries("back_t"),
        Vec::<(Vec<u8>, Vec<u8>)>::new()
    );
    assert_eq!(env.row_store_keys("col_t"), Vec::<Vec<u8>>::new());
    for report in &reports {
        assert!(report.checked_components.iter().any(|c| c == "column_rows"));
        assert_eq!(report.mismatch_count, 0, "{:?}", report.last_error);
    }
}

/// Seeds a column table, restarts once so its cardinality statistics are
/// current, then corrupts one field key. Returns the corrupted row id.
fn seed_and_corrupt(store: &Store) -> String {
    {
        let env = store.open();
        env.run("CREATE TABLE victims (id INT, name TEXT) USING columnar");
        env.run("INSERT INTO victims VALUES (1, 'a'), (2, 'b'), (3, 'c')");
    }
    let env = store.open();
    let mut ids = env
        .cassie
        .midge
        .scan_documents("victims")
        .expect("scan documents")
        .into_iter()
        .map(|document| document.id)
        .collect::<Vec<_>>();
    ids.sort();
    let victim = ids.remove(1);
    env.cassie
        .midge
        .raw_delete(StorageFamily::Data, &env.field_key("victims", 2, &victim))
        .expect("corrupt a field key");
    victim
}

#[test]
fn should_fail_reads_with_data_corrupted_after_a_restart() {
    // Arrange
    let store = Store::new("columnar-corruption-restart");
    let victim = seed_and_corrupt(&store);

    // Act
    let env = store.open();
    let scan = env
        .cassie
        .execute_sql(&env.session, "SELECT id, name FROM victims", vec![]);
    let report = env
        .cassie
        .midge
        .verify_projection_integrity("victims", false, true, true)
        .expect("integrity report");

    // Assert
    let error = scan.expect_err("scan of a corrupt table fails");
    let message = error.to_string();
    assert_eq!(error.sql_state(), "XX001", "{message}");
    assert!(message.contains("victims"), "{message}");
    assert!(message.contains(&victim), "{message}");
    assert!(report.checked_components.iter().any(|c| c == "column_rows"));
    assert_eq!(report.mismatch_count, 1);
}

#[test]
fn should_refuse_startup_naming_a_corrupt_column_table_with_stale_statistics() {
    // Arrange
    let store = Store::new("columnar-corruption-startup");
    let victim;
    {
        let env = store.open();
        env.run("CREATE TABLE victims (id INT, name TEXT) USING columnar");
        env.run("INSERT INTO victims VALUES (1, 'a'), (2, 'b')");
        victim = env
            .cassie
            .midge
            .scan_documents("victims")
            .expect("scan documents")
            .remove(0)
            .id;
        env.cassie
            .midge
            .raw_delete(StorageFamily::Data, &env.field_key("victims", 2, &victim))
            .expect("corrupt a field key");
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&store.0).expect("local store");
    let startup = cassie.startup();

    // Assert
    let message = startup
        .expect_err("startup must fail closed on a corrupt column table")
        .to_string();
    assert!(message.contains("victims"), "{message}");
    assert!(message.contains(&victim), "{message}");
    assert!(message.contains("corrupt"), "{message}");
}

#[test]
fn should_rebuild_a_recovered_column_table_index_from_authoritative_values() {
    // Arrange
    let store = Store::new("columnar-index-rebuild-restart");
    {
        let env = store.open();
        env.run("CREATE TABLE indexed_columns (id INT PRIMARY KEY, label TEXT) USING columnar");
        env.run("CREATE INDEX recovered_label ON indexed_columns (label)");
        env.run("INSERT INTO indexed_columns VALUES (1, 'old'), (2, 'stable'), (3, 'deleted')");
        env.run("UPDATE indexed_columns SET label = 'new' WHERE id = 1");
        env.run("DELETE FROM indexed_columns WHERE id = 3");
        env.run("BEGIN");
        env.run("UPDATE indexed_columns SET label = 'rolled-back' WHERE id = 2");
        env.run("ROLLBACK");
    }

    // Act
    let env = store.open();
    let before = text_rows(
        &env,
        "SELECT id::TEXT FROM indexed_columns WHERE label = 'new'",
    );
    env.run("REINDEX INDEX recovered_label");
    let after = text_rows(
        &env,
        "SELECT id::TEXT FROM indexed_columns WHERE label = 'new'",
    );
    let removed = text_rows(
        &env,
        "SELECT id::TEXT FROM indexed_columns WHERE label IN ('old', 'deleted', 'rolled-back')",
    );
    let all = text_rows(
        &env,
        "SELECT id::TEXT, label FROM indexed_columns ORDER BY id",
    );
    let plan = format!(
        "{:?}",
        env.rows("EXPLAIN SELECT id FROM indexed_columns WHERE label = 'new'")
    );

    // Assert
    assert_eq!(before, vec![vec!["1".to_owned()]]);
    assert_eq!(after, before);
    assert_eq!(removed, Vec::<Vec<String>>::new());
    assert_eq!(
        all,
        vec![
            vec!["1".to_owned(), "new".to_owned()],
            vec!["2".to_owned(), "stable".to_owned()]
        ]
    );
    assert!(plan.contains("recovered_label"), "{plan}");
    assert!(plan.contains("storage_mode=column-store"), "{plan}");
    env.assert_column_authority("indexed_columns", 2, 2, "reindex after restart");
}

/// Reopens an indexed column source, then removes and recreates its derived
/// indexes. Each query must keep the pre-restart answer with no index as well.
fn assert_retrieval_recovery(
    label: &str,
    setup: &[&str],
    requires_vector: bool,
    indexes: &[(&str, &str)],
    queries: &[&str],
) {
    let store = Store::new(label);
    let expected = {
        let env = store.open();
        if requires_vector {
            let root = env.cassie.create_session("root", None);
            env.cassie
                .execute_sql(&root, "CREATE EXTENSION vector", vec![])
                .expect("root installs vector for fixture");
        }
        for statement in setup {
            env.run(statement);
        }
        queries
            .iter()
            .map(|query| env.rows(query))
            .collect::<Vec<_>>()
    };
    let env = store.open();
    for (query, expected) in queries.iter().zip(&expected) {
        assert_eq!(env.rows(query), *expected, "after restart: {query}");
    }
    for (index, _) in indexes {
        env.run(&format!("DROP INDEX {index}"));
    }
    for (query, expected) in queries.iter().zip(&expected) {
        assert_eq!(env.rows(query), *expected, "authoritative scan: {query}");
    }
    for (_, ddl) in indexes {
        env.run(ddl);
    }
    for (query, expected) in queries.iter().zip(&expected) {
        assert_eq!(env.rows(query), *expected, "rebuilt generation: {query}");
    }
    assert_eq!(
        env.row_store_keys("retrieval_columns"),
        Vec::<Vec<u8>>::new()
    );
}

#[test]
fn should_preserve_fulltext_results_across_restart_and_index_rebuild() {
    // Arrange
    let setup = [
        "CREATE TABLE retrieval_columns (id INT, body TEXT) USING columnar",
        "INSERT INTO retrieval_columns VALUES (1,'alpha beta'),(2,'alpha gamma'),(3,'delta')",
        "CREATE INDEX recovered_text ON retrieval_columns USING fulltext (body)",
        "UPDATE retrieval_columns SET body = 'delta' WHERE id = 1",
        "DELETE FROM retrieval_columns WHERE id = 2",
        "INSERT INTO retrieval_columns VALUES (4,'alpha delta')",
        "BEGIN",
        "INSERT INTO retrieval_columns VALUES (5,'alpha')",
        "ROLLBACK",
    ];

    // Act
    // Assert
    assert_retrieval_recovery(
        "columnar-fulltext-recovery",
        &setup,
        false,
        &[(
            "recovered_text",
            "CREATE INDEX recovered_text ON retrieval_columns USING fulltext (body)",
        )],
        &["SELECT id FROM retrieval_columns WHERE search(body, 'alpha') ORDER BY id"],
    );
}

fn assert_vector_recovery(method: &str, options: &str) {
    let ddl = format!("CREATE INDEX recovered_vector ON retrieval_columns USING {method} (emb vector_l2_ops) WITH ({options})");
    let setup = [
        "CREATE TABLE retrieval_columns (id INT, emb VECTOR(3)) USING columnar",
        "INSERT INTO retrieval_columns VALUES (1,'[1,0,0]'::vector),(2,'[0,1,0]'::vector),(3,'[-1,0,0]'::vector)",
        &ddl,
        "UPDATE retrieval_columns SET emb = '[0,0,1]'::vector WHERE id = 1",
        "DELETE FROM retrieval_columns WHERE id = 2",
        "INSERT INTO retrieval_columns VALUES (4,'[0.9,0.1,0]'::vector)",
        "BEGIN",
        "UPDATE retrieval_columns SET emb = '[1,0,0]'::vector WHERE id = 3",
        "ROLLBACK",
    ];
    assert_retrieval_recovery(
        &format!("columnar-{method}-recovery"),
        &setup,
        true,
        &[("recovered_vector", &ddl)],
        &["SELECT id FROM retrieval_columns ORDER BY emb <-> '[1,0,0]'::vector, id LIMIT 2"],
    );
}

#[test]
fn should_preserve_hnsw_results_across_restart_and_index_rebuild() {
    // Arrange
    let options = "m = 4, ef_construction = 16";

    // Act
    // Assert
    assert_vector_recovery("hnsw", options);
}

#[test]
fn should_preserve_ivfflat_results_across_restart_and_index_rebuild() {
    // Arrange
    let options = "lists = 1";

    // Act
    // Assert
    assert_vector_recovery("ivfflat", options);
}

#[test]
fn should_preserve_hybrid_results_across_restart_and_index_rebuild() {
    // Arrange
    let fulltext = "CREATE INDEX recovered_text ON retrieval_columns USING fulltext (body)";
    let vector = "CREATE INDEX recovered_vector ON retrieval_columns USING hnsw (emb vector_l2_ops) WITH (m = 4, ef_construction = 16)";
    let setup = [
        "CREATE TABLE retrieval_columns (id INT, body TEXT, emb VECTOR(3)) USING columnar",
        "INSERT INTO retrieval_columns VALUES (1,'alpha beta','[1,0,0]'::vector),(2,'alpha','[0,1,0]'::vector),(3,'delta','[-1,0,0]'::vector)",
        fulltext,
        vector,
        "UPDATE retrieval_columns SET body = 'alpha delta', emb = '[0,0,1]'::vector WHERE id = 3",
        "DELETE FROM retrieval_columns WHERE id = 1",
        "INSERT INTO retrieval_columns VALUES (4,'alpha alpha','[0.9,0.1,0]'::vector)",
        "BEGIN",
        "INSERT INTO retrieval_columns VALUES (5,'alpha','[1,0,0]'::vector)",
        "ROLLBACK",
    ];

    // Act
    // Assert
    assert_retrieval_recovery(
        "columnar-hybrid-recovery",
        &setup,
        true,
        &[("recovered_text", fulltext), ("recovered_vector", vector)],
        &["SELECT id, hybrid_score(search_score(body,'alpha'), vector_score(emb,'[1,0,0]')) AS score FROM retrieval_columns ORDER BY score DESC, id LIMIT 3"],
    );
}

#[test]
fn should_preserve_time_series_results_across_restart_and_index_rebuild() {
    // Arrange
    let ddl = "CREATE INDEX recovered_time ON retrieval_columns USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)";
    let setup = [
        "CREATE TABLE retrieval_columns (id INT, tenant TEXT, event_at TIMESTAMP, amount INT) USING columnar",
        "INSERT INTO retrieval_columns VALUES (1,'a','2026-01-01T12:00:00Z',10),(2,'a','2026-01-01T13:00:00Z',20),(3,'b','2026-01-01T12:30:00Z',30)",
        ddl,
        "UPDATE retrieval_columns SET event_at = '2026-01-03T00:00:00Z' WHERE id = 2",
        "DELETE FROM retrieval_columns WHERE id = 1",
        "INSERT INTO retrieval_columns VALUES (4,'a','2026-01-01T12:45:00Z',40)",
        "BEGIN",
        "UPDATE retrieval_columns SET amount = 99 WHERE id = 4",
        "ROLLBACK",
    ];

    // Act
    // Assert
    assert_retrieval_recovery(
        "columnar-time-series-recovery",
        &setup,
        false,
        &[("recovered_time", ddl)],
        &["SELECT tenant, amount FROM retrieval_columns WHERE tenant = 'a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T14:00:00Z' ORDER BY amount"],
    );
}

#[test]
fn should_restore_mixed_authority_from_an_independent_physical_snapshot() {
    // Arrange
    let root = Store::new("columnar-physical-snapshot");
    let source_path = root.0.join("source");
    let snapshot_path = root.0.join("snapshot");
    let restored_path = root.0.join("restored");
    let open = |path: &std::path::Path| {
        let cassie = Cassie::new_with_data_dir(path).expect("local snapshot fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        Env { cassie, session }
    };
    {
        let source = open(&source_path);
        source.run("CREATE TABLE snapshot_parents (id INT PRIMARY KEY, label TEXT NOT NULL)");
        source.run("CREATE TABLE snapshot_columns (id INT PRIMARY KEY, parent_id INT REFERENCES snapshot_parents(id), note TEXT) USING columnar");
        source.run("CREATE INDEX snapshot_note ON snapshot_columns (note)");
        source.run("INSERT INTO snapshot_parents VALUES (1, 'captured')");
        source.run("INSERT INTO snapshot_columns VALUES (10, 1, 'first'), (11, 1, NULL)");
    }
    Cassie::create_snapshot_from_data_dir(
        &source_path,
        &snapshot_path,
        cassie::app::CassieSnapshotOptions::default(),
    )
    .expect("physical snapshot");
    {
        let source = open(&source_path);
        source.run("UPDATE snapshot_parents SET label = 'source-only' WHERE id = 1");
    }

    // Act
    Cassie::restore_snapshot(&snapshot_path, &restored_path).expect("physical restore");
    let restored = open(&restored_path);
    let modes = text_rows(&restored, "SELECT tablename, storage_mode FROM pg_catalog.pg_table_storage WHERE tablename IN ('snapshot_parents', 'snapshot_columns') ORDER BY tablename");
    let rows = text_rows(&restored, "SELECT c.id::TEXT, p.label, c.note FROM snapshot_columns c JOIN snapshot_parents p ON p.id = c.parent_id ORDER BY c.id");
    let duplicate = restored.cassie.execute_sql(
        &restored.session,
        "INSERT INTO snapshot_columns VALUES (10, 1, 'duplicate')",
        vec![],
    );
    let orphan = restored.cassie.execute_sql(
        &restored.session,
        "INSERT INTO snapshot_columns VALUES (12, 99, 'orphan')",
        vec![],
    );
    restored.run("UPDATE snapshot_columns SET note = 'restored-only' WHERE id = 10");
    let source = open(&source_path);
    let source_note = text_rows(&source, "SELECT note FROM snapshot_columns WHERE id = 10");
    let source_label = text_rows(&source, "SELECT label FROM snapshot_parents WHERE id = 1");

    // Assert
    assert_eq!(
        modes,
        vec![
            vec!["snapshot_columns".to_owned(), "column-store".to_owned()],
            vec!["snapshot_parents".to_owned(), "row-store".to_owned()]
        ]
    );
    assert_eq!(
        rows,
        expected(&[("10", "captured", "first"), ("11", "captured", "NULL")])
    );
    assert_eq!(
        duplicate.expect_err("restored primary key").sql_state(),
        "23505"
    );
    assert_eq!(
        orphan.expect_err("restored foreign key").sql_state(),
        "23503"
    );
    assert_eq!(source_note, vec![vec!["first".to_owned()]]);
    assert_eq!(source_label, vec![vec!["source-only".to_owned()]]);
    assert_eq!(
        text_rows(
            &restored,
            "SELECT id::TEXT FROM snapshot_columns WHERE note = 'restored-only'"
        ),
        vec![vec!["10".to_owned()]]
    );
    restored.assert_column_authority("snapshot_columns", 2, 3, "physical restore");
}
