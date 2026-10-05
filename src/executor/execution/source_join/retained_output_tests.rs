use std::collections::HashMap;
use std::time::Instant;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryExecutionControls;

use super::{
    execute_lateral_join, execute_loaded_join, finish_retained_join, reserve_join_rows, BatchRow,
    BinaryOp, Expr, JoinExecutionSpec, JoinKind, JoinRowsSpec, QuerySource, SourceExecutionEnv,
    Value,
};

#[test]
fn should_release_nested_candidates_rejected_by_the_predicate() {
    // Arrange
    with_fixture(false, |env| {
        let left = [row("left", 1, &"l".repeat(4096))];
        let right = [row("right", 1, &"r".repeat(4096))];
        let templates = templates();
        let on = Expr::BoolLiteral(false);
        let input = reserve_join_rows(env, &left, &right).expect("complete inputs fit");
        let input_bytes = env.controls.current_query_memory_bytes();
        let before = env.cassie.runtime.snapshot();

        // Act
        let joined = execute_loaded_join(
            env,
            rows_spec(JoinKind::Inner, &on, &left, &right, &templates),
            &["left.key".to_owned()],
            &["right.key".to_owned()],
        )
        .expect("rejected candidates complete");

        // Assert
        assert!(joined.rows.is_empty());
        assert!(env.controls.peak_query_memory_bytes() > input_bytes);
        assert_eq!(env.controls.current_query_memory_bytes(), input_bytes);
        assert_eq!(
            env.cassie.runtime.snapshot().joins.executions,
            before.joins.executions
        );
        let (batches, _) = finish_retained_join(env, joined).expect("empty completion");
        assert!(batches.is_empty());
        drop(input);
    });
}

#[test]
fn should_keep_nested_null_extended_rows_charged_until_drop() {
    // Arrange
    let on = Expr::BoolLiteral(false);

    // Act
    let rows = outer_rows(&on);

    // Assert
    assert_null_extended_rows(&rows);
}

#[test]
fn should_keep_merge_null_extended_rows_charged_until_drop() {
    // Arrange
    let on = equality();

    // Act
    let rows = outer_rows(&on);

    // Assert
    assert_null_extended_rows(&rows);
}

#[test]
fn should_keep_hash_fanout_rows_charged_until_drop() {
    // Arrange
    let vectorized = true;

    // Act
    let rows = fanout_rows(vectorized);

    // Assert
    assert_eq!(
        rows,
        vec![
            (Value::Int64(1), Value::String("r1".to_owned())),
            (Value::Int64(1), Value::String("r2".to_owned()))
        ]
    );
}

#[test]
fn should_keep_merge_fanout_rows_charged_until_drop() {
    // Arrange
    let vectorized = false;

    // Act
    let rows = fanout_rows(vectorized);

    // Assert
    assert_eq!(
        rows,
        vec![
            (Value::Int64(1), Value::String("r1".to_owned())),
            (Value::Int64(1), Value::String("r2".to_owned()))
        ]
    );
}

#[test]
fn should_keep_lateral_output_charged_after_its_right_source_drops() {
    // Arrange
    with_fixture(false, |env| {
        let crate::sql::QueryStatement::Select(select) =
            crate::sql::parse_statement("SELECT 1 AS key")
                .expect("constant SELECT")
                .statement
        else {
            panic!("SELECT fixture")
        };
        let left_source = QuerySource::SingleRow;
        let right_source = QuerySource::Subquery {
            alias: "right".to_owned(),
            select: Box::new(select),
            lateral: true,
        };
        let on = Expr::BoolLiteral(true);
        let left_batches = vec![vec![row("left", 1, "l1"), row("left", 2, "l2")]];
        let mut context = HashMap::new();

        // Act
        let (batches, _) = execute_lateral_join(
            env,
            &JoinExecutionSpec {
                left: &left_source,
                right: &right_source,
                kind: JoinKind::Cross,
                on: &on,
                outer_row: None,
                row_budget: None,
            },
            &mut context,
            left_batches,
        )
        .expect("actual lateral source execution");

        // Assert
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 2);
        assert!(env.controls.current_query_memory_bytes() > 0);
        assert_eq!(
            batches[0][0].get("left.payload"),
            Some(&Value::String("l1".to_owned()))
        );
        assert_eq!(
            batches[0][1].get("left.payload"),
            Some(&Value::String("l2".to_owned()))
        );
        drop(batches);
    });
}

