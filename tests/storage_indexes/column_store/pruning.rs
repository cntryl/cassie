//! Pruning of authoritative column-table scans (#1357).
//!
//! Segment summaries are derived metadata. These tests prove that a verified
//! summary skips a segment with zero payload reads and decodes, that sorted
//! input prunes while shuffled or unselective input reads everything, that an
//! update which moves a value between predicate ranges is never missed, and
//! that a corrupt or stale summary only ever costs a normal read.

use super::{env, Env};
use cassie::midge::adapter::{ColumnFieldReadSnapshot, ColumnPruningSnapshot, StorageFamily};
use cassie::types::Value;

pub(super) const TABLE: &str = "events";
const ROWS: usize = 1_280;
const CHUNK: usize = 256;
/// Rows per segment after a full rebuild of `ROWS` rows: 256, 256, 256, 512.
const LEADING_SEGMENT_ROWS: u64 = 768;

pub(super) struct Observed {
    pub rows: Vec<Vec<Value>>,
    pub fields: ColumnFieldReadSnapshot,
    pub pruning: ColumnPruningSnapshot,
}

pub(super) fn observe(env: &Env, sql: &str) -> Observed {
    let midge = &env.cassie.midge;
    let fields = midge.column_field_reads_for_diagnostics();
    let pruning = midge.column_pruning_for_diagnostics();
    let rows = env.rows(sql);
    Observed {
        rows,
        fields: midge.column_field_reads_for_diagnostics().since(fields),
        pruning: midge.column_pruning_for_diagnostics().since(pruning),
    }
}

pub(super) fn ints(rows: &[Vec<Value>], column: usize) -> Vec<i64> {
    let mut values = rows
        .iter()
        .map(|row| match &row[column] {
            Value::Int64(value) => *value,
            other => panic!("expected an integer, found {other:?}"),
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

fn tenant(index: usize) -> String {
    format!("tenant-{:02}", index / CHUNK)
}

/// Rows `0..ROWS` in insertion order: ascending `amount`, tenant per 256 rows,
/// and NULL notes only in the last 180 rows.
pub(super) fn sorted_row(index: usize) -> String {
    let note = if index >= 1_100 {
        "NULL".to_owned()
    } else {
        "'note'".to_owned()
    };
    format!("({index}, '{}', {note})", tenant(index))
}

/// The same values with `amount` a permutation, so every segment spans the range.
fn shuffled_row(index: usize) -> String {
    let amount = (index * 7_919) % ROWS;
    let note = if index % 7 == 0 { "NULL" } else { "'note'" };
    let tenant = format!("tenant-{:02}", (index * 37) % 5);
    format!("({amount}, '{tenant}', {note})")
}

pub(super) fn load(env: &Env, table: &str, row: impl Fn(usize) -> String) {
    env.run(&format!(
        "CREATE TABLE {table} (amount INT, tenant TEXT, note TEXT) USING columnar"
    ));
    for start in (0..ROWS).step_by(CHUNK) {
        let values = (start..start + CHUNK)
            .map(&row)
            .collect::<Vec<_>>()
            .join(",");
        env.run(&format!("INSERT INTO {table} VALUES {values}"));
    }
    env.cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics(table)
        .expect("rebuild summaries");
}

pub(super) fn summary_entries(env: &Env, table: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let prefix = env
        .cassie
        .midge
        .column_store_summary_prefix_for_diagnostics(table)
        .expect("summary prefix");
    env.cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("summary scan")
}

fn payload_reads(segment_rows: u64, fields: u64) -> u64 {
    segment_rows * fields
}

#[test]
fn should_skip_leading_segments_with_zero_payload_reads_on_a_sorted_numeric_range() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    assert_eq!(summary_entries(&env, TABLE).len(), 4);
    let sql = "SELECT amount, tenant FROM events WHERE amount >= 1000";
    // The result cache keys on the statement text, so the unpruned read uses an
    // equivalent spelling.
    let equivalent = "SELECT amount, tenant FROM events WHERE amount > 999";

    // Act
    let pruned = observe(&env, sql);
    env.cassie.midge.set_column_pruning_enabled_for_diagnostics(false);
    let unpruned = observe(&env, equivalent);

    // Assert
    assert_eq!(ints(&pruned.rows, 0), (1_000..1_280).collect::<Vec<_>>());
    assert_eq!(ints(&unpruned.rows, 0), ints(&pruned.rows, 0));
    assert_eq!(pruned.pruning.segments_skipped, 3);
    assert_eq!(pruned.pruning.rows_skipped, LEADING_SEGMENT_ROWS);
    assert_eq!(pruned.pruning.summaries_stale, 0);
    assert_eq!(
        pruned.fields.payload_reads,
        payload_reads((ROWS as u64) - LEADING_SEGMENT_ROWS, 2)
    );
    assert_eq!(pruned.fields.decodes, pruned.fields.payload_reads);
    assert_eq!(unpruned.fields.payload_reads, payload_reads(ROWS as u64, 2));
    assert_eq!(unpruned.pruning.segments_skipped, 0);
}

#[test]
fn should_skip_segments_on_sorted_repeated_string_equality() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let sql = "SELECT amount, tenant FROM events WHERE tenant = 'tenant-04'";

    // Act
    let pruned = observe(&env, sql);

    // Assert
    assert_eq!(ints(&pruned.rows, 0), (1_024..1_280).collect::<Vec<_>>());
    assert_eq!(pruned.pruning.segments_skipped, 3);
    assert_eq!(pruned.pruning.rows_skipped, LEADING_SEGMENT_ROWS);
    assert_eq!(
        pruned.fields.payload_reads,
        payload_reads((ROWS as u64) - LEADING_SEGMENT_ROWS, 2)
    );
}

#[test]
fn should_skip_segments_without_nulls_for_is_null() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let sql = "SELECT amount FROM events WHERE note IS NULL";

    // Act
    let pruned = observe(&env, sql);

    // Assert
    assert_eq!(ints(&pruned.rows, 0), (1_100..1_280).collect::<Vec<_>>());
    assert_eq!(pruned.pruning.segments_skipped, 3);
    assert_eq!(
        pruned.fields.payload_reads,
        payload_reads((ROWS as u64) - LEADING_SEGMENT_ROWS, 2)
    );
}

