//! Column-authoritative table lifecycle (#1106, #1112): transactional create,
//! restart, and atomic conversion between row and column authority.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::midge::adapter::{
    set_document_write_storage_failures, DocumentWriteStorageFailure, StorageFamily,
};
use cassie::types::Value;
use std::path::PathBuf;

const SEEDED_ROWS: &[&str] = &[
    "CREATE TABLE conv_rows (id INT PRIMARY KEY, label TEXT NOT NULL, note TEXT, score DOUBLE PRECISION)",
    "CREATE INDEX conv_label ON conv_rows (label)",
    "INSERT INTO conv_rows VALUES (1, 'alpha', 'first', 1.5), (2, 'beta', NULL, NULL), (3, 'gamma', 'third', 3.25)",
];

struct Memory {
    cassie: Cassie,
    session: CassieSession,
}

impl Memory {
    fn new(setup: &[&str]) -> Self {
        Self::with_config(CassieRuntimeConfig::default(), setup)
    }

    fn with_config(config: CassieRuntimeConfig, setup: &[&str]) -> Self {
        let cassie = Cassie::new_in_memory_with_config(config).expect("memory instance");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        let fixture = Self { cassie, session };
        for sql in setup {
            fixture.run(sql);
        }
        fixture
    }

    fn run(&self, sql: &str) {
        if let Err(error) = self.cassie.execute_sql(&self.session, sql, vec![]) {
            panic!("statement failed: {sql}: {error}");
        }
    }

    fn fail(&self, sql: &str) -> CassieError {
        match self.cassie.execute_sql(&self.session, sql, vec![]) {
            Ok(_) => panic!("statement should fail: {sql}"),
            Err(error) => error,
        }
    }

    fn rows(&self, sql: &str) -> Vec<Vec<String>> {
        text_rows(&self.cassie, &self.session, sql)
    }

    fn mode_of(&self, table: &str) -> Vec<Vec<String>> {
        self.rows(&format!(
            "SELECT storage_mode FROM pg_catalog.pg_table_storage WHERE tablename = '{table}'"
        ))
    }

    /// Every raw `\x18` entry of a column table.
    fn column_entries(&self, table: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
        let prefix = self
            .cassie
            .midge
            .column_store_prefix_for_diagnostics(table)
            .expect("column-store prefix");
        self.cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("raw column-store scan")
    }

    /// Every raw row-blob and legacy document entry of a table.
    fn row_entries(&self, table: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.cassie
            .midge
            .row_store_prefixes_for_diagnostics(table)
            .expect("row-store prefixes")
            .into_iter()
            .flat_map(|prefix| {
                self.cassie
                    .midge
                    .raw_scan_prefix(StorageFamily::Data, &prefix)
                    .expect("raw row-store scan")
            })
            .collect()
    }

    fn row_ids(&self, table: &str) -> Vec<String> {
        let mut ids = self
            .cassie
            .midge
            .scan_documents(table)
            .expect("scan documents")
            .into_iter()
            .map(|document| document.id)
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }
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

fn mode(label: &str) -> Vec<Vec<String>> {
    vec![vec![label.to_owned()]]
}

fn converted_rows() -> Vec<Vec<String>> {
    vec![
        vec!["1", "alpha", "first", "1.5"],
        vec!["2", "beta", "NULL", "NULL"],
        vec!["3", "gamma", "third", "3.25"],
    ]
    .into_iter()
    .map(|row| row.into_iter().map(str::to_owned).collect())
    .collect()
}

const READ_ALL: &str = "SELECT id::TEXT, label, note, score::TEXT FROM conv_rows ORDER BY id";

struct Store(PathBuf);

impl Store {
    fn new(label: &str) -> Self {
        crate::support_sql::use_local_storage();
        let path = PathBuf::from(crate::support_sql::data_dir(label));
        std::fs::create_dir_all(&path).expect("isolated data directory");
        Self(path)
    }

    fn open(&self) -> (Cassie, CassieSession) {
        let cassie = Cassie::new_with_data_dir(&self.0).expect("local store");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            assert!(std::thread::panicking(), "fixture cleanup: {error}");
        }
    }
}

