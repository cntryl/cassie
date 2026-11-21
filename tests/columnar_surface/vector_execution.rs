//! Issue #1358: eligible queries over column tables read vector batches (no row
//! documents), equal the heap tables, and report why the rest read documents.

use super::pair::{with_pair, with_pair_config, Pair, COL, ROW};
use cassie::midge::adapter::ColumnVectorReadSnapshot;
use cassie::types::Value;

const COLUMNS: &str = "g TEXT, k INT, v INT, f FLOAT, b BOOLEAN";
const ROWS: &str = "('a',1,10,1.5,true),('a',1,NULL,2.5,false),('b',2,30,NULL,true),\
    ('b',NULL,30,4.5,NULL),(NULL,2,50,5.5,false),('c',NULL,NULL,NULL,true),\
    (NULL,NULL,-7,0.25,NULL),('a',1,10,1.5,true)";
const SHAPES: [&str; 9] = [
    "SELECT COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v) FROM {t}",
    "SELECT COUNT(f), SUM(f), MIN(f), MAX(f) FROM {t}",
    "SELECT g, COUNT(*), SUM(v), MIN(v), MAX(v) FROM {t} GROUP BY g",
    "SELECT k, COUNT(v), SUM(f), MAX(f) FROM {t} GROUP BY k",
    "SELECT SUM(v) FROM {t} WHERE k = 1",
    "SELECT g, MAX(v) FROM {t} WHERE v > 5 AND k IS NOT NULL GROUP BY g",
    "SELECT v, f FROM {t} WHERE v >= 10",
    "SELECT g, b FROM {t} WHERE f IS NOT NULL",
    "SELECT COUNT(*) FROM {t} WHERE v > 1000",
];

fn table(template: &str, mode: &str) -> String {
    Pair::resolved(template, "t", mode)
}

fn vector(pair: &Pair) -> ColumnVectorReadSnapshot {
    pair.cassie.midge.column_vector_reads_for_diagnostics()
}

fn plan(pair: &Pair, sql: &str) -> String {
    match &pair.run(&format!("EXPLAIN {sql}"))[0][0] {
        Value::String(plan) => plan.clone(),
        other => panic!("EXPLAIN returned {other:?}"),
    }
}

fn token<'a>(plan: &'a str, name: &str) -> &'a str {
    plan.split_ascii_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("token {name} absent from {plan}"))
}

fn sorted(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

#[test]
fn should_equal_the_heap_table_for_every_issue_shape() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        for shape in SHAPES {
            // Act
            let heap = sorted(pair.run(&table(shape, ROW)));
            let column = sorted(pair.run(&table(shape, COL)));

            // Assert
            assert_eq!(column, heap, "{shape}");
        }
    });
}

#[test]
fn should_read_every_issue_shape_through_vector_batches_without_row_documents() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        for shape in &SHAPES[..8] {
            let before = vector(pair);

            // Act
            pair.run(&table(shape, COL));
            let delta = vector(pair).since(before);

            // Assert
            assert!(delta.batches > 0, "{shape}: batches");
            assert_eq!(delta.row_documents_built, 0, "{shape}: row documents");
        }
    });
}

#[test]
fn should_read_row_documents_for_the_same_query_when_vector_batches_are_disabled() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        let sql = table(SHAPES[0], COL);
        let vector_rows = pair.run(&sql);
        pair.cassie
            .midge
            .set_column_vector_enabled_for_diagnostics(false);
        let before = vector(pair);

        // Act
        let document_rows = pair.run(&sql);
        let delta = vector(pair).since(before);
        pair.cassie
            .midge
            .set_column_vector_enabled_for_diagnostics(true);

        // Assert
        assert_eq!(document_rows, vector_rows);
        assert_eq!(delta.batches, 0);
        assert!(delta.row_documents_built > 0);
    });
}

#[test]
fn should_report_the_vector_path_in_the_plan_output() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        let sql = table("SELECT g, SUM(v) FROM {t} GROUP BY g", COL);

        // Act
        let explained = plan(pair, &sql);
        let analyzed = plan(pair, &format!("ANALYZE {sql}"));

        // Assert
        assert_eq!(token(&explained, "column_vector"), "eligible");
        assert_eq!(token(&explained, "column_vector_reason"), "none");
        assert!(
            token(&analyzed, "column_vector_batches_delta")
                .parse::<u64>()
                .expect("number")
                > 0
        );
        assert_eq!(token(&analyzed, "row_documents_delta"), "0");
    });
}

#[test]
fn should_report_not_applicable_for_a_heap_table() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);

        // Act
        let explained = plan(pair, &table(SHAPES[2], ROW));

        // Assert
        assert_eq!(token(&explained, "column_vector"), "not_applicable");
    });
}

