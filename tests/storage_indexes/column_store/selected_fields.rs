//! Selected-field reads of column-authoritative tables (#1356).
//!
//! A narrow query pays for the marker and the fields it selects. These tests
//! prove it with the independent counters: field payload reads, bytes and
//! decodes, retained value bytes, and ownership validation reads.

use super::{env, Env};
use cassie::app::CassieError;
use cassie::midge::adapter::ColumnFieldReadSnapshot;
use cassie::types::Value;

const ROWS: usize = 20;
const STATUS_FIELD: u32 = 2;
const BIG_FIELD: u32 = 3;
const WIDE_COLUMNS: &str = "id INT, status TEXT, big TEXT, extra TEXT";
const NARROW_COLUMNS: &str = "id INT, status TEXT";

fn insert_wide(env: &Env, table: &str, big_len: usize) {
    let big = "b".repeat(big_len);
    let rows = (0..ROWS)
        .map(|index| {
            let extra = if index % 2 == 0 { "'e'" } else { "NULL" };
            format!("({index}, 's{index}', '{big}', {extra})")
        })
        .collect::<Vec<_>>()
        .join(",");
    env.run(&format!("INSERT INTO {table} VALUES {rows}"));
}

fn insert_narrow(env: &Env, table: &str) {
    let rows = (0..ROWS)
        .map(|index| format!("({index}, 's{index}')"))
        .collect::<Vec<_>>()
        .join(",");
    env.run(&format!("INSERT INTO {table} VALUES {rows}"));
}

fn create_wide(env: &Env, table: &str, big_len: usize) {
    env.run(&format!(
        "CREATE TABLE {table} ({WIDE_COLUMNS}) USING columnar"
    ));
    insert_wide(env, table, big_len);
}

fn create_narrow(env: &Env, table: &str) {
    env.run(&format!(
        "CREATE TABLE {table} ({NARROW_COLUMNS}) USING columnar"
    ));
    insert_narrow(env, table);
}

#[derive(Debug)]
struct Observed {
    rows: Vec<Vec<Value>>,
    fields: ColumnFieldReadSnapshot,
    ownership_reads: u64,
    marker_reads: u64,
}

fn observe(env: &Env, sql: &str) -> Result<Observed, CassieError> {
    let midge = &env.cassie.midge;
    let fields = midge.column_field_reads_for_diagnostics();
    let ownership = midge.column_validation_reads_for_diagnostics();
    let markers = midge.query_scan_entries_for_diagnostics();
    let result = env.cassie.execute_sql(&env.session, sql, vec![])?;
    Ok(Observed {
        rows: result.rows().to_vec(),
        fields: midge.column_field_reads_for_diagnostics().since(fields),
        ownership_reads: midge.column_validation_reads_for_diagnostics() - ownership,
        marker_reads: midge.query_scan_entries_for_diagnostics() - markers,
    })
}

fn observed(env: &Env, sql: &str) -> Observed {
    observe(env, sql).unwrap_or_else(|error| panic!("{sql}: {error}"))
}

#[test]
fn should_decode_zero_unselected_payloads_for_a_two_field_projection() {
    // Arrange
    let env = env();
    create_narrow(&env, "narrow_col");
    create_wide(&env, "wide_col", 8_192);

    // Act
    let narrow = observed(&env, "SELECT id, status FROM narrow_col ORDER BY id");
    let wide = observed(&env, "SELECT id, status FROM wide_col ORDER BY id");

    // Assert
    assert_eq!(wide.rows, narrow.rows);
    assert_eq!(wide.rows.len(), ROWS);
    assert_eq!(wide.fields.payload_reads, 2 * ROWS as u64);
    assert_eq!(wide.fields.decodes, 2 * ROWS as u64);
    assert_eq!(wide.fields, narrow.fields);
    assert!(
        wide.fields.payload_bytes < 8_192,
        "selected bytes {} must be far below one unselected payload",
        wide.fields.payload_bytes
    );
    assert!(wide.fields.retained_bytes > 0);
    assert_eq!(wide.marker_reads, narrow.marker_reads);
}

#[test]
fn should_report_ownership_validation_reads_apart_from_selected_field_work() {
    // Arrange
    let env = env();
    create_narrow(&env, "narrow_col");
    create_wide(&env, "wide_col", 64);

    // Act
    let narrow = observed(&env, "SELECT id, status FROM narrow_col");
    let wide = observed(&env, "SELECT id, status FROM wide_col");

    // Assert
    // Complete traversal validates every non-retired field key and its marker.
    assert_eq!(narrow.ownership_reads, 2 * 2 * ROWS as u64);
    assert_eq!(wide.ownership_reads, 4 * 2 * ROWS as u64);
    assert_eq!(wide.fields, narrow.fields);
    assert_eq!(wide.marker_reads, ROWS as u64);
}

