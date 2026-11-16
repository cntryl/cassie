//! Validator, order and JSON export laws of the reserved graph types (#1268).
use std::cmp::Ordering;

use uuid::Uuid;

use super::json::path_to_json;
use super::model::{Direction, ElementKind, ElementRef, GraphDescriptors, KeyPart, Path};
use super::order::{compare_element_refs, compare_paths};
use super::validate::validate_record;
use crate::catalog::graph_schema::build_graph_types;
use crate::types::composite::RecordValue;
use crate::types::typed::TypedValue;
use crate::types::{SqlArray, Value};

fn descriptors() -> GraphDescriptors {
    let mut next = 100;
    let types = build_graph_types("main", || {
        next += 1;
        Ok(next)
    })
    .expect("build");
    GraphDescriptors {
        key_part: types[0].descriptor(),
        element_ref: types[1].descriptor(),
        path: types[2].descriptor(),
    }
}

fn id(last: u8) -> Uuid {
    Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, last])
}

fn vertex(key: Vec<KeyPart>) -> ElementRef {
    ElementRef {
        database_id: id(1),
        graph_id: id(2),
        mapping_id: id(3),
        kind: ElementKind::Vertex,
        key,
    }
}

fn edge(key: i64) -> ElementRef {
    ElementRef {
        mapping_id: id(4),
        kind: ElementKind::Edge,
        key: vec![KeyPart::BigInt(key)],
        ..vertex(Vec::new())
    }
}

fn record_of(value: &Value) -> &RecordValue {
    match value {
        Value::Typed(TypedValue::Record(record)) => record,
        _ => panic!("expected a record"),
    }
}

fn path_of(vertices: Vec<ElementRef>, edges: Vec<ElementRef>, directions: Vec<Direction>) -> Path {
    Path {
        vertices,
        edges,
        directions,
    }
}

fn key_part_with(fields: Vec<Value>) -> Result<(), String> {
    let descriptors = descriptors();
    let record = RecordValue::new(descriptors.key_part, fields).expect("shape");
    validate_record(&record).map_err(|error| error.message().to_owned())
}

#[test]
fn should_accept_every_well_formed_key_part() {
    // Arrange
    let descriptors = descriptors();
    let parts = [
        KeyPart::SmallInt(-5),
        KeyPart::Int(7),
        KeyPart::BigInt(9_007_199_254_740_993),
        KeyPart::Text("a,b \"c\"".into()),
        KeyPart::Uuid(id(9)),
    ];

    // Act
    let outcomes = parts
        .iter()
        .map(|part| validate_record(record_of(&part.to_value(&descriptors).expect("value"))))
        .collect::<Vec<_>>();

    // Assert
    assert!(outcomes.iter().all(Result::is_ok));
}