struct Fallback {
    sql: String,
    token: String,
    reason: String,
    batches: u64,
}

fn observe_fallback(pair: &Pair, sql: &str) -> Fallback {
    let explained = plan(pair, sql);
    let before = vector(pair);
    pair.run(sql);
    Fallback {
        sql: sql.to_owned(),
        token: token(&explained, "column_vector").to_owned(),
        reason: token(&explained, "column_vector_reason").to_owned(),
        batches: vector(pair).since(before).batches,
    }
}

fn assert_fallback(observed: &Fallback, reason: &str) {
    assert_eq!(observed.token, "fallback", "{}", observed.sql);
    assert_eq!(observed.reason, reason, "{}", observed.sql);
    assert_eq!(
        observed.batches, 0,
        "{}: a fallback reads no vector batch",
        observed.sql
    );
}

#[test]
fn should_fall_back_with_the_unsupported_type_label() {
    // Arrange
    with_pair(|pair| {
        pair.run("CREATE TABLE j_col (id INT, doc JSONB) WITH (storage = column_store)");
        pair.run("INSERT INTO j_col VALUES (1, '{\"a\":1}')");

        // Act
        let observed = observe_fallback(pair, "SELECT doc FROM j_col");

        // Assert
        assert_fallback(&observed, "column-vector-unsupported-type:jsonb");
    });
}

#[test]
fn should_fall_back_with_the_unsupported_expression_label() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);

        // Act
        let observed =
            observe_fallback(pair, &table("SELECT v FROM {t} WHERE lower(g) = 'a'", COL));

        // Assert
        assert_fallback(&observed, "column-vector-unsupported-expression");
    });
}

#[test]
fn should_fall_back_with_the_unsupported_aggregate_shape_label() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);

        // Act
        let observed = observe_fallback(pair, &table("SELECT COUNT(DISTINCT v) FROM {t}", COL));

        // Assert
        assert_fallback(&observed, "column-vector-unsupported-aggregate-shape");
    });
}

#[test]
fn should_fall_back_with_the_unsupported_join_label() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);

        // Act
        let observed = observe_fallback(pair, "SELECT a.v FROM t_col a JOIN t_col b ON a.k = b.k");

        // Assert
        assert_fallback(&observed, "column-vector-unsupported-join");
    });
}

#[test]
fn should_fall_back_with_the_unsupported_transport_budget_label() {
    // Arrange
    with_pair_config(
        |config| config.limits.query_memory_budget_bytes = 12 * 1_024,
        |pair| {
            pair.create("t", COLUMNS, ROWS);

            // Act
            let explained = plan(pair, &table("SELECT v, f FROM {t} WHERE v > 1", COL));

            // Assert
            assert_eq!(token(&explained, "column_vector"), "fallback");
            assert_eq!(
                token(&explained, "column_vector_reason"),
                "column-vector-unsupported-transport-budget"
            );
        },
    );
}

fn transaction_script(mode: &str) -> Vec<String> {
    [
        "INSERT INTO {t} VALUES ('d',4,40,4.0,true),('a',1,5,0.5,false)",
        "UPDATE {t} SET v = 99 WHERE g = 'b' AND k = 2",
        "DELETE FROM {t} WHERE g = 'c'",
    ]
    .iter()
    .map(|sql| table(sql, mode))
    .collect()
}

#[test]
fn should_equal_the_heap_table_with_a_transactions_staged_rows_merged() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        pair.run("BEGIN");
        for mode in [ROW, COL] {
            for sql in transaction_script(mode) {
                pair.run(&sql);
            }
        }
        let before = vector(pair);

        // Act
        let results = SHAPES
            .iter()
            .map(|shape| {
                (
                    *shape,
                    sorted(pair.run(&table(shape, ROW))),
                    sorted(pair.run(&table(shape, COL))),
                )
            })
            .collect::<Vec<_>>();
        let delta = vector(pair).since(before);

        // Assert
        for (shape, heap, column) in results {
            assert_eq!(column, heap, "{shape}");
        }
        assert!(delta.staged_rows_merged > 0, "staged rows merged");
        assert!(delta.batches > 0);
        assert_eq!(delta.row_documents_built, 0);
        pair.run("ROLLBACK");
    });
}

#[test]
fn should_see_no_staged_rows_after_the_transaction_rolls_back() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        let sql = table(SHAPES[0], COL);
        let baseline = pair.run(&sql);
        pair.run("BEGIN");
        for statement in transaction_script(COL) {
            pair.run(&statement);
        }
        pair.run("ROLLBACK");

        // Act
        let after = pair.run(&sql);

        // Assert
        assert_eq!(after, baseline);
    });
}