#[test]
fn should_publish_a_column_table_created_in_a_committed_transaction() {
    // Arrange
    let fixture = Memory::new(&[]);

    // Act
    fixture.run("BEGIN");
    fixture.run("CREATE TABLE committed_columns (id INT, label TEXT) USING columnar");
    fixture.run("INSERT INTO committed_columns VALUES (1, 'kept')");
    fixture.run("COMMIT");

    // Assert
    assert_eq!(fixture.mode_of("committed_columns"), mode("column-store"));
    assert_eq!(
        fixture.rows("SELECT id::TEXT, label FROM committed_columns"),
        vec![vec!["1".to_owned(), "kept".to_owned()]]
    );
}

#[test]
fn should_not_publish_a_column_table_created_in_a_rolled_back_transaction() {
    // Arrange
    let fixture = Memory::new(&[]);

    // Act
    fixture.run("BEGIN");
    fixture.run("CREATE TABLE rolled_back_columns (id INT) USING columnar");
    fixture.run("ROLLBACK");

    // Assert
    assert_eq!(
        fixture.mode_of("rolled_back_columns"),
        Vec::<Vec<String>>::new()
    );
    assert!(fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM rolled_back_columns",
            vec![]
        )
        .is_err());
}

#[test]
fn should_discard_a_column_table_created_after_a_rolled_back_savepoint() {
    // Arrange
    let fixture = Memory::new(&[]);

    // Act
    fixture.run("BEGIN");
    fixture.run("SAVEPOINT before_columns");
    fixture.run("CREATE TABLE savepoint_columns (id INT) USING columnar");
    fixture.run("ROLLBACK TO SAVEPOINT before_columns");
    fixture.run("CREATE TABLE savepoint_rows (id INT)");
    fixture.run("COMMIT");

    // Assert
    assert_eq!(
        fixture.mode_of("savepoint_columns"),
        Vec::<Vec<String>>::new()
    );
    assert_eq!(fixture.mode_of("savepoint_rows"), mode("row-store"));
}

#[test]
fn should_preserve_each_table_mode_across_a_local_restart() {
    // Arrange
    let store = Store::new("columnar-restart");
    {
        let (cassie, session) = store.open();
        for sql in [
            "CREATE TABLE restart_rows (id INT, label TEXT)",
            "CREATE TABLE restart_columns (id INT, label TEXT) USING columnar",
            "INSERT INTO restart_rows VALUES (1, 'row')",
            "INSERT INTO restart_columns VALUES (2, 'column'), (3, NULL)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("seed");
        }
    }

    // Act
    let (cassie, session) = store.open();
    let modes = text_rows(
        &cassie,
        &session,
        "SELECT tablename, storage_mode FROM pg_catalog.pg_table_storage \
         WHERE tablename IN ('restart_rows', 'restart_columns') ORDER BY tablename",
    );
    let columns = text_rows(
        &cassie,
        &session,
        "SELECT id::TEXT, label FROM restart_columns ORDER BY id",
    );

    // Assert
    assert_eq!(
        modes,
        vec![
            vec!["restart_columns".to_owned(), "column-store".to_owned()],
            vec!["restart_rows".to_owned(), "row-store".to_owned()],
        ]
    );
    assert_eq!(
        columns,
        vec![
            vec!["2".to_owned(), "column".to_owned()],
            vec!["3".to_owned(), "NULL".to_owned()],
        ]
    );
}

#[test]
fn should_refuse_an_unknown_collection_storage_version_at_startup() {
    // Arrange
    let store = Store::new("columnar-unknown-storage-version");
    {
        let (cassie, session) = store.open();
        cassie
            .execute_sql(&session, "CREATE TABLE future_table (id INT)", vec![])
            .expect("seed");
        cassie
            .midge
            .set_collection_storage_version_for_diagnostics("future_table", 99)
            .expect("write future version");
    }

    // Act
    let result = Cassie::new_with_data_dir(&store.0).and_then(|cassie| cassie.startup());
    let error = result.expect_err("unknown storage version");

    // Assert
    let message = error.to_string();
    assert!(message.contains("future_table"), "{message}");
    assert!(message.contains("storage version"), "{message}");
}

