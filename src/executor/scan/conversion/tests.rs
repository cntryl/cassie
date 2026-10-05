use std::path::PathBuf;
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::catalog::FieldMeta;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::executor::batch::BatchRow;
use crate::runtime::QueryCancellationHandle;
use crate::types::{DataType, Value};

struct Fixture {
    cassie: Cassie,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-scan-conversion-controls-{}",
            uuid::Uuid::new_v4()
        ));
        let config = CassieRuntimeConfig {
            limits: CassieRuntimeLimits {
                parallel_scan_workers: 2,
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            },
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("engine");
        Self { cassie, path }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation,
    )
}

fn accounted_input(controls: &QueryExecutionControls, payload: serde_json::Value) -> InputBatch {
    let document = DocumentRef {
        id: "row-0".to_owned(),
        payload,
    };
    let bytes = AccountedDocument::estimated_retained_bytes(&document).expect("source size");
    let accounted = AccountedDocument::try_build(controls, bytes, || Ok(document)).expect("source");
    InputBatch::Accounted(vec![accounted])
}

fn request<'a>(controls: &'a QueryExecutionControls, shape: Shape<'a>) -> Request<'a> {
    Request {
        schema: None,
        controls,
        shape,
        parallel: true,
        after_row: None,
    }
}

#[derive(Default)]
struct Gate {
    released: Mutex<bool>,
    wake: Condvar,
}

impl Gate {
    fn wait(&self) {
        let released = self.released.lock().expect("gate");
        drop(
            self.wake
                .wait_while(released, |released| !*released)
                .expect("release"),
        );
    }

    fn release(&self) {
        *self.released.lock().expect("gate") = true;
        self.wake.notify_all();
    }
}