#[test]
fn should_keep_selected_costs_constant_when_only_unselected_payloads_grow_tenfold() {
    // Arrange
    let env = env();
    create_wide(&env, "small_col", 1_000);
    create_wide(&env, "large_col", 10_000);
    let sql = "SELECT id, status FROM {t} ORDER BY id";

    // Act
    let small = observed(&env, &sql.replace("{t}", "small_col"));
    let large = observed(&env, &sql.replace("{t}", "large_col"));

    // Assert
    assert_eq!(large.rows, small.rows);
    assert_eq!(large.fields, small.fields);
    assert_eq!(large.fields.retained_bytes, small.fields.retained_bytes);
    assert_eq!(large.fields.decodes, small.fields.decodes);
    assert_eq!(large.ownership_reads, small.ownership_reads);
}

#[test]
fn should_read_the_wide_field_only_when_it_is_selected() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 4_096);

    // Act
    let selected = observed(&env, "SELECT id, big FROM wide_col");
    let filtered = observed(&env, "SELECT id FROM wide_col WHERE status = 's3'");

    // Assert
    assert_eq!(selected.fields.decodes, 2 * ROWS as u64);
    assert!(selected.fields.payload_bytes >= (ROWS * 4_096) as u64);
    assert!(
        filtered.fields.payload_bytes < 4_096,
        "{:?}",
        filtered.fields
    );
    assert_eq!(filtered.rows.len(), 1);
}

#[test]
fn should_resolve_a_field_added_after_the_rows_without_reading_a_payload() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 256);
    env.run("ALTER TABLE wide_col ADD COLUMN later INT");

    // Act
    let added = observed(&env, "SELECT later FROM wide_col");
    let dropped = {
        env.run("ALTER TABLE wide_col DROP COLUMN extra");
        observed(&env, "SELECT id, later FROM wide_col ORDER BY id")
    };

    // Assert
    assert!(
        added.rows.iter().all(|row| row == &[Value::Null]),
        "{:?}",
        added.rows
    );
    assert_eq!(added.fields.payload_reads, 0);
    assert_eq!(added.fields.decodes, 0);
    assert_eq!(dropped.rows.len(), ROWS);
    assert_eq!(dropped.fields.decodes, ROWS as u64);
}

#[test]
fn should_match_row_tables_for_null_default_rename_point_pagination() {
    // Arrange
    let env = env();
    for (table, clause) in [("t_row", ""), ("t_col", " USING columnar")] {
        env.run(&format!(
            "CREATE TABLE {table} (id INT, status TEXT, note TEXT){clause}"
        ));
        env.run(&format!(
            "INSERT INTO {table} VALUES (1,'a',NULL),(2,NULL,'n2'),(3,'c','n3'),(4,'d',NULL),(5,NULL,NULL)"
        ));
        env.run(&format!(
            "ALTER TABLE {table} ADD COLUMN later INT DEFAULT 7"
        ));
        env.run(&format!(
            "ALTER TABLE {table} RENAME COLUMN status TO state"
        ));
        env.run(&format!("INSERT INTO {table} VALUES (6,'f','n6',60)"));
    }

    // Act
    // Assert
    for query in [
        "SELECT id, state, later FROM {t} ORDER BY id",
        "SELECT id FROM {t} WHERE state IS NULL ORDER BY id",
        "SELECT state, note FROM {t} WHERE id = 3",
        "SELECT id, later FROM {t} WHERE id = 6",
        "SELECT id, state FROM {t} ORDER BY id LIMIT 2 OFFSET 2",
        "SELECT id FROM {t} WHERE note IS NULL ORDER BY id DESC LIMIT 2",
        "SELECT * FROM {t} ORDER BY id",
    ] {
        let row = env.rows(&query.replace("{t}", "t_row"));
        let col = env.rows(&query.replace("{t}", "t_col"));
        assert_eq!(col, row, "{query}");
    }
    // The rename keeps the stable field id: the key written before it still serves the column.
    let Value::String(third) = env
        .rows("SELECT _id FROM t_col WHERE id = 3")
        .remove(0)
        .remove(0)
    else {
        panic!("row id must be text");
    };
    let stored = env
        .cassie
        .midge
        .raw_get(
            cassie::midge::adapter::StorageFamily::Data,
            &env.field_key("t_col", STATUS_FIELD, &third),
        )
        .expect("raw get");
    assert!(stored.is_some(), "renamed column keeps its field id key");
}

#[test]
fn should_fail_complete_traversal_on_an_orphan_field_but_not_a_bounded_limit() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 64);
    let mut orphan = vec![0x05];
    orphan.extend_from_slice(b"ghost");
    env.cassie
        .midge
        .raw_put(
            cassie::midge::adapter::StorageFamily::Data,
            &env.field_key("wide_col", BIG_FIELD, "zz-ghost"),
            &orphan,
        )
        .expect("orphan field key");

    // Act
    let complete = observe(&env, "SELECT id FROM wide_col").expect_err("orphan must fail");
    let bounded = observed(&env, "SELECT id FROM wide_col LIMIT 3");
    let exists = observed(&env, "SELECT EXISTS (SELECT 1 FROM wide_col)");

    // Assert
    assert_eq!(complete.sql_state(), "XX001");
    assert!(complete.to_string().contains("zz-ghost"), "{complete}");
    assert_eq!(bounded.rows.len(), 3);
    assert_eq!(bounded.ownership_reads, 0);
    assert_eq!(exists.rows, vec![vec![Value::Bool(true)]]);
    assert_eq!(exists.ownership_reads, 0);
}

