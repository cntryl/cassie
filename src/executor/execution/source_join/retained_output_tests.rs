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

#[test]
fn should_preserve_exact_loaded_input_slots_through_backing_transfer() {
    // Arrange
    with_fixture(false, |env| {
        let batches = vec![vec![row("left", 1, "payload")]];
        let body_bytes = super::accounting::moved_row_bytes(&batches[0][0])
            .expect("complete retained input shape");
        let old_backing = batches.capacity() * std::mem::size_of::<super::Batch>()
            + batches[0].capacity() * std::mem::size_of::<BatchRow>();
        assert_eq!(env.controls.current_query_memory_bytes(), 0);

        // Act
        let (rows, memory) = super::prepare_join_rows(env, batches)
            .expect("complete input and temporary backing overlap fit");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.capacity(), 1);
        assert_eq!(
            rows[0].get("left.payload"),
            Some(&Value::String("payload".to_owned()))
        );
        assert_eq!(env.controls.current_query_memory_bytes(), body_bytes);
        assert_eq!(
            env.controls.peak_query_memory_bytes(),
            body_bytes + old_backing
        );
        drop(rows);
        assert_eq!(env.controls.current_query_memory_bytes(), body_bytes);
        drop(memory);
        assert_eq!(env.controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_admit_existing_alias_capacity_before_moving_join_inputs() {
    // Arrange
    with_fixture(false, |env| {
        let mut aliases = Vec::with_capacity(64);
        aliases.push(("left.value".to_owned(), 0));
        let alias_backing = aliases.capacity() * std::mem::size_of::<(String, usize)>();
        let row = BatchRow::with_aliases(vec![("value".to_owned(), Value::Int64(1))], aliases);
        assert!(row.query_memory().is_none());
        assert_eq!(row.aliases().len(), 1);
        let batches = vec![vec![row]];

        // Act
        let (rows, memory) = super::prepare_join_rows(env, batches)
            .expect("valid input with retained spare aliases fits");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.capacity(), 1);
        assert_eq!(rows[0].get("left.value"), Some(&Value::Int64(1)));
        assert!(
            env.controls.current_query_memory_bytes() >= alias_backing,
            "moved alias backing alone retains {alias_backing} bytes but only {} are admitted",
            env.controls.current_query_memory_bytes(),
        );
        drop(rows);
        drop(memory);
        assert_eq!(env.controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_release_retained_join_output_after_a_boolean_predicate_error() {
    // Arrange
    with_fixture(false, |env| {
        let left = [row("left", 1, &"l".repeat(1024))];
        let right_row = |accepted| {
            BatchRow::new(vec![
                ("right.key".to_owned(), Value::Int64(1)),
                ("right.payload".to_owned(), Value::String("r".repeat(1024))),
                ("right.accepted".to_owned(), accepted),
            ])
        };
        let right = [
            right_row(Value::Bool(true)),
            right_row(Value::String("no".to_owned())),
        ];
        let templates = templates();
        let on = Expr::Column("right.accepted".to_owned());
        let input = reserve_join_rows(env, &left, &right).expect("complete inputs fit");
        let input_bytes = env.controls.current_query_memory_bytes();
        let first_output = super::accounting::combined_row_bytes(&left[0], &right[0])
            .expect("first retained candidate shape");
        let second_output = super::accounting::combined_row_bytes(&left[0], &right[1])
            .expect("second retained candidate shape");
        let expected = [
            input_bytes + right.len() + first_output,
            input_bytes + right.len() + first_output + second_output,
        ];
        assert!(expected[1] < env.controls.query_memory_budget_bytes);
        let calls = std::cell::Cell::new(0);
        let observed = std::cell::Cell::new([0; 2]);
        let probe = |phase| {
            assert_eq!(phase, super::JoinRetentionPhase::NestedOutput);
            let index = calls.get();
            assert!(index < 2);
            let mut charges = observed.get();
            charges[index] = env.controls.current_query_memory_bytes();
            observed.set(charges);
            calls.set(index + 1);
            Ok(())
        };
        let retention = super::JoinRetentionContext::with_probe(&probe);
        let before = env.cassie.runtime.snapshot();

        // Act
        let result = super::execute_loaded_join_with_context(
            env,
            rows_spec(JoinKind::Inner, &on, &left, &right, &templates),
            &["left.key".to_owned()],
            &["right.key".to_owned(), "right.accepted".to_owned()],
            || {},
            &retention,
        );

        // Assert
        assert!(matches!(
            result,
            Err(super::QueryError::General(ref message))
                if message == "Boolean expression requires BOOLEAN or SQL NULL"
        ));
        assert_eq!(calls.get(), 2);
        assert_eq!(observed.get(), expected);
        assert_eq!(env.controls.current_query_memory_bytes(), input_bytes);
        assert_eq!(env.controls.peak_query_memory_bytes(), expected[1]);
        let after = env.cassie.runtime.snapshot();
        assert_eq!(after.joins.executions, before.joins.executions);
        assert_eq!(after.joins.scalar_joins, before.joins.scalar_joins);
        assert_eq!(
            after.joins.matched_rows_total,
            before.joins.matched_rows_total
        );
        assert_eq!(
            after.joins.output_rows_total,
            before.joins.output_rows_total
        );
        assert_eq!(after.joins.last_strategy, before.joins.last_strategy);
        assert_eq!(
            after.adaptive_candidates.operator_switch_successes,
            before.adaptive_candidates.operator_switch_successes
        );
        drop(input);
        assert_eq!(env.controls.current_query_memory_bytes(), 0);
    });
}