#[test]
fn should_convert_a_row_table_to_columnar_preserving_its_logical_contents() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    let ids_before = fixture.row_ids("conv_rows");
    assert_eq!(fixture.rows(READ_ALL), converted_rows());

    // Act
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");

    // Assert
    assert_eq!(fixture.mode_of("conv_rows"), mode("column-store"));
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
    assert_eq!(fixture.row_ids("conv_rows"), ids_before);
    assert_eq!(
        fixture.rows("SELECT id::TEXT FROM conv_rows WHERE label = 'beta'"),
        vec![vec!["2".to_owned()]]
    );
    assert_eq!(
        fixture
            .fail("INSERT INTO conv_rows VALUES (1, 'again', NULL, NULL)")
            .sql_state(),
        "23505"
    );
    assert_eq!(
        fixture
            .fail("INSERT INTO conv_rows VALUES (9, NULL, NULL, NULL)")
            .sql_state(),
        "23502"
    );
}

#[test]
fn should_convert_a_column_table_back_to_heap_preserving_its_logical_contents() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    let ids_before = fixture.row_ids("conv_rows");

    // Act
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD heap");

    // Assert
    assert_eq!(fixture.mode_of("conv_rows"), mode("row-store"));
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
    assert_eq!(fixture.row_ids("conv_rows"), ids_before);
    assert_eq!(
        fixture
            .fail("INSERT INTO conv_rows VALUES (2, 'dup', NULL, NULL)")
            .sql_state(),
        "23505"
    );
    fixture.run("INSERT INTO conv_rows VALUES (4, 'delta', NULL, 4.5)");
    assert_eq!(
        fixture.rows("SELECT label FROM conv_rows WHERE id = 4"),
        vec![vec!["delta".to_owned()]]
    );
}

#[test]
fn should_treat_a_same_mode_conversion_as_a_no_op() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    let ids_before = fixture.row_ids("conv_rows");

    // Act
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD heap");

    // Assert
    assert_eq!(fixture.mode_of("conv_rows"), mode("row-store"));
    assert_eq!(fixture.row_ids("conv_rows"), ids_before);
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_convert_an_empty_table_in_both_directions() {
    // Arrange
    let fixture = Memory::new(&["CREATE TABLE conv_empty (id INT, label TEXT)"]);

    // Act
    fixture.run("ALTER TABLE conv_empty SET ACCESS METHOD columnar");
    let columnar = fixture.mode_of("conv_empty");
    fixture.run("ALTER TABLE conv_empty SET ACCESS METHOD heap");

    // Assert
    assert_eq!(columnar, mode("column-store"));
    assert_eq!(fixture.mode_of("conv_empty"), mode("row-store"));
}

#[test]
fn should_reject_conversion_to_columnar_while_a_column_index_exists() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    fixture.run("CREATE INDEX conv_column_index ON conv_rows USING column (note)");

    // Act
    let error = fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD columnar");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
    assert_eq!(fixture.mode_of("conv_rows"), mode("column-indexed"));
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_reject_unsupported_methods_without_changing_the_table() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);

    // Act
    let unknown = fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD brin");
    let combined =
        fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD columnar, ADD COLUMN extra INT");

    // Assert
    assert_eq!(unknown.sql_state(), "0A000");
    assert_eq!(combined.sql_state(), "0A000");
    assert_eq!(fixture.mode_of("conv_rows"), mode("row-store"));
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_roll_back_a_conversion_with_its_explicit_transaction() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);

    // Act
    fixture.run("BEGIN");
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    let inside = fixture.rows(READ_ALL);
    fixture.run("ROLLBACK");

    // Assert
    assert_eq!(inside, converted_rows());
    assert_eq!(fixture.mode_of("conv_rows"), mode("row-store"));
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
    fixture.run("INSERT INTO conv_rows VALUES (4, 'delta', NULL, NULL)");
}

