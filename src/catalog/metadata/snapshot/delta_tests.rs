use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use crate::catalog::{
    CollectionSchema, DatabaseMeta, FieldMeta, NamespaceMeta, RoleGrantTarget, RoleObjectGrantMeta,
    RoleObjectPrivilege, SequenceMeta,
};
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::collections::BTreeSet;

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

fn namespace(name: &str, description: &str) -> NamespaceMeta {
    NamespaceMeta {
        name: name.to_owned(),
        description: Some(description.to_owned()),
        object_id: 17,
    }
}

fn role_grant(target: RoleGrantTarget) -> RoleObjectGrantMeta {
    RoleObjectGrantMeta {
        database_family: "db-family".to_owned(),
        role_id: 23,
        target,
        privileges: BTreeSet::from([RoleObjectPrivilege::Select]),
    }
}

fn example() -> (Catalog, Catalog, Catalog) {
    let initial = Catalog::new();
    for name in ["kept", "removed", "changed"] {
        initial
            .namespaces
            .write()
            .insert(name.into(), namespace(name, "initial"));
    }
    initial.databases.write().insert(
        "example".into(),
        DatabaseMeta {
            name: "example".into(),
            description: Some("initial database".into()),
            physical_family: "opaque".into(),
        },
    );
    let base = initial.detached_transaction_snapshot();
    base.namespaces
        .write()
        .insert("outside".into(), namespace("outside", "outside"));
    let overlay = base.detached_transaction_snapshot();
    overlay.namespaces.write().remove("removed");
    overlay
        .namespaces
        .write()
        .insert("changed".into(), namespace("changed", "own"));
    overlay
        .namespaces
        .write()
        .insert("created".into(), namespace("created", "own"));
    overlay
        .databases
        .write()
        .get_mut("example")
        .expect("database")
        .description = Some("not published by transactional DDL".into());
    (initial, base, overlay)
}

#[test]
fn should_compose_only_own_catalog_delta_over_the_initial_view() {
    // Arrange
    let (initial, base, overlay) = example();
    let controls = controls(usize::MAX);

    // Act
    let owner = initial
        .snapshot_with_delta(&base, &overlay, &controls)
        .expect("merged snapshot");
    let namespaces = owner.catalog().namespaces.read();

    // Assert
    assert_eq!(namespaces.len(), 3);
    assert_eq!(namespaces["kept"].description.as_deref(), Some("initial"));
    assert_eq!(namespaces["changed"].description.as_deref(), Some("own"));
    assert!(namespaces.contains_key("created"));
    assert!(!namespaces.contains_key("removed"));
    assert!(!namespaces.contains_key("outside"));
    assert_eq!(
        owner.catalog().databases.read()["example"]
            .description
            .as_deref(),
        Some("initial database")
    );
}

#[test]
fn should_apply_own_catalog_delta_over_current_descriptor_metadata() {
    // Arrange
    let (_, base, overlay) = example();
    let controls = controls(usize::MAX);

    // Act
    let owner = base
        .snapshot_with_delta(&base, &overlay, &controls)
        .expect("live descriptors plus own DDL");
    overlay.namespaces.write().clear();

    // Assert
    let namespaces = owner.catalog().namespaces.read();
    assert_eq!(namespaces.len(), 4);
    assert!(namespaces.contains_key("outside"));
    assert!(namespaces.contains_key("created"));
    assert!(!namespaces.contains_key("removed"));
    assert_eq!(namespaces["changed"].description.as_deref(), Some("own"));
}

#[test]
fn should_publish_role_grant_delta_from_a_detached_transaction_snapshot() {
    // Arrange
    let initial = Catalog::new();
    let removed = role_grant(RoleGrantTarget::Table(41));
    let added = role_grant(RoleGrantTarget::Table(42));
    initial.register_role_object_grant(removed.clone());
    let base = initial.detached_transaction_snapshot();
    let overlay = base.detached_transaction_snapshot();
    overlay.unregister_role_object_grant(&removed);
    overlay.register_role_object_grant(added.clone());
    let controls = controls(usize::MAX);

    // Act
    let owner = initial
        .snapshot_with_delta(&base, &overlay, &controls)
        .expect("role grant delta snapshot");

    // Assert
    assert_eq!(owner.catalog().list_role_object_grants(), vec![added]);
    assert_eq!(
        owner.accounted_bytes(),
        controls.current_query_memory_bytes()
    );
    assert!(!initial
        .matches_transaction_metadata(&overlay, &controls)
        .unwrap());
}

