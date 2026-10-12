use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::catalog::canonical_relation_name;
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use crate::executor::batch::flatten_batches;
use crate::runtime::QueryExecutionControls;
use crate::sql::{bind, parse_statement, QuerySource, QueryStatement};
use crate::types::Value;

use super::super::super::{finish_retained_join, EquiJoinKeys};
use super::super::{
    execute_dense_streaming_bounded_inner_join, execute_left_build_streaming_bounded_inner_join,
    JoinRetentionContext, JoinRetentionPhase, SourceExecutionEnv, StreamingJoinSpec,
};

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
    config: CassieRuntimeConfig,
    integer: String,
    float: String,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-mixed-stream-budget-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_timeout_ms = 0;
        config.limits.query_memory_budget_bytes = 8 * 1024 * 1024;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
            .expect("mixed stream fixture");
        cassie.startup().expect("mixed stream catalog");
        let session = cassie.create_session("root", None);
        for sql in [
            "CREATE TABLE numeric_stream_integer (k BIGINT)",
            "INSERT INTO numeric_stream_integer(k) VALUES (9007199254740993),(9007199254740992)",
            "CREATE TABLE numeric_stream_float (k FLOAT)",
            "INSERT INTO numeric_stream_float(k) VALUES (9007199254740992.0),(9007199254740992.0)",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("mixed stream SQL seed");
        }
        Self {
            cassie: Some(cassie),
            path,
            config,
            integer: canonical_relation_name("postgres", "public", "numeric_stream_integer"),
            float: canonical_relation_name("postgres", "public", "numeric_stream_float"),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cassie.take();
        std::fs::remove_dir_all(&self.path).expect("retire mixed stream fixture");
    }
}

struct Observation {
    rows: Vec<(Value, Value)>,
    source_calls: usize,
    build_calls: usize,
    output_calls: usize,
    completed_executions: u64,
    live_output: usize,
    retired: usize,
}

fn observe(fixture: &Fixture, dense: bool, reverse: bool) -> Observation {
    let cassie = fixture.cassie.as_ref().expect("live fixture");
    let controls = QueryExecutionControls::from_limits(&fixture.config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let (left, right) = if reverse {
        (&fixture.float, &fixture.integer)
    } else {
        (&fixture.integer, &fixture.float)
    };
    let parsed = parse_statement(&format!(
        "SELECT * FROM {left} INNER JOIN {right} ON {left}.k = {right}.k LIMIT 64"
    ))
    .expect("stream SQL");
    let bound = bind(parsed, &cassie.catalog).expect("actual bound mixed equality");
    let QueryStatement::Select(select) = bound.statement.statement else {
        panic!("SELECT fixture");
    };
    let QuerySource::Join { on, .. } = select.source else {
        panic!("JOIN fixture");
    };
    let (left_key, right_key, domain) = crate::sql::numeric_comparison::equality_columns(&on)
        .expect("actual generated equality domain");
    let spec = StreamingJoinSpec {
        left_collection: left,
        right_collection: right,
        keys: EquiJoinKeys {
            left: left_key.to_owned(),
            right: right_key.to_owned(),
            domain,
        },
        on: &on,
        left_scan_fields: vec!["k".into()],
        right_scan_fields: vec!["k".into()],
        output_budget: 64,
    };
    let source_calls = Cell::new(0);
    let build_calls = Cell::new(0);
    let output_calls = Cell::new(0);
    let probe = |phase| {
        match phase {
            JoinRetentionPhase::BoundedSource => {
                source_calls.set(source_calls.get() + 1);
            }
            JoinRetentionPhase::BoundedBuild => build_calls.set(build_calls.get() + 1),
            JoinRetentionPhase::BoundedOutput => output_calls.set(output_calls.get() + 1),
            _ => {}
        }
        Ok(())
    };
    let retention = JoinRetentionContext::with_probe(&probe);
    let before = cassie.runtime.snapshot().joins.executions;
    let joined = if dense {
        execute_dense_streaming_bounded_inner_join(&env, &spec, &retention)
    } else {
        execute_left_build_streaming_bounded_inner_join(&env, &spec, &retention)
    };
    let result = joined
        .and_then(|joined| finish_retained_join(&env, joined))
        .map_err(CassieError::from);
    let mut values = Vec::new();
    let live_output;
    match result {
        Ok((batches, _text)) => {
            let rows = flatten_batches(batches);
            live_output = controls.current_query_memory_bytes();
            for row in &rows {
                let integer = row
                    .get(&format!("{}.k", fixture.integer))
                    .expect("original integer")
                    .clone();
                let float = row
                    .get(&format!("{}.k", fixture.float))
                    .expect("original float")
                    .clone();
                values.push((integer, float));
            }
            drop(rows);
        }
        Err(error) => panic!("unexpected stream failure: {error}"),
    }
    Observation {
        rows: values,
        source_calls: source_calls.get(),
        build_calls: build_calls.get(),
        output_calls: output_calls.get(),
        completed_executions: cassie.runtime.snapshot().joins.executions - before,
        live_output,
        retired: controls.current_query_memory_bytes(),
    }
}

#[test]
fn should_preserve_mixed_streaming_key_ownership() {
    // Arrange
    let fixture = Fixture::new();

    // Act
    let observations = [false, true]
        .into_iter()
        .flat_map(|dense| [false, true].map(|reverse| (dense, observe(&fixture, dense, reverse))))
        .collect::<Vec<_>>();
    drop(fixture);

    // Assert
    for (dense, ample) in observations {
        let mut integers = ample
            .rows
            .iter()
            .map(|(integer, float)| {
                assert_eq!(*float, Value::Float64(9_007_199_254_740_992.0));
                let Value::Int64(integer) = integer else {
                    panic!("original BIGINT carrier");
                };
                *integer
            })
            .collect::<Vec<_>>();
        integers.sort_unstable();
        assert_eq!(
            integers,
            [
                9_007_199_254_740_992,
                9_007_199_254_740_992,
                9_007_199_254_740_993,
                9_007_199_254_740_993
            ]
        );
        assert_eq!(
            ample.completed_executions, 1,
            "actual retained result publishes one completed join"
        );
        assert_eq!(ample.output_calls, 4);
        assert!(ample.source_calls > 0 && ample.live_output > 0);
        assert_eq!(ample.build_calls, usize::from(!dense));
        assert_eq!(ample.retired, 0);
    }
}
