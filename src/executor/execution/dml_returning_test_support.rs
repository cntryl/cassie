//! Plain TEXT fixtures and real private-helper invocation for RETURNING probes.
use super::super::{
    build_dml_result, BatchRow, Cassie, CassieSession, ColumnMeta, DmlResultContext, QueryError,
    QueryExecutionControls, QueryResult, SelectItem, Value,
};
use super::observer::{record, Hook, Observation, Phase, Records};
use crate::app::CassieError;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use crate::runtime::{ExecutionMode, QueryCancellationHandle, QueryMemoryReservation};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

pub(super) const COPIES: usize = 17;
pub(super) const PAYLOAD_BYTES: usize = 2048;

pub(super) struct Fixture {
    pub session: CassieSession,
    pub cassie: Cassie,
    _directory: Directory,
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("RETURNING cleanup failed while unwinding: {error}");
            } else {
                panic!("strict RETURNING cleanup: {error}");
            }
        }
    }
}
impl Fixture {
    pub fn new() -> Self {
        let directory = Directory(std::env::temp_dir().join(format!(
            "cassie-returning-boundary-{}",
            uuid::Uuid::new_v4()
        )));
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.plan_cache_entries = 0;
        let cassie = Cassie::new_with_data_dir_and_config(&directory.0, config).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("returning-setup", None);
        for sql in [
            "CREATE TABLE rt_memory(id BIGINT, payload TEXT)",
            "CREATE TABLE ri_memory(id BIGINT)",
            "INSERT INTO ri_memory VALUES(1)",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("plain fixture schema");
        }
        cassie
            .execute_sql(
                &session,
                "INSERT INTO rt_memory VALUES(1,$1),(2,$2)",
                vec![Value::String("x".repeat(PAYLOAD_BYTES)); 2],
            )
            .expect("two exact plain TEXT rows");
        Self {
            session,
            cassie,
            _directory: directory,
        }
    }

    pub fn assert_readback(&self, changed: bool) {
        let observer = self.cassie.create_session("returning-observer", None);
        let actual = self
            .cassie
            .execute_sql(
                &observer,
                "SELECT id,payload FROM rt_memory ORDER BY id",
                vec![],
            )
            .expect("fresh independent readback");
        let first = if changed { 11 } else { 1 };
        assert_eq!(actual.command(), "SELECT");
        assert_eq!(
            actual.columns(),
            vec![
                column("id", "bigint", 20, 8),
                column("payload", "text", 25, -1)
            ]
        );
        assert_eq!(
            actual.rows(),
            (first..first + 2)
                .map(|id| vec![Value::Int64(id), Value::String("x".repeat(PAYLOAD_BYTES))])
                .collect::<Vec<_>>()
        );
    }

    pub fn assert_retired(&self, controls: &QueryExecutionControls) {
        assert_eq!(
            controls.current_query_memory_bytes(),
            0,
            "all private command owners retired"
        );
        assert_eq!(
            self.cassie
                .runtime
                .snapshot()
                .runtime
                .active_operator_workers,
            0
        );
    }
}

pub(super) fn limits(budget: usize, timeout_ms: u64) -> CassieRuntimeLimits {
    CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: timeout_ms,
        ..CassieRuntimeLimits::default()
    }
}

pub(super) fn projection_text(changed: bool) -> String {
    let mut items = vec!["id".to_string()];
    items.extend((0..COPIES).map(|index| format!("payload AS p{index}")));
    let key = if changed {
        "rt_memory.id-10"
    } else {
        "rt_memory.id"
    };
    items.push(format!(
        "EXISTS(SELECT 1 FROM ri_memory i WHERE i.id={key}) AS member"
    ));
    items.join(", ")
}

pub(super) fn projection() -> Vec<SelectItem> {
    parse_projection(&projection_text(false))
}

pub(super) fn ancestor_projection() -> Vec<SelectItem> {
    parse_projection(&projection_text(false).replace(
        "i.id=rt_memory.id",
        "i.id=rt_memory.id AND i.id=ancestor.id",
    ))
}

fn parse_projection(projection: &str) -> Vec<SelectItem> {
    let parsed = crate::sql::parse_statement(&format!("SELECT {projection} FROM rt_memory"))
        .expect("projection AST");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT AST");
    };
    select.projection
}

fn plain_row(id: i64) -> BatchRow {
    BatchRow::new(vec![
        ("id".into(), Value::Int64(id)),
        ("payload".into(), Value::String("x".repeat(PAYLOAD_BYTES))),
    ])
}

