//! Issue #1358: the typed grouped aggregate equals the row path on heap and column tables.

use super::pair::{with_pair, Pair, COL, ROW};
use cassie::types::Value;

const DISPATCH: &str = "typed-grouped-row-order-fold";
const COLUMNS: &str = "id INT, g TEXT, k INT, v INT, f FLOAT, b BOOLEAN";
const ROWS: &str = "(1,'a',1,10,1.5,true),(2,'a',1,NULL,2.5,false),(3,'b',2,30,NULL,true),\
    (4,'b',NULL,30,4.5,NULL),(5,NULL,2,50,5.5,false),(6,'c',NULL,NULL,NULL,true),\
    (7,NULL,NULL,-7,0.25,NULL),(8,'a',1,10,1.5,true)";

/// Appending an always-true HAVING keeps the result and forces the row path.
fn forced_row_path(sql: &str) -> String {
    format!("{sql} HAVING COUNT(*) > 0")
}

fn last_dispatch(pair: &Pair) -> String {
    pair.cassie.metrics()["parallel_aggregation"]["last_fallback_reason"]
        .as_str()
        .expect("fallback reason")
        .to_owned()
}

fn mode_sql(template: &str, mode: &str) -> String {
    Pair::resolved(template, "m", mode)
}

#[test]
fn should_match_the_row_path_for_grouped_aggregates_in_both_storage_modes() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, ROWS);
        let queries = [
            "SELECT g, COUNT(*) AS c, COUNT(v) AS cv, SUM(v) AS s, MIN(v) AS lo, MAX(v) AS hi, AVG(v) AS a FROM {m} GROUP BY g",
            "SELECT k, COUNT(*), SUM(f), MIN(f), MAX(f), AVG(f) FROM {m} GROUP BY k",
            "SELECT g, k, COUNT(f), SUM(v) FROM {m} GROUP BY g, k",
            "SELECT b, COUNT(*), COUNT(g), MAX(v) FROM {m} GROUP BY b",
            "SELECT g, COUNT(*) FROM {m} WHERE v IS NOT NULL AND k > 0 GROUP BY g",
            "SELECT COUNT(*) AS c FROM {m} GROUP BY g",
            "SELECT g FROM {m} GROUP BY g",
        ];
        for query in queries {
            let mut typed_by_mode = Vec::new();
            for mode in [ROW, COL] {
                // Act
                let typed = pair.run(&mode_sql(query, mode));
                let reason = last_dispatch(pair);
                let forced = pair.run(&forced_row_path(&mode_sql(query, mode)));
                // Assert
                assert_eq!(typed, forced, "{query} on {mode}");
                assert_eq!(reason, DISPATCH, "{query} on {mode}");
                typed_by_mode.push(typed);
            }
            assert_eq!(typed_by_mode[0], typed_by_mode[1], "{query}");
        }
    });
}

#[test]
fn should_keep_null_keys_as_their_own_group_in_row_path_order() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, ROWS);
        for mode in [ROW, COL] {
            // Act
            let rows = pair.run(&mode_sql("SELECT g, COUNT(*) FROM {m} GROUP BY g", mode));
            // Assert
            assert_eq!(
                rows,
                vec![
                    vec![Value::Null, Value::Int64(2)],
                    vec![Value::String("a".into()), Value::Int64(3)],
                    vec![Value::String("b".into()), Value::Int64(2)],
                    vec![Value::String("c".into()), Value::Int64(1)],
                ],
                "{mode}"
            );
        }
    });
}

#[test]
fn should_return_no_groups_for_empty_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, "");
        for mode in [ROW, COL] {
            // Act
            let rows = pair.run(&mode_sql(
                "SELECT g, COUNT(*), SUM(v) FROM {m} GROUP BY g",
                mode,
            ));
            // Assert
            assert!(rows.is_empty(), "{mode}");
        }
    });
}

#[test]
fn should_report_the_row_path_error_for_float_sum_overflow() {
    // Arrange
    with_pair(|pair| {
        pair.create(
            "m",
            COLUMNS,
            "(1,'a',1,1,1.0e308,true),(2,'a',1,2,1.0e308,true),(3,'b',1,3,1.5,true)",
        );
        for mode in [ROW, COL] {
            for query in [
                "SELECT g, SUM(f) FROM {m} GROUP BY g",
                "SELECT g, AVG(f) FROM {m} GROUP BY g",
            ] {
                // Act
                let typed = pair
                    .try_run(&mode_sql(query, mode))
                    .map_err(|e| e.to_string());
                let forced = pair
                    .try_run(&forced_row_path(&mode_sql(query, mode)))
                    .map_err(|e| e.to_string());
                // Assert
                assert!(typed.is_err(), "{query} on {mode}");
                assert!(typed == forced, "{query} on {mode}");
            }
        }
    });
}

