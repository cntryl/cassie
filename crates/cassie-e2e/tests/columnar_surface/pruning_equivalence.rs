//! Issue #1357: pruned column-table scans return exactly the values of the
//! row table and of a forced unpruned column read, for NULLs, extrema, floats,
//! text, booleans and fields added after rows were written.

use super::pair::{with_pair, Pair, COL, ROW};

const ROWS: usize = 1_300;
const CHUNK: usize = 260;
const COLUMNS: &str = "a BIGINT, s TEXT, b BOOLEAN, n INT, note TEXT";

fn row(index: usize) -> String {
    let a = match index {
        0 => "-9223372036854775807".to_owned(),
        i if i == ROWS - 1 => "9223372036854775807".to_owned(),
        i if i % 50 == 7 => "NULL".to_owned(),
        i => (i * 10).to_string(),
    };
    let s = if index.is_multiple_of(97) {
        "NULL".to_owned()
    } else {
        format!("'s-{:02}'", index / 100)
    };
    let b = index >= 1_000;
    let n = if index >= 1_100 {
        "NULL".to_owned()
    } else {
        (index % 5).to_string()
    };
    let note = format!("{}-{index}", "xyz".repeat(80));
    format!("({a}, {s}, {b}, {n}, '{note}')")
}

fn load(pair: &Pair) {
    for (mode, suffix) in [(ROW, ""), (COL, " WITH (storage = column_store)")] {
        pair.run(&format!("CREATE TABLE t_{mode} ({COLUMNS}){suffix}"));
        for start in (0..ROWS).step_by(CHUNK) {
            let values = (start..start + CHUNK)
                .map(row)
                .collect::<Vec<_>>()
                .join(",");
            pair.run(&format!("INSERT INTO t_{mode} VALUES {values}"));
        }
    }
    pair.cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics("t_col")
        .expect("summaries");
}

/// Asserts the heap table, the pruned column table and the forced unpruned
/// column table return the same rows; returns the segments the pruned read skipped.
fn assert_same(pair: &Pair, projection: &str, predicate: &str) -> u64 {
    let midge = &pair.cassie.midge;
    let select = |table: &str| format!("SELECT {projection} FROM {table} WHERE {predicate}");
    let heap = pair.sorted(&select("t_row"));
    let before = midge.column_pruning_for_diagnostics();
    let pruned = pair.sorted(&select("t_col"));
    let skipped = midge
        .column_pruning_for_diagnostics()
        .since(before)
        .segments_skipped;
    midge.set_column_pruning_enabled_for_diagnostics(false);
    let unpruned = pair.sorted(&select("t_col"));
    midge.set_column_pruning_enabled_for_diagnostics(true);
    assert_eq!(pruned, heap, "pruned vs heap: {predicate}");
    assert_eq!(unpruned, heap, "unpruned vs heap: {predicate}");
    skipped
}

#[test]
fn should_match_heap_for_numeric_predicates() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let predicates = [
            "a = 500",
            "a = 9223372036854775807",
            "a > 9223372036854775806",
            "a <= -9223372036854775806",
            "a BETWEEN 3000 AND 3100",
            "a >= 12000",
            "a < 0",
            "a > 5.5 AND a < 40.5",
            "a IS NULL",
            "a IS NOT NULL AND a < 100",
            "n IS NULL",
            "n IS NOT NULL AND a > 12000",
            "n = 3 AND a >= 100 AND a <= 4000",
            "a >= 0",
        ];

        // Act
        let skipped = predicates
            .iter()
            .map(|predicate| assert_same(pair, "a, n", predicate))
            .sum::<u64>();

        // Assert
        assert!(skipped > 0, "the sorted fixture must exercise pruning");
    });
}

#[test]
fn should_match_heap_for_text_predicates() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let predicates = [
            "s = 's-05'",
            "s >= 's-10'",
            "s < 's-02'",
            "s = 'absent'",
            "s IS NULL",
            "s IS NOT NULL AND a >= 11000",
            "b = true",
            "b = false AND a >= 5000",
            "a >= 11000 AND s <= 's-12'",
        ];

        // Act
        let skipped = predicates
            .iter()
            .map(|predicate| assert_same(pair, "a, s, b", predicate))
            .sum::<u64>();

        // Assert
        assert!(skipped > 0, "the sorted fixture must exercise pruning");
    });
}