#[test]
fn should_admit_the_complete_delta_owner_before_cloning_any_map() {
    // Arrange
    let (initial, base, overlay) = example();
    let measure = controls(usize::MAX);
    let owner = initial
        .snapshot_with_delta(&base, &overlay, &measure)
        .expect("measure");
    let required = measure.current_query_memory_bytes();
    assert_eq!(owner.accounted_bytes(), required);
    drop(owner);
    let below = controls(required - 1);
    let copied = Cell::new(0);

    // Act
    let rejected = initial.snapshot_with_delta_observer(&base, &overlay, &below, |boundary| {
        if boundary == CaptureBoundary::MapCloned {
            copied.set(copied.get() + 1);
        }
    });
    let exact = controls(required);
    let admitted = initial
        .snapshot_with_delta(&base, &overlay, &exact)
        .expect("exact admission");

    // Assert
    assert!(matches!(
        rejected,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(copied.get(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(exact.current_query_memory_bytes(), required);
    let map = Arc::downgrade(&admitted.catalog().namespaces);
    let sibling = Arc::clone(&admitted);
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), required);
    assert!(map.upgrade().is_some());
    drop(sibling);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert!(map.upgrade().is_none());
}

#[test]
fn should_admit_sparse_nested_delta_backing_before_any_output_copy() {
    // Arrange
    let initial = Catalog::new();
    let base = initial.detached_transaction_snapshot();
    let overlay = base.detached_transaction_snapshot();
    let mut fields = Vec::with_capacity(128);
    let mut name = String::with_capacity(512);
    name.push_str("nested");
    fields.push(FieldMeta {
        name,
        data_type: crate::types::DataType::Array(Box::new(crate::types::DataType::Array(
            Box::new(crate::types::DataType::Text),
        ))),
        is_indexed: false,
        boost: Some(-0.0),
    });
    overlay.schemas.write().insert(
        "rows".into(),
        CollectionSchema {
            collection: "rows".into(),
            fields,
        },
    );
    let measure = controls(usize::MAX);
    let owner = initial
        .snapshot_with_delta(&base, &overlay, &measure)
        .expect("measure sparse delta");
    let required = owner.accounted_bytes();
    drop(owner);
    let below = controls(required - 1);
    let copied = Cell::new(0);

    // Act
    let rejected = initial.snapshot_with_delta_observer(&base, &overlay, &below, |boundary| {
        if boundary == CaptureBoundary::MapCloned {
            copied.set(copied.get() + 1);
        }
    });
    let exact = controls(required);
    let admitted = initial
        .snapshot_with_delta(&base, &overlay, &exact)
        .expect("complete sparse admission");
    overlay.schemas.write().clear();

    // Assert
    assert!(matches!(
        rejected,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(copied.get(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(
        admitted.catalog().schemas.read()["rows"].fields[0].name,
        "nested"
    );
    assert_eq!(exact.current_query_memory_bytes(), required);
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_a_partially_cloned_delta_when_canceled() {
    // Arrange
    let (initial, base, overlay) = example();
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
    let result = initial.snapshot_with_delta_observer(&base, &overlay, &controls, |boundary| {
        if boundary == CaptureBoundary::MapCloned {
            copied.set(copied.get() + 1);
            // The third map is the nonempty namespace delta; earlier map
            // clones must also unwind before the one output lease retires.
            if copied.get() == 3 {
                cancellation.cancel();
            }
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(copied.get(), 3);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_compare_transaction_metadata_without_capacity_or_version_identity() {
    // Arrange
    let (initial, _, _) = example();
    let other = initial.detached_transaction_snapshot();
    other.namespaces.write().reserve(64);
    other.databases.write().clear();
    other.bump_version();
    let controls = controls(0);

    // Act
    let same = initial
        .matches_transaction_metadata(&other, &controls)
        .expect("allocation-free comparison");
    other.namespaces.write().remove("kept");
    let changed = initial
        .matches_transaction_metadata(&other, &controls)
        .expect("removed metadata");

    // Assert
    assert!(same);
    assert!(!changed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_treat_sequence_cursor_as_non_structural_metadata() {
    // Arrange
    let current = Catalog::new();
    let mut sequence = SequenceMeta::new("public.sequence_cursor", crate::types::DataType::BigInt);
    sequence.object_id = 83;
    current.register_sequence(sequence.clone());
    let base = current.detached_transaction_snapshot();
    sequence.current_value = 2;
    sequence.is_called = true;
    current.update_sequence_cursor(&sequence);
    let mut staged_sequence = sequence.clone();
    staged_sequence.current_value = 3;
    let staged = base.detached_transaction_snapshot();
    staged.register_sequence(staged_sequence);
    let controls = controls(usize::MAX);

    // Act
    let cursor_only_change = current
        .matches_transaction_metadata(&base, &controls)
        .expect("cursor is not structural catalog metadata");
    let owner = current
        .snapshot_with_delta(&base, &staged, &controls)
        .expect("owner view retains its sequence cursor");
    let owner_cursor = owner
        .catalog()
        .get_sequence("public.sequence_cursor")
        .expect("sequence is visible in owner view");
    let mut changed_definition = sequence;
    changed_definition.increment_by = 2;
    current.register_sequence(changed_definition);
    let structural_change = current
        .matches_transaction_metadata(&base, &controls)
        .expect("definition change is structural catalog metadata");

    // Assert
    assert!(cursor_only_change);
    assert_eq!(owner_cursor.current_value, 3);
    assert!(owner_cursor.is_called);
    assert!(!structural_change);
}

#[test]
fn should_compare_nested_float_metadata_by_its_exact_bits() {
    // Arrange
    let initial = Catalog::new();
    let nan = f32::from_bits(0x7fc0_0011);
    initial.schemas.write().insert(
        "rows".into(),
        CollectionSchema {
            collection: "rows".into(),
            fields: vec![FieldMeta {
                name: "value".into(),
                data_type: crate::types::DataType::Int,
                is_indexed: false,
                boost: Some(nan),
            }],
        },
    );
    let other = initial.detached_transaction_snapshot();
    let controls = controls(0);

    // Act
    let same_nan = initial
        .matches_transaction_metadata(&other, &controls)
        .expect("same NaN payload");
    other
        .schemas
        .write()
        .get_mut("rows")
        .expect("schema")
        .fields[0]
        .boost = Some(f32::from_bits(0x7fc0_0012));
    let other_nan = initial
        .matches_transaction_metadata(&other, &controls)
        .expect("different NaN payload");

    // Assert
    assert!(same_nan);
    assert!(!other_nan);
}

#[test]
fn should_preserve_exact_float_identity_without_allocating_comparison_state() {
    use super::exact_eq::ExactMetadataEq;

    // Arrange
    let nan = f64::from_bits(0x7ff8_0000_0000_0011);
    let same = nan;
    let different = f64::from_bits(0x7ff8_0000_0000_0012);
    let controls = controls(0);

    // Act
    let same_nan = nan.exact_eq(&same, &controls).expect("same NaN");
    let changed_nan = nan.exact_eq(&different, &controls).expect("other NaN");
    let changed_zero = 0.0_f64.exact_eq(&-0.0, &controls).expect("signed zero");

    // Assert
    assert!(same_nan);
    assert!(!changed_nan);
    assert!(!changed_zero);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_metadata_comparison_at_the_actual_map_lock_wait() {
    // Arrange
    let initial = Catalog::new();
    let other = initial.detached_transaction_snapshot();
    let held = other.schemas.write();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = initial.matches_transaction_metadata_observer(&other, &controls, |boundary| {
        if boundary == CaptureBoundary::WaitingForLocks {
            cancellation.cancel();
        }
    });
    drop(held);

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