#[test]
fn should_keep_a_repeatable_read_snapshot_stable_across_a_concurrent_commit() {
    // Arrange
    with_pair(|pair| {
        pair.create("t", COLUMNS, ROWS);
        let other = pair.cassie.create_session("other", None);
        let sql = table("SELECT COUNT(*), SUM(v) FROM {t}", COL);
        let heap_sql = table("SELECT COUNT(*), SUM(v) FROM {t}", ROW);
        pair.run("BEGIN ISOLATION LEVEL REPEATABLE READ");
        let first = pair.run(&sql);
        let heap_first = pair.run(&heap_sql);

        // Act
        for mode in [ROW, COL] {
            pair.cassie
                .execute_sql(
                    &other,
                    &table("INSERT INTO {t} VALUES ('z',9,1000,1.0,true)", mode),
                    vec![],
                )
                .expect("concurrent insert");
        }
        let second = pair.run(&sql);
        let heap_second = pair.run(&heap_sql);
        pair.run("COMMIT");
        let committed = pair.run(&sql);

        // Assert
        assert_eq!(first, heap_first);
        assert_eq!(second, first, "the snapshot ignores the later commit");
        assert_eq!(heap_second, heap_first);
        assert_ne!(committed, first, "a new statement sees the commit");
    });
}

const BOUNDARY_ROWS: usize = 1_025;

fn boundary_row(index: usize) -> String {
    let v = match index {
        1_024 => "9000".to_owned(),
        i if i % 7 == 3 => "NULL".to_owned(),
        i => (i % 100).to_string(),
    };
    let g = if index < 1_024 { "early" } else { "late" };
    format!("('{g}', {}, {v}, NULL, NULL)", index % 3)
}

#[test]
fn should_fold_a_group_across_a_batch_boundary_with_the_only_extremum_in_the_second_batch() {
    // Arrange
    with_pair(|pair| {
        let values = (0..BOUNDARY_ROWS)
            .map(boundary_row)
            .collect::<Vec<_>>()
            .join(",");
        pair.create("t", COLUMNS, &values);
        let sql = table(
            "SELECT k, COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v) FROM {t} GROUP BY k",
            COL,
        );
        let heap = sorted(pair.run(&table(
            "SELECT k, COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v) FROM {t} GROUP BY k",
            ROW,
        )));
        let before = vector(pair);

        // Act
        let column = sorted(pair.run(&sql));
        let delta = vector(pair).since(before);

        // Assert
        assert_eq!(column, heap);
        assert!(column.iter().any(|row| row[5] == Value::Int64(9000)));
        assert_eq!(
            delta.batches, 2,
            "1025 rows are two batches of at most 1024"
        );
        assert_eq!(delta.row_documents_built, 0);
    });
}

#[test]
fn should_return_the_ungrouped_extremum_that_only_the_second_batch_holds() {
    // Arrange
    with_pair(|pair| {
        let values = (0..BOUNDARY_ROWS)
            .map(boundary_row)
            .collect::<Vec<_>>()
            .join(",");
        pair.create("t", COLUMNS, &values);

        // Act
        let column = pair.run(&table("SELECT MAX(v), COUNT(v) FROM {t}", COL));
        let heap = pair.run(&table("SELECT MAX(v), COUNT(v) FROM {t}", ROW));

        // Assert
        assert_eq!(column, heap);
        assert_eq!(column[0][0], Value::Int64(9000));
    });
}

#[test]
fn should_sum_beyond_the_bigint_range_exactly_like_the_heap_table() {
    // Arrange
    with_pair(|pair| {
        pair.create(
            "o",
            "v BIGINT",
            "(9223372036854775807),(9223372036854775807),(NULL)",
        );

        // Act
        let heap = pair.run("SELECT SUM(v), COUNT(v) FROM o_row");
        let column = pair.run("SELECT SUM(v), COUNT(v) FROM o_col");

        // Assert
        assert_eq!(column, heap);
    });
}

#[test]
fn should_return_the_empty_input_results_of_the_heap_table() {
    // Arrange
    with_pair(|pair| {
        pair.create("e", "v INT, g TEXT", "");

        // Act
        let queries = [
            "SELECT COUNT(*), SUM(v), MIN(v), MAX(v) FROM {e}",
            "SELECT g, COUNT(*) FROM {e} GROUP BY g",
        ];

        // Assert
        for query in queries {
            let sql = |mode| Pair::resolved(query, "e", mode);
            assert_eq!(pair.run(&sql(COL)), pair.run(&sql(ROW)), "{query}");
        }
    });
}