#[test]
fn should_commit_a_conversion_with_rows_written_earlier_in_the_same_transaction() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);

    // Act
    fixture.run("BEGIN");
    fixture.run("INSERT INTO conv_rows VALUES (4, 'delta', 'fourth', 4.5)");
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    fixture.run("COMMIT");

    // Assert
    assert_eq!(fixture.mode_of("conv_rows"), mode("column-store"));
    assert_eq!(
        fixture.rows("SELECT id::TEXT, label FROM conv_rows WHERE id = 4"),
        vec![vec!["4".to_owned(), "delta".to_owned()]]
    );
    assert_eq!(
        fixture.rows("SELECT count(*)::TEXT FROM conv_rows"),
        vec![vec!["4".to_owned()]]
    );
}

#[test]
fn should_preserve_a_committed_conversion_across_a_local_restart() {
    // Arrange
    let store = Store::new("columnar-conversion-restart");
    {
        let (cassie, session) = store.open();
        for sql in SEEDED_ROWS {
            cassie.execute_sql(&session, sql, vec![]).expect("seed");
        }
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE conv_rows SET ACCESS METHOD columnar",
                vec![],
            )
            .expect("convert");
    }

    // Act
    let (cassie, session) = store.open();
    let storage = text_rows(
        &cassie,
        &session,
        "SELECT storage_mode FROM pg_catalog.pg_table_storage WHERE tablename = 'conv_rows'",
    );
    let rows = text_rows(&cassie, &session, READ_ALL);

    // Assert
    assert_eq!(storage, mode("column-store"));
    assert_eq!(rows, converted_rows());
}

#[test]
fn should_fail_a_conversion_over_the_memory_budget_without_changing_the_table() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 256 * 1024;
    let fixture = Memory::with_config(config, &["CREATE TABLE conv_large (id INT, body TEXT)"]);
    let padding = "x".repeat(200);
    for batch in 0..8 {
        let values = (0..100)
            .map(|offset| format!("({}, '{padding}')", batch * 100 + offset))
            .collect::<Vec<_>>()
            .join(", ");
        fixture.run(&format!("INSERT INTO conv_large VALUES {values}"));
    }

    // Act
    let error = fixture.fail("ALTER TABLE conv_large SET ACCESS METHOD columnar");

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert_eq!(fixture.mode_of("conv_large"), mode("row-store"));
    // The table is larger than the query budget, so verify through the storage
    // layer instead of a budgeted SQL scan.
    assert_eq!(fixture.row_ids("conv_large").len(), 800);
}

#[test]
fn should_charge_column_field_payloads_before_converting_to_heap() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 256 * 1024;
    let fixture = Memory::with_config(
        config,
        &["CREATE TABLE wide_column (id INT, body TEXT) USING columnar"],
    );
    let payload = "x".repeat(128 * 1024);
    fixture
        .cassie
        .midge
        .put_document(
            "wide_column",
            Some("one".to_owned()),
            serde_json::json!({"id": 1, "body": payload}),
        )
        .expect("seed without a SQL result reservation");
    let before = fixture
        .cassie
        .midge
        .scan_documents("wide_column")
        .expect("before");

    // Act
    let error = fixture.fail("ALTER TABLE wide_column SET ACCESS METHOD heap");

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert_eq!(fixture.mode_of("wide_column"), mode("column-store"));
    let after = fixture
        .cassie
        .midge
        .scan_documents("wide_column")
        .expect("after");
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(after[0].payload, before[0].payload);
}