struct ReleaseOnDrop<'a>(&'a Gate);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
fn should_cancel_active_parallel_row_conversion_with_complete_cleanup() {
    // Arrange
    let fixture = Fixture::new();
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(1_048_576, cancellation.clone());
    let inputs = vec![
        accounted_input(&controls, serde_json::json!({"payload": "x".repeat(1_024)})),
        accounted_input(&controls, serde_json::json!({"payload": "y".repeat(1_024)})),
    ];
    let source_bytes = controls.current_query_memory_bytes();
    let gate = Gate::default();
    let (ready, activation) = mpsc::channel();
    let observer = || {
        ready.send(()).expect("accounted converted row");
        gate.wait();
    };
    let mut request = request(&controls, Shape::Full);
    request.after_row = Some(&observer);
    let before = fixture.cassie.metrics();

    // Act
    let result = std::thread::scope(|scope| {
        let release = ReleaseOnDrop(&gate);
        let coordinator = scope.spawn(|| convert(&fixture.cassie, inputs, &request));
        for _ in 0..2 {
            activation
                .recv_timeout(Duration::from_secs(5))
                .expect("two active conversion workers");
        }
        assert_eq!(
            fixture
                .cassie
                .runtime
                .snapshot()
                .runtime
                .active_operator_workers,
            2
        );
        assert!(controls.current_query_memory_bytes() > source_bytes);
        cancellation.cancel();
        gate.release();
        let result = coordinator.join().expect("coordinator");
        drop(release);
        result
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        0
    );
    assert_eq!(
        fixture.cassie.metrics()["parallel_scans"]["scans"],
        before["parallel_scans"]["scans"]
    );
    let replacement = fixture
        .cassie
        .runtime
        .try_acquire_operator_workers(2)
        .expect("reusable permits");
    assert_eq!(replacement.workers(), 2);
}

fn reject_before_conversion(shape: Shape<'_>) {
    let fixture = Fixture::new();
    let payload = serde_json::json!({"payload": "x".repeat(1_024)});
    let ample = controls(1_048_576, QueryCancellationHandle::new());
    let calibration = vec![accounted_input(&ample, payload.clone())];
    let source_bytes = ample.current_query_memory_bytes();
    let calibration_request = request(&ample, shape);
    let conversion_bytes = accounting::identifier_scratch(&calibration, &calibration_request, 1)
        .expect("identifier scratch")
        + accounting::conversion_bytes(&calibration, &calibration_request, 1).expect("row size");
    drop(calibration);
    assert_eq!(ample.current_query_memory_bytes(), 0);
    let controls = controls(
        source_bytes + conversion_bytes - 1,
        QueryCancellationHandle::new(),
    );
    let inputs = vec![accounted_input(&controls, payload)];
    let called = std::sync::atomic::AtomicBool::new(false);
    let observer = || called.store(true, std::sync::atomic::Ordering::Release);
    let mut request = request(&controls, shape);
    request.after_row = Some(&observer);
    let result = convert(&fixture.cassie, inputs, &request);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert!(!called.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reserve_full_row_state_before_conversion_while_source_is_live() {
    // Arrange
    let shape = Shape::Full;

    // Act
    reject_before_conversion(shape);

    // Assert
    // The helper verifies first-row nonactivation and complete release at the overlap bound.
}

#[test]
fn should_reserve_projected_row_state_before_conversion_while_source_is_live() {
    // Arrange
    let fields = ["payload".to_owned()];
    let shape = Shape::Projected {
        fields: &fields,
        filter: None,
    };

    // Act
    reject_before_conversion(shape);

    // Assert
    // The helper verifies first-row nonactivation and complete release at the overlap bound.
}

fn field(name: &str, data_type: DataType) -> FieldMeta {
    FieldMeta {
        name: name.to_owned(),
        data_type,
        is_indexed: false,
        boost: None,
    }
}

fn probe_serial_overlap(deny_permits: bool, projected: bool) {
    let fixture = Fixture::new();
    let controls = controls(1_048_576, QueryCancellationHandle::new());
    let blocker = deny_permits.then(|| {
        fixture
            .cassie
            .runtime
            .try_acquire_operator_workers(usize::MAX)
            .expect("occupy worker permits")
    });
    let inputs = vec![
        accounted_input(&controls, serde_json::json!({"payload": "x".repeat(1_024)})),
        accounted_input(&controls, serde_json::json!({"payload": "y".repeat(1_024)})),
    ];
    let source_bytes = controls.current_query_memory_bytes();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let observer = || {
        if calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 0 {
            assert!(controls.current_query_memory_bytes() > source_bytes);
        }
    };
    let fields = ["payload".to_owned()];
    let shape = if projected {
        Shape::Projected {
            fields: &fields,
            filter: None,
        }
    } else {
        Shape::Full
    };
    let request = Request {
        schema: None,
        controls: &controls,
        shape,
        parallel: deny_permits,
        after_row: Some(&observer),
    };
    let output = convert(&fixture.cassie, inputs, &request).expect("controlled serial conversion");
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 2);
    assert_eq!(output.batches.len(), 2);
    assert_eq!(controls.current_query_memory_bytes(), output.memory.bytes());
    assert_eq!(
        fixture.cassie.metrics()["parallel_scans"]["scans"].as_u64(),
        Some(0)
    );
    drop(output);
    drop(blocker);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        0
    );
}

#[test]
fn should_preserve_reservations_through_serial_row_conversion() {
    // Arrange
    let deny_permits = false;

    // Act
    for projected in [false, true] {
        probe_serial_overlap(deny_permits, projected);
    }

    // Assert
    // The helper asserts source/output overlap and release for both row shapes.
}

#[test]
fn should_preserve_reservations_when_parallel_conversion_permits_are_unavailable() {
    // Arrange
    let deny_permits = true;

    // Act
    for projected in [false, true] {
        probe_serial_overlap(deny_permits, projected);
    }

    // Assert
    // The helper asserts source/output overlap, fallback metrics and permit cleanup.
}

#[test]
fn should_account_missing_schema_fields_with_recursive_array_metadata() {
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(1_048_576, QueryCancellationHandle::new());
    let schema = CollectionSchema {
        collection: "events".to_owned(),
        fields: vec![
            field("missing", DataType::Text),
            field(
                "nested",
                DataType::Array(Box::new(DataType::Array(Box::new(DataType::Int)))),
            ),
        ],
    };
    let inputs = vec![accounted_input(&controls, serde_json::json!({"extra": 7}))];
    let request = Request {
        schema: Some(&schema),
        controls: &controls,
        shape: Shape::Full,
        parallel: false,
        after_row: None,
    };

    // Act
    let output = convert(&fixture.cassie, inputs, &request).expect("typed conversion");

    // Assert
    let row = &output.batches[0][0];
    assert_eq!(row.entries().len(), 4);
    assert_eq!(row.entries()[1].1, Value::Null);
    assert_eq!(row.entries()[2].1, Value::Null);
    assert_eq!(row.data_types().len(), 4);
    assert_eq!(row.data_types()[2], schema.fields[1].data_type);
    assert_eq!(row.data_types()[3], DataType::Null);
    let lower_bound = std::mem::size_of::<BatchRow>()
        + 4 * std::mem::size_of::<(String, Value)>()
        + 4 * std::mem::size_of::<DataType>()
        + 2 * std::mem::size_of::<DataType>();
    assert!(output.memory.bytes() >= lower_bound);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_invalid_typed_vectors_by_their_json_fallback() {
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(1_048_576, QueryCancellationHandle::new());
    let payload = serde_json::json!({"embedding": [1, {"nested": "x".repeat(8_192)}, 3]});
    let schema = CollectionSchema {
        collection: "events".to_owned(),
        fields: vec![field("embedding", DataType::Vector(3))],
    };
    let fields = ["embedding".to_owned()];
    let inputs = vec![accounted_input(&controls, payload)];
    let request = Request {
        schema: Some(&schema),
        controls: &controls,
        shape: Shape::Projected {
            fields: &fields,
            filter: None,
        },
        parallel: false,
        after_row: None,
    };

    // Act
    let output = convert(&fixture.cassie, inputs, &request).expect("JSON fallback conversion");

    // Assert
    let Value::Json(value) = &output.batches[0][0].entries()[1].1 else {
        panic!("invalid vector keeps JSON fallback")
    };
    let lower_bound =
        crate::runtime::accounted::json::retained_bytes(value).expect("JSON clone bound");
    assert!(output.memory.bytes() >= lower_bound);
    assert!(!output.batches[0][0].lookup_initialized());
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cover_extreme_finite_vector_formatting_scratch_before_conversion() {
    // Arrange
    let controls = controls(1_048_576, QueryCancellationHandle::new());
    let schema = CollectionSchema {
        collection: "events".to_owned(),
        fields: vec![field("embedding", DataType::Vector(3))],
    };
    let fields = ["embedding".to_owned()];
    let request = Request {
        schema: Some(&schema),
        controls: &controls,
        shape: Shape::Projected {
            fields: &fields,
            filter: None,
        },
        parallel: false,
        after_row: None,
    };
    let baseline = vec![InputBatch::Owned(vec![DocumentRef {
        id: "row-0".to_owned(),
        payload: serde_json::json!({"embedding": null}),
    }])];
    let row_floor = accounting::conversion_bytes(&baseline, &request, 1).expect("row floor");
    let mut uncovered = Vec::new();

    // Act
    for (label, finite) in [
        ("maximum", f64::MAX),
        ("negative maximum", -f64::MAX),
        ("smallest subnormal", f64::from_bits(1)),
        ("negative smallest subnormal", -f64::from_bits(1)),
    ] {
        assert!(finite.is_finite());
        let formatted = finite.to_string();
        let values = [1.0, finite, 2.0]
            .into_iter()
            .map(super::super::parse_f64_to_f32)
            .collect::<Option<Vec<_>>>()
            .expect("existing production formatter/parser accepts each finite f64");
        let inputs = vec![InputBatch::Owned(vec![DocumentRef {
            id: "row-0".to_owned(),
            payload: serde_json::json!({"embedding": [1.0, finite, 2.0]}),
        }])];
        let estimated = accounting::conversion_bytes(&inputs, &request, 1)
            .expect("preconstruction estimate")
            - row_floor;
        let required = formatted.capacity() + values.capacity() * std::mem::size_of::<f32>();
        if estimated < required {
            uncovered.push((
                label,
                estimated,
                required,
                formatted.capacity(),
                values.capacity(),
            ));
        }
    }

    // Assert
    // The middle element formats while a partial float Vec is already live. This tests
    // the producer's transient scratch bound, without claiming process RSS or query leakage.
    assert!(
        uncovered.is_empty(),
        "uncovered vector formatting scratch: {uncovered:?}"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