#[test]
fn should_match_the_row_path_across_a_batch_boundary() {
    // Arrange
    with_pair(|pair| {
        let rows = (0..1500)
            .map(|row| {
                let value = if row == 1300 {
                    "1000000".to_owned()
                } else if row == 1301 {
                    "NULL".to_owned()
                } else {
                    row.to_string()
                };
                format!("({row},'g{}',{},{value},{row}.5,true)", row % 3, row % 5)
            })
            .collect::<Vec<_>>();
        pair.create("m", COLUMNS, &rows.join(","));
        let query = "SELECT g, k, COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v), AVG(f) FROM {m} GROUP BY g, k";
        for mode in [ROW, COL] {
            // Act
            let typed = pair.run(&mode_sql(query, mode));
            let reason = last_dispatch(pair);
            let forced = pair.run(&forced_row_path(&mode_sql(query, mode)));
            // Assert
            assert_eq!(typed, forced, "{mode}");
            assert_eq!(reason, DISPATCH, "{mode}");
            assert!(
                typed.iter().any(|row| row[6] == Value::Int64(1_000_000)),
                "{mode}"
            );
        }
    });
}

#[test]
fn should_keep_prepared_parameters_on_the_typed_grouped_path() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, ROWS);
        let query = "SELECT g, SUM(v) AS s FROM {m} WHERE v > $1 AND k <> $2 GROUP BY g";
        let params = vec![Value::Int64(5), Value::Int64(2)];
        for mode in [ROW, COL] {
            let sql = mode_sql(query, mode);
            // Act
            let typed = pair
                .cassie
                .execute_sql(&pair.session, &sql, params.clone())
                .expect("typed")
                .rows()
                .to_vec();
            let reason = last_dispatch(pair);
            let forced = pair
                .cassie
                .execute_sql(&pair.session, &forced_row_path(&sql), params.clone())
                .expect("row path")
                .rows()
                .to_vec();
            // Assert
            assert_eq!(typed, forced, "{mode}");
            assert_eq!(reason, DISPATCH, "{mode}");
        }
    });
}

#[test]
fn should_leave_unsupported_grouped_shapes_on_the_row_path() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, ROWS);
        for query in [
            "SELECT g, COUNT(DISTINCT v) FROM {m} GROUP BY g",
            "SELECT g, SUM(v) FILTER (WHERE k > 0) FROM {m} GROUP BY g",
            "SELECT g, SUM(v) FROM {m} GROUP BY g HAVING SUM(v) > 10",
            "SELECT g, SUM(v) FROM {m} GROUP BY ROLLUP (g)",
            "SELECT k + 1 AS k1, COUNT(*) FROM {m} GROUP BY k + 1",
            "SELECT g, bool_and(b) FROM {m} GROUP BY g",
        ] {
            // Act
            pair.run(&mode_sql(query, ROW));
            // Assert
            assert_ne!(last_dispatch(pair), DISPATCH, "{query}");
        }
    });
}

/// Rows, or the SQLSTATE and message of the failure. An always-true `HAVING COUNT(*) >= 0`
/// forces the row path without dropping the single row of an ungrouped aggregate.
fn outcome(pair: &Pair, sql: &str, params: &[Value]) -> Result<Vec<Vec<Value>>, String> {
    pair.cassie
        .execute_sql(&pair.session, sql, params.to_vec())
        .map(|result| result.rows().to_vec())
        .map_err(|error| format!("{}: {error}", error.sql_state()))
}

#[test]
fn should_match_the_row_path_for_parameters_of_every_value_type() {
    // Arrange
    with_pair(|pair| {
        pair.create("m", COLUMNS, ROWS);
        let queries = [
            "SELECT g, COUNT(*) AS c, SUM(v) AS s FROM {m} WHERE v > $1 GROUP BY g",
            "SELECT COUNT(*) AS c, SUM(v) AS s FROM {m} WHERE v > $1",
            "SELECT k, MAX(f) AS m FROM {m} WHERE f >= $1 GROUP BY k",
            "SELECT k, COUNT(*) AS c FROM {m} WHERE g = $1 GROUP BY k",
            "SELECT g, COUNT(*) AS c FROM {m} WHERE b = $1 GROUP BY g",
        ];
        let params = [
            Value::Int64(10),
            Value::Float64(9.5),
            Value::Float64(-0.0),
            Value::String("30".to_owned()),
            Value::String("a".to_owned()),
            Value::Bool(true),
            Value::Null,
        ];
        let mut served = 0;
        for query in queries {
            for param in &params {
                for mode in [ROW, COL] {
                    let sql = mode_sql(query, mode);
                    let bound = std::slice::from_ref(param);

                    // Act
                    let typed = outcome(pair, &sql, bound);
                    served += usize::from(last_dispatch(pair) == DISPATCH);
                    let forced = outcome(pair, &format!("{sql} HAVING COUNT(*) >= 0"), bound);

                    // Assert
                    assert_eq!(typed, forced, "{mode}: {query} with {param:?}");
                }
            }
        }
        assert!(
            served >= 10,
            "the typed grouped kernel served {served} statements"
        );
    });
}