#[test]
fn should_match_heap_for_compressed_values_under_a_pruned_predicate() {
    // Arrange
    with_pair(|pair| {
        load(pair);

        // Act
        let skipped = assert_same(pair, "a, note", "a < 1000");

        // Assert
        assert!(skipped > 0);
    });
}

#[test]
fn should_match_heap_for_fields_added_after_the_rows() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        for mode in [ROW, COL] {
            pair.run(&format!("ALTER TABLE t_{mode} ADD COLUMN d INT DEFAULT 7"));
            pair.run(&format!("ALTER TABLE t_{mode} ADD COLUMN e INT"));
        }
        let predicates = [
            "d = 7",
            "d > 7",
            "d < 7",
            "d IS NULL",
            "e IS NULL",
            "e IS NOT NULL",
            "e = 1",
            "d = 7 AND a >= 12000",
        ];

        // Act
        for predicate in predicates {
            assert_same(pair, "a, d, e", predicate);
        }
        for mode in [ROW, COL] {
            pair.run(&format!(
                "INSERT INTO t_{mode} VALUES (99999, 's-99', true, 1, 'x', 9, 1)"
            ));
        }
        let skipped = predicates
            .iter()
            .map(|predicate| assert_same(pair, "a, d, e", predicate))
            .sum::<u64>();

        // Assert
        assert!(skipped > 0, "summaries rebuild after the schema change");
    });
}

#[test]
fn should_match_heap_after_mixed_writes() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let predicates = ["a < 0", "a BETWEEN 100 AND 300", "a >= 12000", "s = 's-03'"];
        for mode in [ROW, COL] {
            pair.run(&format!("UPDATE t_{mode} SET a = -5 WHERE a = 100"));
            pair.run(&format!("UPDATE t_{mode} SET a = 13000 WHERE a = 200"));
            pair.run(&format!(
                "DELETE FROM t_{mode} WHERE a BETWEEN 2000 AND 2400"
            ));
            pair.run(&format!(
                "INSERT INTO t_{mode} VALUES (150, 's-03', false, 2, 'late'), (NULL, 's-03', true, NULL, 'late')"
            ));
        }

        // Act
        for predicate in predicates {
            assert_same(pair, "a, s", predicate);
        }
        pair.run("BEGIN");
        for mode in [ROW, COL] {
            pair.run(&format!("UPDATE t_{mode} SET a = -7 WHERE a = 300"));
            pair.run(&format!(
                "INSERT INTO t_{mode} VALUES (-9, 's-00', true, 1, 'staged')"
            ));
        }
        let staged_heap = pair.sorted("SELECT a, s FROM t_row WHERE a < 0");
        let staged_col = pair.sorted("SELECT a, s FROM t_col WHERE a < 0");
        pair.run("ROLLBACK");

        // Assert
        assert_eq!(staged_col, staged_heap);
        assert!(staged_col.len() >= 3);
    });
}

#[test]
fn should_return_staged_rows_moved_into_a_segment_the_summaries_exclude() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        pair.run("BEGIN");
        for mode in [ROW, COL] {
            pair.run(&format!("UPDATE t_{mode} SET s = 's-12' WHERE a = 500"));
            pair.run(&format!("UPDATE t_{mode} SET s = 's-00' WHERE a = 12500"));
            pair.run(&format!("DELETE FROM t_{mode} WHERE a = 12400"));
        }
        let midge = &pair.cassie.midge;
        let before = midge.column_pruning_for_diagnostics();

        // Act
        let staged_col = pair.sorted("SELECT a, s FROM t_col WHERE s = 's-12'");
        let skipped = midge
            .column_pruning_for_diagnostics()
            .since(before)
            .segments_skipped;
        let staged_heap = pair.sorted("SELECT a, s FROM t_row WHERE s = 's-12'");
        pair.run("ROLLBACK");

        // Assert
        assert_eq!(staged_col, staged_heap);
        assert!(
            staged_col
                .iter()
                .any(|row| row[0] == cassie::types::Value::Int64(500)),
            "the staged row moved out of an excluded segment must be returned"
        );
        assert!(skipped > 0, "the persisted side must still prune");
    });
}
