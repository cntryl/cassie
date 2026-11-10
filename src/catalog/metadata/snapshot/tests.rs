use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::catalog::{DatabaseMeta, FieldMeta, RoleMeta};
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;

use super::{CaptureBoundary, Catalog, QueryExecutionControls};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn catalog() -> Catalog {
    let catalog = Catalog::new();
    catalog.databases.write().insert(
        "example".to_owned(),
        DatabaseMeta {
            name: "example".to_owned(),
            description: Some("retained description".to_owned()),
            physical_family: "opaque-family".to_owned(),
        },
    );
    catalog
}

#[test]
fn should_retain_the_catalog_charge_until_its_last_snapshot_owner_is_dropped() {
    // Arrange
    let source = catalog();
    let controls = controls(usize::MAX);

    // Act
    let snapshot = source.snapshot_with_controls(&controls).expect("snapshot");
    let retained = controls.current_query_memory_bytes();
    let sibling = Arc::clone(&snapshot);
    drop(snapshot);
    source.databases.write().clear();

    // Assert
    assert!(retained > 0);
    assert_eq!(controls.current_query_memory_bytes(), retained);
    assert_eq!(sibling.catalog().databases.read().len(), 1);
    assert_eq!(source.databases.read().len(), 0);
    drop(sibling);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_catalog_clone_before_copying_when_one_byte_below_its_admission() {
    // Arrange
    let source = catalog();
    let measurement = controls(usize::MAX);
    let owner = source
        .snapshot_with_controls(&measurement)
        .expect("measure");
    let required = measurement.current_query_memory_bytes();
    drop(owner);
    let insufficient = controls(required - 1);
    let copied = Cell::new(0);

    // Act
    let rejected = source.snapshot_with_observer(&insufficient, |boundary| {
        if boundary == CaptureBoundary::MapCloned {
            copied.set(copied.get() + 1);
        }
    });
    let exact = controls(required);
    let admitted = source.snapshot_with_controls(&exact).expect("exact budget");

    // Assert
    assert!(matches!(
        rejected,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(copied.get(), 0);
    assert_eq!(insufficient.current_query_memory_bytes(), 0);
    assert_eq!(exact.current_query_memory_bytes(), required);
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_complete_sparse_snapshot_backing() {
    // Arrange
    let source = catalog();
    source.databases.write().reserve(4096);
    let mut fields = Vec::with_capacity(512);
    fields.push(FieldMeta {
        name: "nested".to_owned(),
        data_type: crate::types::DataType::Array(Box::new(crate::types::DataType::Array(
            Box::new(crate::types::DataType::Text),
        ))),
        is_indexed: false,
        boost: None,
    });
    source.schemas.write().insert(
        "example.rows".to_owned(),
        crate::catalog::CollectionSchema {
            collection: "example.rows".to_owned(),
            fields,
        },
    );
    let serialized = serde_json::to_vec(&*source.schemas.read()).expect("fixture JSON");
    let budget = controls(serialized.len() + 1024);
    let copying_started = Cell::new(false);

    // Act
    let result = source.snapshot_with_observer(&budget, |boundary| {
        if boundary == CaptureBoundary::BeforeClone {
            copying_started.set(true);
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(!copying_started.get());
    assert_eq!(budget.current_query_memory_bytes(), 0);
}

#[test]
fn should_retire_partial_catalog_snapshot_ownership_when_capture_is_canceled() {
    // Arrange
    let source = catalog();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: usize::MAX,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let copied = Cell::new(0);

    // Act
    let result = source.snapshot_with_observer(&controls, |boundary| {
        if boundary == CaptureBoundary::MapCloned {
            copied.set(copied.get() + 1);
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(copied.get(), 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(source.databases.read().len(), 1);
}

#[test]
fn should_check_cancellation_while_waiting_for_catalog_read_guards() {
    // Arrange
    let source = catalog();
    let locked = source.roles.write();
    let cancellation = QueryCancellationHandle::new();
    let earlier_guards_released = Cell::new(false);
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = source.snapshot_with_observer(&controls, |boundary| {
        if boundary == CaptureBoundary::WaitingForLocks {
            earlier_guards_released.set(source.databases.try_write().is_some());
            cancellation.cancel();
        }
    });
    drop(locked);

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert!(earlier_guards_released.get());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_nested_vector_backing_without_sparse_root_capacity() {
    // Arrange
    use crate::embeddings::{
        DistanceMetric, HnswGraphNode, HnswGraphState, IvfFlatTrainingState, VectorIndexMetadata,
        VectorIndexRecord, VectorIndexType,
    };
    let source = catalog();
    let mut neighbors = Vec::with_capacity(256);
    let mut neighbor = String::with_capacity(2048);
    neighbor.push_str("neighbor");
    neighbors.push(neighbor);
    let mut vector = Vec::with_capacity(1024);
    vector.push(1.0);
    let training = IvfFlatTrainingState {
        version: 1,
        source_fingerprint: 0,
        trained: true,
        row_count: 1,
        lists: 1,
        probes: 1,
        training_seed: 1,
        centroid_ids: vec!["centroid".to_owned()],
        centroids: vec![vec![1.0]],
        assignments: std::collections::BTreeMap::from([("row".to_owned(), 0)]),
        list_sizes: vec![1],
    };
    let record = VectorIndexRecord {
        collection: "example.rows".to_owned(),
        field: "embedding".to_owned(),
        source_field: "text".to_owned(),
        metadata: VectorIndexMetadata {
            provider: "disabled".to_owned(),
            model: "fixture".to_owned(),
            dimensions: 1,
            metric: DistanceMetric::Cosine,
            index_type: VectorIndexType::Hnsw,
            hnsw: None,
            hnsw_graph: Some(HnswGraphState {
                version: 1,
                source_fingerprint: 0,
                row_count: 1,
                dimensions: 1,
                metric: DistanceMetric::Cosine,
                entry_point: Some("row".to_owned()),
                max_layer: 0,
                nodes: vec![HnswGraphNode {
                    id: "row".to_owned(),
                    vector,
                    magnitude: 1.0,
                    layers: vec![neighbors],
                }],
            }),
            ivfflat: None,
            ivfflat_training: Some(training),
        },
    };
    let serialized = serde_json::to_vec(&record).expect("fixture JSON");
    source
        .vector_indexes
        .write()
        .insert("fixture".to_owned(), record);
    let budget = controls(serialized.len() + 2048);
    let copying_started = Cell::new(false);

    // Act
    let result = source.snapshot_with_observer(&budget, |boundary| {
        if boundary == CaptureBoundary::BeforeClone {
            copying_started.set(true);
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(!copying_started.get());
    assert_eq!(budget.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_an_expired_capture_without_retaining_memory() {
    // Arrange
    let source = catalog();
    let mut controls = controls(usize::MAX);
    controls.deadline = Instant::now().checked_sub(Duration::from_secs(1));

    // Act
    let result = source.snapshot_with_controls(&controls);

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::DeadlineExceeded)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_sanitize_catalog_snapshot_debug_output() {
    // Arrange
    let source = catalog();
    let secret = "secret-hash-sentinel";
    let mut role = RoleMeta::login("reader", secret.to_owned());
    role.database_grants = Some(BTreeSet::from(["example".to_owned()]));
    source.roles.write().insert("reader".to_owned(), role);
    let controls = controls(usize::MAX);
    let snapshot = source.snapshot_with_controls(&controls).expect("snapshot");

    // Act
    let debug = format!("{snapshot:?}");

    // Assert
    assert!(!debug.contains(secret));
    assert!(!debug.contains("reader"));
    assert!(!debug.contains("opaque-family"));
}