fn admit(
    row: BatchRow,
    controls: &QueryExecutionControls,
) -> (BatchRow, Arc<QueryMemoryReservation>) {
    let reservation = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("input backing"))
            .expect("input admission before measured helper peak"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&reservation)))
        .retain_operator_memory(controls, Arc::clone(&reservation))
        .expect("input operator root");
    (row, reservation)
}

fn inputs(
    controls: &QueryExecutionControls,
    leased: bool,
) -> (Vec<BatchRow>, Vec<Arc<QueryMemoryReservation>>) {
    if !leased {
        return ((1..=2).map(plain_row).collect(), vec![]);
    }
    let mut anchors = Vec::new();
    let rows = (1..=2)
        .map(|id| {
            let ancestor = BatchRow::new(vec![
                ("ancestor.id".into(), Value::Int64(id)),
                (
                    "ancestor.payload".into(),
                    Value::String("x".repeat(PAYLOAD_BYTES)),
                ),
            ]);
            let (ancestor, parent) = admit(ancestor, controls);
            let (row, current) = admit(plain_row(id), controls);
            let row = row
                .retain_operator_memory(controls, Arc::clone(&parent))
                .expect("current operator chain retains its real ancestor reservation");
            anchors.extend([parent, current]);
            row.with_outer_scope(Arc::new(ancestor))
        })
        .collect();
    (rows, anchors)
}

pub(super) fn call_helper(
    fixture: &Fixture,
    projection: &[SelectItem],
    controls: &QueryExecutionControls,
    read_controls: &QueryExecutionControls,
    rows: Vec<BatchRow>,
) -> Result<QueryResult, QueryError> {
    build_dml_result(
        &DmlResultContext {
            cassie: &fixture.cassie,
            session: Some(&fixture.session),
            table: "rt_memory",
            returning: projection,
            params: &[],
            user_functions: &HashMap::new(),
            command_prefix: "UPDATE",
            controls,
            statement_read_controls: read_controls,
            returning_schema: None,
        },
        rows.len(),
        rows,
    )
}

pub(super) fn captured(
    fixture: &Fixture,
    controls: &QueryExecutionControls,
) -> QueryExecutionControls {
    super::super::super::entrypoints::statement_read_controls(
        &fixture.cassie,
        Some(&fixture.session),
        controls,
    )
    .expect("supplied pre-command statement view")
}

pub(super) fn records() -> Records {
    Rc::new(RefCell::new(Vec::new()))
}

pub(super) struct HelperRun {
    pub result: Result<QueryResult, QueryError>,
    pub records: Records,
    pub peak: usize,
    pub setup_peak: usize,
    pub retained: usize,
    pub caller_charge: usize,
}

pub(super) fn helper_run(
    fixture: &Fixture,
    projection: &[SelectItem],
    budget: usize,
    leased: bool,
    retain_caller: bool,
) -> HelperRun {
    let controls = QueryExecutionControls::from_limits(&limits(budget, 30_000), Instant::now());
    let read_controls = captured(fixture, &controls);
    let view_charge = controls.current_query_memory_bytes();
    let (rows, mut anchors) = inputs(&controls, leased);
    if !retain_caller {
        anchors.clear();
    }
    let caller_charge = anchors.iter().map(|anchor| anchor.bytes()).sum::<usize>();
    let setup_peak = controls.peak_query_memory_bytes();
    let records = records();
    let observed = Rc::clone(&records);
    let hook = Hook::install(move |frame| record(&observed, &frame));
    let result = call_helper(fixture, projection, &controls, &read_controls, rows);
    drop(hook);
    let peak = controls.peak_query_memory_bytes();
    let result_bytes = result.as_ref().map_or(0, QueryResult::retained_bytes);
    let retained = controls
        .current_query_memory_bytes()
        .checked_sub(result_bytes)
        .expect("managed result is charged to the same request tracker");
    assert_eq!(
        retained,
        view_charge + caller_charge,
        "private helper state retires while the public result retains only its own lease"
    );
    drop(anchors);
    drop(read_controls);
    assert_eq!(
        controls.current_query_memory_bytes(),
        result_bytes,
        "only the independently owned public result survives private scope retirement"
    );
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        0
    );
    assert!(records
        .borrow()
        .iter()
        .flat_map(|record| &record.roots)
        .all(super::observer::Roots::retired));
    println!("helper leased={leased} caller={retain_caller} budget={budget} setup={setup_peak} peak={peak} retained={retained} phases={}", records.borrow().len());
    HelperRun {
        result,
        records,
        peak,
        setup_peak,
        retained,
        caller_charge,
    }
}

pub(super) fn column(name: &str, data_type: &str, oid: i64, typlen: i16) -> ColumnMeta {
    ColumnMeta {
        name: name.into(),
        data_type: data_type.into(),
        type_oid: oid,
        typlen,
        atttypmod: -1,
        format_code: 0,
        nullable: true,
    }
}