#[test]
fn should_read_every_segment_for_shuffled_input_and_unselective_predicates() {
    // Arrange
    let env = env();
    load(&env, "shuffled", shuffled_row);
    load(&env, TABLE, sorted_row);
    let shuffled_range = "SELECT amount FROM shuffled WHERE amount >= 1000";
    let shuffled_text = "SELECT amount FROM shuffled WHERE tenant = 'tenant-03'";
    let unselective = "SELECT amount FROM events WHERE amount >= 0";

    // Act
    let range = observe(&env, shuffled_range);
    let text = observe(&env, shuffled_text);
    let all = observe(&env, unselective);

    // Assert
    assert_eq!(range.rows.len(), 280);
    assert_eq!(text.rows.len(), (0..ROWS).filter(|i| (i * 37) % 5 == 3).count());
    assert_eq!(all.rows.len(), ROWS);
    for (observed, fields) in [(&range, 1), (&text, 2), (&all, 1)] {
        assert_eq!(observed.pruning.segments_skipped, 0);
        assert_eq!(observed.pruning.rows_skipped, 0);
        assert_eq!(observed.fields.payload_reads, payload_reads(ROWS as u64, fields));
    }
}

#[test]
fn should_find_a_value_moved_between_predicate_ranges_by_an_update() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let high = "SELECT amount FROM events WHERE amount = 1100";
    let moved_from = "SELECT amount FROM events WHERE amount = 10";
    let before = observe(&env, high);

    // Act
    env.run("UPDATE events SET amount = 1100 WHERE amount = 10");
    let after = observe(&env, high);
    let old = observe(&env, moved_from);

    // Assert
    assert_eq!(ints(&before.rows, 0), vec![1_100]);
    assert_eq!(before.pruning.segments_skipped, 3);
    assert_eq!(ints(&after.rows, 0), vec![1_100, 1_100]);
    assert_eq!(after.pruning.segments_skipped, 2);
    assert!(old.rows.is_empty());
}

