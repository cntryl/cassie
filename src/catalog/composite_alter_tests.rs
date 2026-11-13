use super::*;
use crate::catalog::COMPOSITE_TYPE_FORMAT_VERSION;

fn meta(name: &str, object_id: u64, fields: &[(&str, DataType)]) -> CompositeTypeMeta {
    CompositeTypeMeta {
        format_version: COMPOSITE_TYPE_FORMAT_VERSION,
        database: "cassie".to_string(),
        name: format!("cassie.public.{name}"),
        object_id,
        array_object_id: object_id + 1,
        relation_object_id: object_id + 2,
        owner_oid: 10,
        revision: 1,
        fields: fields
            .iter()
            .map(|(field, data_type)| CompositeField {
                name: (*field).to_string(),
                data_type: data_type.clone(),
            })
            .collect(),
        dropped_positions: Vec::new(),
    }
}

fn address() -> CompositeTypeMeta {
    meta(
        "address",
        20_000,
        &[("street", DataType::Text), ("zip", DataType::Int)],
    )
}

#[test]
fn should_append_a_physical_slot_when_adding_an_attribute() {
    // Arrange
    let before = address();

    // Act
    let after = before
        .with_added_attribute("city", DataType::Text)
        .expect("add attribute");

    // Assert
    assert_eq!(after.descriptor().physical_len(), 3);
    assert_eq!(after.revision, before.revision + 1);
}

#[test]
fn should_keep_the_dropped_slot_position_when_dropping_an_attribute() {
    // Arrange
    let before = address();

    // Act
    let after = before.with_dropped_attribute("street").expect("drop");

    // Assert
    assert_eq!(after.dropped_positions, vec![0]);
    assert_eq!(after.descriptor().physical_positions(), vec![1]);
}

#[test]
fn should_place_an_attribute_added_after_a_drop_in_a_fresh_slot() {
    // Arrange
    let dropped = address().with_dropped_attribute("street").expect("drop");

    // Act
    let after = dropped
        .with_added_attribute("street", DataType::Text)
        .expect("re-add");

    // Assert
    assert_eq!(after.descriptor().physical_positions(), vec![1, 2]);
}

#[test]
fn should_reject_adding_a_duplicate_attribute_with_sqlstate_42701() {
    // Arrange
    let before = address();

    // Act
    let error = before
        .with_added_attribute("zip", DataType::Int)
        .expect_err("duplicate");

    // Assert
    assert_eq!(error.sqlstate, "42701");
}

#[test]
fn should_reject_dropping_a_missing_attribute_with_sqlstate_42703() {
    // Arrange
    let before = address();

    // Act
    let error = before.with_dropped_attribute("nope").expect_err("missing");

    // Assert
    assert_eq!(error.sqlstate, "42703");
}

#[test]
fn should_rename_an_attribute_without_moving_its_slot() {
    // Arrange
    let before = address();

    // Act
    let after = before
        .with_renamed_attribute("zip", "postcode")
        .expect("rename");

    // Assert
    assert_eq!(after.fields[1].name, "postcode");
    assert_eq!(after.descriptor().physical_positions(), vec![0, 1]);
}

#[test]
fn should_reject_a_type_that_contains_itself_with_sqlstate_42p16() {
    // Arrange
    let before = address();
    let own = DataType::Composite(before.descriptor());

    // Act
    let error = before
        .with_retyped_attribute("zip", own)
        .expect_err("cycle");

    // Assert
    assert_eq!(error.sqlstate, "42P16");
}

#[test]
fn should_rebuild_embedded_descriptors_of_dependent_types() {
    // Arrange
    let catalog = Catalog::new();
    let inner = address();
    let outer = meta(
        "person",
        20_010,
        &[("home", DataType::Composite(inner.descriptor()))],
    );
    catalog.register_composite_type(inner.clone());
    catalog.register_composite_type(outer);
    let widened = inner
        .with_added_attribute("city", DataType::Text)
        .expect("add attribute");

    // Act
    let propagation = catalog.propagate_composite_change(&widened.descriptor());

    // Assert
    let [rebuilt] = propagation.types.as_slice() else {
        panic!("expected one dependent type");
    };
    let DataType::Composite(embedded) = &rebuilt.fields[0].data_type else {
        panic!("embedded descriptor");
    };
    assert_eq!(embedded.physical_len(), 3);
    assert_eq!(rebuilt.revision, 2);
}

#[test]
fn should_report_no_stored_columns_for_an_unused_type() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_composite_type(address());

    // Act
    let columns = catalog.composite_type_stored_in(address().type_oid());

    // Assert
    assert_eq!(columns.len(), 0);
}