pub(super) fn assert_result(actual: &QueryResult, changed: bool) {
    let mut columns = vec![column("id", "bigint", 20, 8)];
    columns.extend((0..COPIES).map(|index| column(&format!("p{index}"), "text", 25, -1)));
    columns.push(column("member", "boolean", 16, 1));
    assert_eq!(actual.columns(), columns, "literal full descriptors");
    assert_eq!(actual.command(), "UPDATE 2");
    let first = if changed { 11 } else { 1 };
    let expected = (first..first + 2)
        .map(|id| {
            let mut row = vec![Value::Int64(id)];
            row.extend((0..COPIES).map(|_| Value::String("x".repeat(PAYLOAD_BYTES))));
            row.push(Value::Bool(id == first));
            row
        })
        .collect::<Vec<_>>();
    let mut rows = actual.rows().to_vec();
    rows.sort_by_key(|row| match row[0] {
        Value::Int64(id) => id,
        ref other => panic!("id: {other:?}"),
    });
    assert_eq!(
        rows, expected,
        "public values survive all private guard retirement"
    );
}

pub(super) fn assert_resource_limit(result: &Result<QueryResult, QueryError>) {
    let error = result.as_ref().expect_err("measured peak-minus-one denial");
    let QueryError::Cassie(public @ CassieError::ResourceLimit(_)) = error else {
        panic!("structured memory denial: {error:?}");
    };
    assert_eq!(public.descriptor().sql_state, "54000");
}

pub(super) struct DmlRun {
    pub result: Result<QueryResult, CassieError>,
    pub records: Records,
    pub peak: usize,
}

pub(super) fn dml_run(budget: usize) -> DmlRun {
    let fixture = Fixture::new();
    let sql = format!(
        "UPDATE rt_memory SET id=id+10 RETURNING {}",
        projection_text(true)
    );
    let controls = QueryExecutionControls::from_limits(&limits(budget, 30_000), Instant::now());
    let records = records();
    let observed = Rc::clone(&records);
    let hook = Hook::install(move |frame| record(&observed, &frame));
    let result = fixture.cassie.execute_sql_with_controls(
        &fixture.session,
        &sql,
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    drop(hook);
    let peak = controls.peak_query_memory_bytes();
    let result_bytes = result.as_ref().map_or(0, QueryResult::retained_bytes);
    assert_eq!(
        controls.current_query_memory_bytes(),
        result_bytes,
        "only the managed public result survives the command scope"
    );
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        0
    );
    fixture.assert_readback(result.is_ok());
    assert!(records
        .borrow()
        .iter()
        .flat_map(|record| &record.roots)
        .all(super::observer::Roots::retired));
    println!(
        "actual DML budget={budget} peak={peak} result={:?} phases={}",
        result.as_ref().map(|result| (
            result.command(),
            result.rows().len(),
            result.columns().len()
        )),
        records.borrow().len()
    );
    DmlRun {
        result,
        records,
        peak,
    }
}

pub(super) fn interrupt_helper(
    fixture: &Fixture,
    projection: &[SelectItem],
    expiry: bool,
) -> (Result<QueryResult, QueryError>, Records) {
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &limits(
            CassieRuntimeLimits::default().query_memory_budget_bytes,
            if expiry { 500 } else { 30_000 },
        ),
        Instant::now(),
        cancellation.clone(),
    );
    let read_controls = captured(fixture, &controls);
    let rows = inputs(&controls, false).0;
    let records = records();
    let observed = Rc::clone(&records);
    let hook = Hook::install(move |frame: Observation<'_>| {
        record(&observed, &frame);
        if frame.phase == Phase::Materializing(0) {
            assert!(
                !frame.controls.is_cancelled() && !frame.controls.is_timed_out(),
                "interrupt must begin at live final boundary"
            );
            if expiry {
                let deadline = frame.controls.deadline.expect("configured deadline");
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(remaining <= std::time::Duration::from_secs(1));
                std::thread::sleep(remaining);
                while !frame.controls.is_timed_out() {
                    std::thread::yield_now();
                }
            } else {
                cancellation.cancel();
            }
        }
    });
    let result = call_helper(fixture, projection, &controls, &read_controls, rows);
    drop(hook);
    drop(read_controls);
    fixture.assert_retired(&controls);
    println!(
        "actual final-boundary expiry={expiry} result={:?} phases={}",
        result.as_ref().map(|result| (
            result.command(),
            result.rows().len(),
            result.columns().len()
        )),
        records.borrow().len()
    );
    (result, records)
}