#[test]
fn should_keep_row_authority_after_a_conversion_rejected_before_commit() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    let rows_before = fixture.row_entries("conv_rows");
    set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 1);

    // Act
    let error = fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 0);

    // Assert
    assert!(error.to_string().contains("injected test"), "{error}");
    assert_eq!(fixture.mode_of("conv_rows"), mode("row-store"));
    assert_eq!(fixture.row_entries("conv_rows"), rows_before);
    assert!(fixture.column_entries("conv_rows").is_empty());
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_keep_column_authority_after_a_conversion_rejected_before_commit() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    let columns_before = fixture.column_entries("conv_rows");
    set_document_write_storage_failures(DocumentWriteStorageFailure::Fenced, 1);

    // Act
    let error = fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD heap");
    set_document_write_storage_failures(DocumentWriteStorageFailure::Fenced, 0);

    // Assert
    assert!(error.to_string().contains("injected test"), "{error}");
    assert_eq!(fixture.mode_of("conv_rows"), mode("column-store"));
    assert_eq!(fixture.column_entries("conv_rows"), columns_before);
    assert!(fixture.row_entries("conv_rows").is_empty());
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_complete_a_conversion_retried_after_an_injected_precommit_failure() {
    // Arrange
    let fixture = Memory::new(SEEDED_ROWS);
    set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 1);
    let _ = fixture.fail("ALTER TABLE conv_rows SET ACCESS METHOD columnar");
    set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 0);

    // Act
    fixture.run("ALTER TABLE conv_rows SET ACCESS METHOD columnar");

    // Assert
    assert_eq!(fixture.mode_of("conv_rows"), mode("column-store"));
    assert!(fixture.row_entries("conv_rows").is_empty());
    assert_eq!(fixture.rows(READ_ALL), converted_rows());
}

#[test]
fn should_not_publish_a_like_clone_when_a_copied_comment_is_invalid() {
    // Arrange
    let fixture = Memory::new(&["CREATE TABLE comment_source (id INT, label TEXT)"]);
    let source = cassie::catalog::canonical_relation_name("postgres", "public", "comment_source");
    let oid =
        cassie::catalog::object_identity::stored_relation_oid(&fixture.cassie.catalog, &source)
            .expect("source oid");
    fixture
        .cassie
        .catalog
        .register_object_comment(cassie::catalog::ObjectCommentMeta {
            format_version: cassie::catalog::ObjectCommentMeta::FORMAT_VERSION,
            address: cassie::catalog::ObjectCommentAddress {
                database: "postgres".to_owned(),
                class_oid: 1259,
                object_oid: oid,
                object_sub_id: 2,
            },
            description: String::new(),
        });

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "CREATE TABLE comment_target (LIKE comment_source INCLUDING COMMENTS) USING columnar",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "invalid comments must reject the whole statement"
    );
    assert_eq!(fixture.mode_of("comment_target"), Vec::<Vec<String>>::new());
    assert!(fixture
        .cassie
        .midge
        .collection_metadata("comment_target")
        .expect("storage metadata")
        .is_none());
    assert_eq!(
        fixture.cassie.catalog.object_comments().len(),
        1,
        "only the source canary remains"
    );
}

fn rejected_decode_conversion(data_type: &str, payload: serde_json::Value) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 256 * 1024;
    let fixture = Memory::with_config(
        config,
        &[&format!(
            "CREATE TABLE decoded_columns (body {data_type}) USING columnar"
        )],
    );
    fixture
        .cassie
        .midge
        .put_document(
            "decoded_columns",
            Some("one".to_owned()),
            serde_json::json!({"body": payload}),
        )
        .expect("seed compact field");
    let before = fixture.column_entries("decoded_columns");
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "ALTER TABLE decoded_columns SET ACCESS METHOD heap",
        vec![],
    );
    assert_eq!(
        result
            .expect_err("decode envelope exceeds budget")
            .sql_state(),
        "54000"
    );
    assert_eq!(fixture.mode_of("decoded_columns"), mode("column-store"));
    assert_eq!(fixture.column_entries("decoded_columns"), before);
    assert!(fixture.row_entries("decoded_columns").is_empty());
}

#[test]
fn should_reserve_json_decode_space_before_conversion_to_heap() {
    // Arrange
    let payload = serde_json::json!({"value": "x".repeat(2048)});

    // Act
    // Assert
    rejected_decode_conversion("JSONB", payload);
}

#[test]
fn should_reserve_array_decode_space_before_conversion_to_heap() {
    // Arrange
    let payload = serde_json::json!(vec![1_i64; 1024]);

    // Act
    // Assert
    rejected_decode_conversion("BIGINT[]", payload);
}