fn outer_rows(on: &Expr) -> Vec<(Value, Value)> {
    let mut output = Vec::new();
    with_fixture(false, |env| {
        let left = [row("left", 1, "l")];
        let right = [row("right", 2, "r")];
        let templates = templates();
        let input = reserve_join_rows(env, &left, &right).expect("complete inputs fit");
        let input_bytes = env.controls.current_query_memory_bytes();
        let before = env.cassie.runtime.snapshot();
        let joined = execute_loaded_join(
            env,
            rows_spec(JoinKind::Full, on, &left, &right, &templates),
            &["left.key".to_owned()],
            &["right.key".to_owned()],
        )
        .expect("outer kernel");
        assert_eq!(
            env.cassie.runtime.snapshot().joins.executions,
            before.joins.executions
        );
        let (batches, _) = finish_retained_join(env, joined).expect("outer completion");
        assert!(env.controls.current_query_memory_bytes() > input_bytes);
        output = batches
            .iter()
            .flatten()
            .map(|row| {
                (
                    row.get("left.key").unwrap().clone(),
                    row.get("right.key").unwrap().clone(),
                )
            })
            .collect();
        drop(batches);
        assert_eq!(env.controls.current_query_memory_bytes(), input_bytes);
        assert_eq!(
            env.cassie.runtime.snapshot().joins.executions,
            before.joins.executions + 1
        );
        drop(input);
    });
    output
}

fn fanout_rows(vectorized: bool) -> Vec<(Value, Value)> {
    let mut output = Vec::new();
    with_fixture(vectorized, |env| {
        let left = [row("left", 1, "l")];
        let right = [row("right", 1, "r1"), row("right", 1, "r2")];
        let templates = templates();
        let on = equality();
        let input = reserve_join_rows(env, &left, &right).expect("complete inputs fit");
        let input_bytes = env.controls.current_query_memory_bytes();
        let joined = execute_loaded_join(
            env,
            rows_spec(JoinKind::Inner, &on, &left, &right, &templates),
            &["left.key".to_owned()],
            &["right.key".to_owned()],
        )
        .expect("fanout kernel");
        let (batches, _) = finish_retained_join(env, joined).expect("fanout completion");
        assert!(env.controls.current_query_memory_bytes() > input_bytes);
        output = batches
            .iter()
            .flatten()
            .map(|row| {
                (
                    row.get("left.key").unwrap().clone(),
                    row.get("right.payload").unwrap().clone(),
                )
            })
            .collect();
        drop(batches);
        assert_eq!(env.controls.current_query_memory_bytes(), input_bytes);
        drop(input);
    });
    output
}

fn rows_spec<'a>(
    kind: JoinKind,
    on: &'a Expr,
    left: &'a [BatchRow],
    right: &'a [BatchRow],
    templates: &'a (BatchRow, BatchRow),
) -> JoinRowsSpec<'a> {
    JoinRowsSpec {
        kind,
        on,
        left_rows: left,
        right_rows: right,
        left_template: &templates.0,
        right_template: &templates.1,
        row_budget: None,
    }
}

fn row(side: &str, key: i64, payload: &str) -> BatchRow {
    BatchRow::new(vec![
        (format!("{side}.key"), Value::Int64(key)),
        (format!("{side}.payload"), Value::String(payload.to_owned())),
    ])
}

fn templates() -> (BatchRow, BatchRow) {
    let row = |side| {
        BatchRow::new(vec![
            (format!("{side}.key"), Value::Null),
            (format!("{side}.payload"), Value::Null),
        ])
    };
    (row("left"), row("right"))
}

fn equality() -> Expr {
    Expr::Binary {
        left: Box::new(Expr::Column("left.key".to_owned())),
        op: BinaryOp::Eq,
        right: Box::new(Expr::Column("right.key".to_owned())),
    }
}

fn assert_null_extended_rows(rows: &[(Value, Value)]) {
    assert_eq!(
        rows,
        &[
            (Value::Int64(1), Value::Null),
            (Value::Null, Value::Int64(2))
        ]
    );
}

fn with_fixture(vectorized: bool, run: impl FnOnce(&SourceExecutionEnv<'_>)) {
    let path = std::env::temp_dir().join(format!("cassie-join-output-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024;
    config.limits.vectorized_joins_enabled = vectorized;
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("controlled Cassie");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    run(&SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    });
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