#[test]
fn should_not_visit_corruption_outside_the_rows_a_limit_reads() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 64);
    let last = env
        .rows("SELECT _id FROM wide_col ORDER BY _id DESC LIMIT 1")
        .remove(0)
        .remove(0);
    let Value::String(last) = last else {
        panic!("row id must be text: {last:?}");
    };
    env.cassie
        .midge
        .raw_delete(
            cassie::midge::adapter::StorageFamily::Data,
            &env.field_key("wide_col", STATUS_FIELD, &last),
        )
        .expect("delete selected field");

    // Act
    let bounded = observed(&env, "SELECT id, status FROM wide_col LIMIT 2");
    let complete = observe(&env, "SELECT id, status FROM wide_col").expect_err("must fail");

    // Assert
    assert_eq!(bounded.rows.len(), 2);
    assert_eq!(bounded.ownership_reads, 0);
    assert_eq!(complete.sql_state(), "XX001");
    assert!(complete.to_string().contains(&last), "{complete}");
}

#[test]
fn should_keep_the_missing_field_error_without_decoding_unrelated_values() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 8_192);
    let first = env
        .rows("SELECT _id FROM wide_col ORDER BY _id LIMIT 1")
        .remove(0)
        .remove(0);
    let Value::String(first) = first else {
        panic!("row id must be text: {first:?}");
    };
    env.cassie
        .midge
        .raw_delete(
            cassie::midge::adapter::StorageFamily::Data,
            &env.field_key("wide_col", STATUS_FIELD, &first),
        )
        .expect("delete selected field");
    let before = env.cassie.midge.column_field_reads_for_diagnostics();

    // Act
    let error = observe(&env, "SELECT id, status FROM wide_col").expect_err("must fail");
    let work = env
        .cassie
        .midge
        .column_field_reads_for_diagnostics()
        .since(before);

    // Assert
    assert_eq!(error.sql_state(), "XX001");
    assert!(error.to_string().contains("status"), "{error}");
    assert!(error.to_string().contains(&first), "{error}");
    assert!(
        work.payload_bytes < 8_192,
        "an unselected payload was fetched: {work:?}"
    );
    assert!(work.decodes <= 2 * ROWS as u64, "{work:?}");
}

#[test]
fn should_enforce_the_query_budget_for_selected_values_not_unselected_payloads() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 256 * 1024;
    let config = crate::support_sql::with_test_root_password(config);
    let cassie = cassie::app::Cassie::new_in_memory_with_config(config).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let env = Env { cassie, session };
    env.run("CREATE TABLE wide_col (id INT, status TEXT, big TEXT) USING columnar");
    for index in 0..3 {
        env.cassie
            .midge
            .put_document(
                "wide_col",
                Some(format!("row-{index}")),
                serde_json::json!({"id": index, "status": "ok", "big": "b".repeat(200 * 1024)}),
            )
            .expect("seed wide row");
    }

    // Act
    let narrow = observed(&env, "SELECT id, status FROM wide_col");
    let wide = observe(&env, "SELECT id, big FROM wide_col");

    // Assert
    assert_eq!(narrow.rows.len(), 3);
    assert!(narrow.fields.payload_bytes < 1_024, "{:?}", narrow.fields);
    assert!(
        matches!(wide, Err(CassieError::ResourceLimit(_))),
        "selecting the wide field must exceed the budget"
    );
}

#[test]
fn should_read_transaction_overlays_like_row_tables_without_fetching_unselected_payloads() {
    // Arrange
    let env = env();
    create_wide(&env, "wide_col", 8_192);
    env.run("CREATE TABLE wide_row (id INT, status TEXT, big TEXT, extra TEXT)");
    insert_wide(&env, "wide_row", 8_192);
    let mut results = Vec::new();

    // Act
    for table in ["wide_row", "wide_col"] {
        env.run("BEGIN");
        env.run(&format!(
            "INSERT INTO {table} VALUES (100, 'new', 'x', NULL)"
        ));
        env.run(&format!(
            "UPDATE {table} SET status = 'changed' WHERE id = 3"
        ));
        env.run(&format!("DELETE FROM {table} WHERE id = 4"));
        let inside = observed(&env, &format!("SELECT id, status FROM {table} ORDER BY id"));
        env.run("ROLLBACK");
        let after = observed(&env, &format!("SELECT id, status FROM {table} ORDER BY id"));
        results.push((inside, after));
    }

    // Assert
    let (row_inside, row_after) = &results[0];
    let (col_inside, col_after) = &results[1];
    assert_eq!(col_inside.rows, row_inside.rows);
    assert_eq!(col_after.rows, row_after.rows);
    assert_eq!(col_after.rows.len(), ROWS);
    assert!(
        col_inside.fields.payload_bytes < 8_192,
        "{:?}",
        col_inside.fields
    );
    assert!(
        col_after.fields.payload_bytes < 8_192,
        "{:?}",
        col_after.fields
    );
}