#[test]
fn should_reject_a_key_part_with_an_unknown_tag() {
    // Arrange
    let fields = vec![Value::Int64(9), Value::Int64(1), Value::Null, Value::Null];

    // Act
    let outcome = key_part_with(fields);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_key_part_with_a_null_tag() {
    // Arrange
    let fields = vec![Value::Null, Value::Int64(1), Value::Null, Value::Null];

    // Act
    let outcome = key_part_with(fields);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_key_part_with_an_extra_populated_slot() {
    // Arrange
    let fields = vec![
        Value::Int64(3),
        Value::Int64(1),
        Value::String("x".into()),
        Value::Null,
    ];

    // Act
    let outcome = key_part_with(fields);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_key_part_with_a_missing_slot() {
    // Arrange
    let fields = vec![Value::Int64(4), Value::Null, Value::Null, Value::Null];

    // Act
    let outcome = key_part_with(fields);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_smallint_key_part_outside_its_range() {
    // Arrange
    let fields = vec![Value::Int64(1), Value::Int64(40_000), Value::Null, Value::Null];

    // Act
    let outcome = key_part_with(fields);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_an_element_reference_with_an_empty_key() {
    // Arrange
    let descriptors = descriptors();
    let value = vertex(Vec::new()).to_value(&descriptors).expect("value");

    // Act
    let outcome = validate_record(record_of(&value));

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_path_with_too_few_vertices() {
    // Arrange
    let descriptors = descriptors();
    let first = vertex(vec![KeyPart::BigInt(1)]);
    let path = path_of(vec![first], vec![edge(1)], vec![Direction::Forward]);
    let value = path.to_value(&descriptors).expect("value");

    // Act
    let outcome = validate_record(record_of(&value));

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_path_with_a_vertex_in_the_edge_array() {
    // Arrange
    let descriptors = descriptors();
    let first = vertex(vec![KeyPart::BigInt(1)]);
    let path = path_of(
        vec![first.clone(), first.clone()],
        vec![first],
        vec![Direction::Forward],
    );
    let value = path.to_value(&descriptors).expect("value");

    // Act
    let outcome = validate_record(record_of(&value));

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_path_with_a_direction_outside_one_and_two() {
    // Arrange
    let descriptors = descriptors();
    let first = vertex(vec![KeyPart::BigInt(1)]);
    let good = path_of(
        vec![first.clone(), first],
        vec![edge(1)],
        vec![Direction::Forward],
    )
    .to_value(&descriptors)
    .expect("value");
    let mut fields = record_of(&good).fields().to_vec();
    fields[2] = Value::Array(SqlArray::from_elements(vec![Value::Int64(3)]).expect("array"));
    let record = RecordValue::new(descriptors.path, fields).expect("shape");

    // Act
    let outcome = validate_record(&record);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_reject_a_null_element_inside_an_array_field() {
    // Arrange
    let descriptors = descriptors();
    let first = vertex(vec![KeyPart::BigInt(1)])
        .to_value(&descriptors)
        .expect("value");
    let good = path_of(vec![vertex(vec![KeyPart::BigInt(1)])], Vec::new(), Vec::new())
        .to_value(&descriptors)
        .expect("value");
    let mut fields = record_of(&good).fields().to_vec();
    fields[0] = Value::Array(SqlArray::from_elements(vec![first, Value::Null]).expect("array"));
    let record = RecordValue::new(descriptors.path, fields).expect("shape");

    // Act
    let outcome = validate_record(&record);

    // Assert
    assert!(outcome.is_err());
}

#[test]
fn should_accept_a_zero_hop_path() {
    // Arrange
    let descriptors = descriptors();
    let path = path_of(vec![vertex(vec![KeyPart::BigInt(1)])], Vec::new(), Vec::new());
    let value = path.to_value(&descriptors).expect("value");

    // Act
    let outcome = validate_record(record_of(&value));

    // Assert
    assert!(outcome.is_ok());
}

#[test]
fn should_order_key_parts_by_type_before_value() {
    // Arrange
    let small = vertex(vec![KeyPart::SmallInt(900)]);
    let big = vertex(vec![KeyPart::BigInt(-1)]);

    // Act
    let order = compare_element_refs(&small, &big);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_order_text_key_parts_by_bytes() {
    // Arrange
    let upper = vertex(vec![KeyPart::Text("Z".into())]);
    let lower = vertex(vec![KeyPart::Text("a".into())]);

    // Act
    let order = compare_element_refs(&upper, &lower);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_order_uuid_key_parts_as_unsigned_bytes() {
    // Arrange
    let low = vertex(vec![KeyPart::Uuid(Uuid::from_bytes([0x7f; 16]))]);
    let high = vertex(vec![KeyPart::Uuid(Uuid::from_bytes([0x80; 16]))]);

    // Act
    let order = compare_element_refs(&high, &low);

    // Assert
    assert_eq!(order, Ordering::Greater);
}

#[test]
fn should_order_a_shorter_key_tuple_before_its_extension() {
    // Arrange
    let short = vertex(vec![KeyPart::BigInt(1)]);
    let long = vertex(vec![KeyPart::BigInt(1), KeyPart::BigInt(0)]);

    // Act
    let order = compare_element_refs(&short, &long);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_order_a_zero_hop_path_before_its_extension() {
    // Arrange
    let start = vertex(vec![KeyPart::BigInt(1)]);
    let zero = path_of(vec![start.clone()], Vec::new(), Vec::new());
    let one = path_of(
        vec![start, vertex(vec![KeyPart::BigInt(2)])],
        vec![edge(1)],
        vec![Direction::Forward],
    );

    // Act
    let order = compare_paths(&zero, &one);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_order_a_forward_step_before_a_reverse_step_over_the_same_edge() {
    // Arrange
    let start = vertex(vec![KeyPart::BigInt(1)]);
    let end = vertex(vec![KeyPart::BigInt(2)]);
    let forward = path_of(
        vec![start.clone(), end.clone()],
        vec![edge(1)],
        vec![Direction::Forward],
    );
    let reverse = path_of(vec![start, end], vec![edge(1)], vec![Direction::Reverse]);

    // Act
    let order = compare_paths(&forward, &reverse);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_compare_the_interleaved_sequence_not_the_vertex_array_first() {
    // Arrange
    let start = vertex(vec![KeyPart::BigInt(1)]);
    let low_edge_high_end = path_of(
        vec![start.clone(), vertex(vec![KeyPart::BigInt(9)])],
        vec![edge(1)],
        vec![Direction::Forward],
    );
    let high_edge_low_end = path_of(
        vec![start, vertex(vec![KeyPart::BigInt(2)])],
        vec![edge(5)],
        vec![Direction::Forward],
    );

    // Act
    let order = compare_paths(&low_edge_high_end, &high_edge_low_end);

    // Assert
    assert_eq!(order, Ordering::Less);
}

#[test]
fn should_export_the_version_one_json_document() {
    // Arrange
    let start = vertex(vec![KeyPart::BigInt(9_007_199_254_740_993)]);
    let path = path_of(
        vec![start.clone(), vertex(vec![KeyPart::Text("b".into())])],
        vec![edge(7)],
        vec![Direction::Reverse],
    );

    // Act
    let json = path_to_json(&path);

    // Assert
    assert_eq!(json["version"], 1);
    assert_eq!(json["directions"], serde_json::json!([2]));
    assert_eq!(json["vertices"][0]["kind"], "vertex");
    assert_eq!(json["edges"][0]["kind"], "edge");
    assert_eq!(
        json["vertices"][0]["key_parts"][0],
        serde_json::json!({"type": "bigint", "value": "9007199254740993"})
    );
    assert_eq!(
        json["vertices"][1]["key_parts"][0],
        serde_json::json!({"type": "text", "value": "b"})
    );
}
