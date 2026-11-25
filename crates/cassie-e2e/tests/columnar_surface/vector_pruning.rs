//! Issue #1358: the vector reader consumes #1357 segment pruning. Segments a
//! verified summary excludes are never opened, and no segment that could hold a
//! qualifying or staged row is skipped.

use super::pair::{with_pair, Pair, COL, ROW};
use cassie::midge::adapter::ColumnVectorReadSnapshot;

const ROWS: usize = 1_300;
const CHUNK: usize = 260;
const COLUMNS: &str = "a BIGINT, s TEXT, n INT";

fn row(index: usize) -> String {
    let a = if index % 50 == 7 {
        "NULL".to_owned()
    } else {
        (index * 10).to_string()
    };
    format!("({a}, 's-{:02}', {})", index / 100, index % 5)
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

fn vector(pair: &Pair) -> ColumnVectorReadSnapshot {
    pair.cassie.midge.column_vector_reads_for_diagnostics()
}

const SELECTIVE: &str = "SELECT a, n FROM t_col WHERE a >= 12000";

#[test]
fn should_decode_no_values_in_segments_the_summaries_exclude() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let midge = &pair.cassie.midge;
        midge.set_column_pruning_enabled_for_diagnostics(false);
        let before = vector(pair);
        pair.run(SELECTIVE);
        let unpruned = vector(pair).since(before);
        midge.set_column_pruning_enabled_for_diagnostics(true);
        let skipped_before = midge.column_pruning_for_diagnostics();
        let before = vector(pair);

        // Act
        pair.run(SELECTIVE);
        let pruned = vector(pair).since(before);
        let skipped = midge.column_pruning_for_diagnostics().since(skipped_before);

        // Assert
        assert!(pruned.segments_pruned > 0, "the sorted fixture prunes");
        assert_eq!(pruned.segments_pruned, skipped.segments_skipped);
        assert_eq!(
            unpruned.values_decoded - pruned.values_decoded,
            skipped.rows_skipped * 2,
            "no value of a skipped row is decoded (two fields per row)"
        );
        assert_eq!(pruned.row_documents_built, 0);
    });
}

#[test]
fn should_return_equal_rows_for_pruned_unpruned_heap_reads() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let midge = &pair.cassie.midge;
        let heap = pair.sorted(&SELECTIVE.replace("t_col", "t_row"));

        // Act
        let pruned = pair.sorted(SELECTIVE);
        midge.set_column_pruning_enabled_for_diagnostics(false);
        let unpruned = pair.sorted(SELECTIVE);
        midge.set_column_pruning_enabled_for_diagnostics(true);

        // Assert
        assert_eq!(pruned, heap);
        assert_eq!(unpruned, heap);
    });
}

#[test]
fn should_scan_a_segment_whose_summary_no_longer_matches_its_markers() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        for mode in [ROW, COL] {
            pair.run(&format!("UPDATE t_{mode} SET a = 13000 WHERE a = 100"));
        }
        let before = vector(pair);

        // Act
        let column = pair.sorted(SELECTIVE);
        let delta = vector(pair).since(before);

        // Assert
        assert_eq!(column, pair.sorted(&SELECTIVE.replace("t_col", "t_row")));
        assert!(
            column
                .iter()
                .any(|row| row[0] == cassie::types::Value::Int64(13_000)),
            "the updated row in a previously excluded segment is returned"
        );
        assert!(delta.batches > 0);
    });
}

#[test]
fn should_never_prune_a_segment_holding_a_row_the_open_transaction_staged() {
    // Arrange
    with_pair(|pair| {
        load(pair);
        let baseline_before = vector(pair);
        pair.run(SELECTIVE);
        let baseline = vector(pair).since(baseline_before);
        pair.run("BEGIN");
        for mode in [ROW, COL] {
            pair.run(&format!("UPDATE t_{mode} SET a = 13000 WHERE a = 100"));
        }
        let before = vector(pair);

        // Act
        let column = pair.sorted(SELECTIVE);
        let staged = vector(pair).since(before);
        let heap = pair.sorted(&SELECTIVE.replace("t_col", "t_row"));
        pair.run("ROLLBACK");

        // Assert
        assert_eq!(column, heap);
        assert!(
            staged.segments_pruned < baseline.segments_pruned,
            "the segment holding the staged row is read"
        );
    });
}
