use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{decode_column_chunk_for_test, StorageFamily};
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn metric(metrics: &serde_json::Value, name: &str) -> u64 {
    metrics["column_batches"][name].as_u64().unwrap_or_default()
}

struct EncodedScanFixture {
    cassie: Cassie,
    session: CassieSession,
}

fn corrupted_isolation_fixture(path: &str) -> EncodedScanFixture {
    let cassie = Cassie::new_with_data_dir(path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE encoded_scan_isolation (status TEXT, amount INT, ignored TEXT)",
            vec![],
        )
        .expect("create table");
    for amount in 0..8 {
        let status = if amount % 2 == 0 {
            "active"
        } else {
            "inactive"
        };
        cassie
            .execute_sql(
                &session,
                &format!(
                    "INSERT INTO encoded_scan_isolation (status, amount, ignored) \
                     VALUES ('{status}', {amount}, 'poison-{amount}')"
                ),
                vec![],
            )
            .expect("insert row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX encoded_scan_isolation_idx ON encoded_scan_isolation \
             USING column (status, amount, ignored) WITH (segment_size = 8)",
            vec![],
        )
        .expect("create column index");
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan raw column chunks");
    let (ignored_key, mut ignored_chunk) = entries
        .into_iter()
        .find(|(_, value)| {
            value.starts_with(b"CBC2")
                && decode_column_chunk_for_test(value).is_ok_and(|values| {
                    values.iter().all(|value| {
                        value
                            .as_str()
                            .is_some_and(|value| value.starts_with("poison-"))
                    })
                })
        })
        .expect("find ignored field chunk");
    let last = ignored_chunk
        .last_mut()
        .expect("encoded chunk should not be empty");
    *last ^= 0x80;
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("open write transaction");
    tx.put(ignored_key, ignored_chunk, None)
        .expect("corrupt ignored field");
    tx.commit(WriteOptions::sync())
        .expect("commit field corruption");
    EncodedScanFixture { cassie, session }
}

#[test]
fn should_late_materialize_selected_values_despite_unrequested_corruption() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_encoded_scan_isolation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let fixture = corrupted_isolation_fixture(&path);
        let cassie = fixture.cassie;
        let session = fixture.session;
        let before = cassie.metrics();

        // Act
        let covered = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM encoded_scan_isolation \
                 WHERE status = 'active' ORDER BY amount",
                vec![],
            )
            .expect("scan covered fields");
        let after_covered = cassie.metrics();
        let required = cassie
            .execute_sql(
                &session,
                "SELECT ignored FROM encoded_scan_isolation \
                 WHERE status = 'active' ORDER BY amount",
                vec![],
            )
            .expect("fall back for corrupt required field");
        let after_required = cassie.metrics();

        // Assert
        assert_eq!(
            covered.rows,
            vec![
                vec![Value::Int64(0)],
                vec![Value::Int64(2)],
                vec![Value::Int64(4)],
                vec![Value::Int64(6)],
            ]
        );
        assert_eq!(
            metric(&after_covered, "scans") - metric(&before, "scans"),
            1
        );
        assert_eq!(
            metric(&after_covered, "chunks_read") - metric(&before, "chunks_read"),
            3
        );
        assert_eq!(
            metric(&after_covered, "predicate_values") - metric(&before, "predicate_values"),
            8
        );
        assert_eq!(
            metric(&after_covered, "selected_rows") - metric(&before, "selected_rows"),
            4
        );
        assert_eq!(
            metric(&after_covered, "materialized_values") - metric(&before, "materialized_values"),
            8
        );
        assert_eq!(
            metric(&after_covered, "decode_fallbacks") - metric(&before, "decode_fallbacks"),
            0
        );
        assert_eq!(
            required.rows,
            vec![
                vec![Value::String("poison-0".to_string())],
                vec![Value::String("poison-2".to_string())],
                vec![Value::String("poison-4".to_string())],
                vec![Value::String("poison-6".to_string())],
            ]
        );
        assert_eq!(
            metric(&after_required, "decode_fallbacks")
                - metric(&after_covered, "decode_fallbacks"),
            1
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_encoded_conjunctions_with_null_predicates_exactly() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_encoded_conjunctions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE encoded_conjunctions (label TEXT, score INT, note TEXT)",
                vec![],
            )
            .expect("create table");
        for score in 0..6 {
            let note = if score == 3 {
                "NULL".to_string()
            } else {
                format!("'note-{score}'")
            };
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO encoded_conjunctions (label, score, note) \
                         VALUES ('row-{score}', {score}, {note})"
                    ),
                    vec![],
                )
                .expect("insert row");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX encoded_conjunctions_idx ON encoded_conjunctions \
                 USING column (label, score, note) WITH (segment_size = 6)",
                vec![],
            )
            .expect("create column index");
        let before = cassie.metrics();

        // Act
        let non_null = cassie
            .execute_sql(
                &session,
                "SELECT label FROM encoded_conjunctions \
                 WHERE score >= 2 AND score < 5 AND note IS NOT NULL ORDER BY label",
                vec![],
            )
            .expect("execute non-null conjunction");
        let null = cassie
            .execute_sql(
                &session,
                "SELECT label FROM encoded_conjunctions \
                 WHERE score >= 2 AND score < 5 AND note IS NULL ORDER BY label",
                vec![],
            )
            .expect("execute null conjunction");
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            non_null.rows,
            vec![
                vec![Value::String("row-2".to_string())],
                vec![Value::String("row-4".to_string())],
            ]
        );
        assert_eq!(null.rows, vec![vec![Value::String("row-3".to_string())]]);
        assert_eq!(metric(&after, "scans") - metric(&before, "scans"), 2);
        assert_eq!(
            metric(&after, "fallback_scans") - metric(&before, "fallback_scans"),
            0
        );
        assert_eq!(
            metric(&after, "selected_rows") - metric(&before, "selected_rows"),
            3
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