#[test]
fn should_bypass_a_stale_summary_that_would_skip_the_moved_row() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let stale = summary_entries(&env, TABLE);
    env.run("UPDATE events SET amount = 1100 WHERE amount = 10");
    for (key, value) in &stale {
        env.cassie
            .midge
            .raw_put(StorageFamily::Data, key, value)
            .expect("restore stale summary");
    }

    // Act
    let result = observe(&env, "SELECT amount FROM events WHERE amount = 1100");

    // Assert
    assert_eq!(ints(&result.rows, 0), vec![1_100, 1_100]);
    assert_eq!(result.pruning.summaries_stale, 1);
    assert_eq!(result.pruning.segments_skipped, 2);
}

#[test]
fn should_bypass_a_corrupted_summary_that_would_skip_a_qualifying_row() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let sql = "SELECT amount FROM events WHERE amount <= 5";
    let equivalent = "SELECT amount FROM events WHERE amount < 6";
    let expected = observe(&env, sql);
    let (first_key, first_value) = summary_entries(&env, TABLE).remove(0);
    let mut lying = first_value.clone();
    // The first summarized field is `amount`; its minimum is the eight bytes
    // at offset 62. Claim the segment holds nothing below 5000.
    lying[62..70].copy_from_slice(&5_000_i64.to_be_bytes());
    env.cassie
        .midge
        .raw_put(StorageFamily::Data, &first_key, &lying)
        .expect("corrupt summary");

    // Act
    let corrupted = observe(&env, equivalent);

    // Assert
    assert_eq!(ints(&expected.rows, 0), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(ints(&corrupted.rows, 0), ints(&expected.rows, 0));
    assert_eq!(corrupted.pruning.summaries_rejected, 1);
    assert_eq!(corrupted.pruning.segments_skipped, 3);
}

#[test]
fn should_ignore_summaries_of_an_unknown_format_or_truncated_value() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let entries = summary_entries(&env, TABLE);
    let mut unknown_version = entries[0].1.clone();
    unknown_version[5] = 0xFF;
    env.cassie
        .midge
        .raw_put(StorageFamily::Data, &entries[0].0, &unknown_version)
        .expect("version");
    env.cassie
        .midge
        .raw_put(StorageFamily::Data, &entries[1].0, &entries[1].1[..10])
        .expect("truncate");

    // Act
    let result = observe(&env, "SELECT amount FROM events WHERE amount >= 1000");

    // Assert
    assert_eq!(ints(&result.rows, 0), (1_000..1_280).collect::<Vec<_>>());
    assert_eq!(result.pruning.summaries_rejected, 2);
    assert_eq!(result.pruning.segments_skipped, 1);
}

#[test]
fn should_rebuild_every_segment_after_a_scan_found_stale_summaries() {
    // Arrange
    let env = env();
    load(&env, TABLE, sorted_row);
    let stale = summary_entries(&env, TABLE);
    env.run("UPDATE events SET amount = 1100 WHERE amount = 10");
    for (key, value) in &stale {
        env.cassie
            .midge
            .raw_put(StorageFamily::Data, key, value)
            .expect("restore stale summary");
    }
    observe(&env, "SELECT amount FROM events WHERE amount = 1100");

    // Act
    env.run("UPDATE events SET amount = 1101 WHERE amount = 11");
    let healed = observe(&env, "SELECT amount FROM events WHERE amount >= 1100");

    // Assert
    assert_eq!(healed.pruning.summaries_stale, 0);
    assert_eq!(healed.pruning.segments_skipped, 2);
    assert_eq!(healed.rows.len(), 180 + 2);
}

#[test]
fn should_keep_pruning_and_find_the_moved_row_after_a_restart() {
    // Arrange
    let store = super::restart::Store::new("pruning_restart");
    let high = "SELECT amount FROM events WHERE amount = 1100";
    {
        let env = store.open();
        load(&env, TABLE, sorted_row);
        env.run("UPDATE events SET amount = 1100 WHERE amount = 10");
    }

    // Act
    let env = store.open();
    let after = observe(&env, high);
    let old = observe(&env, "SELECT amount FROM events WHERE amount = 10");

    // Assert
    assert_eq!(ints(&after.rows, 0), vec![1_100, 1_100]);
    assert_eq!(after.pruning.segments_skipped, 2);
    assert_eq!(after.pruning.summaries_stale, 0);
    assert!(old.rows.is_empty());
}
