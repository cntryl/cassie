use super::text::split_record_text;
use super::wire::{default_field_binary, receive_record_binary, send_record_binary};
use super::*;
use crate::types::extended::temporal::default_session;
use crate::types::{DataType, Value};

fn text_record(fields: Vec<Value>) -> RecordValue {
    let types = fields.iter().map(|_| DataType::Text).collect();
    let descriptor = CompositeRef::new(CompositeType::anonymous(types));
    RecordValue::new(descriptor, fields).expect("record")
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_quote_record_fields_where_record_out_does() {
    // Arrange
    let record = text_record(vec![text("a"), text("b c"), Value::Null, text("")]);

    // Act
    let rendered = format_record(&record, default_session());

    // Assert
    assert_eq!(rendered, "(a,\"b c\",,\"\")");
}

#[test]
fn should_double_embedded_quotes_when_rendering() {
    // Arrange
    let record = text_record(vec![text("say \"hi\"")]);

    // Act
    let rendered = format_record(&record, default_session());

    // Assert
    assert_eq!(rendered, "(\"say \"\"hi\"\"\")");
}

#[test]
fn should_double_backslashes_when_rendering() {
    // Arrange
    let record = text_record(vec![text("a\\b")]);

    // Act
    let rendered = format_record(&record, default_session());

    // Assert
    assert_eq!(rendered, "(\"a\\\\b\")");
}

#[test]
fn should_render_a_nested_record_quoted() {
    // Arrange
    let inner = text_record(vec![text("1"), text("2")]);
    let outer_type =
        CompositeType::anonymous(vec![DataType::Composite(inner.descriptor().clone())]);
    let outer = RecordValue::new(
        CompositeRef::new(outer_type),
        vec![Value::Typed(crate::types::typed::TypedValue::Record(inner))],
    )
    .expect("outer");

    // Act
    let rendered = format_record(&outer, default_session());

    // Assert
    assert_eq!(rendered, "(\"(1,2)\")");
}

#[test]
fn should_split_record_text_into_optional_fields() {
    // Arrange
    let inputs = [
        "(x,1)",
        "(,)",
        "(\"\",\"a \"\"q\"\" b\\\\c\")",
        "  ( a , b ) ",
    ];

    // Act
    let [plain, nulls, quoted, spaced] =
        inputs.map(|input| split_record_text(input, 2).expect("valid record text"));

    // Assert
    assert_eq!(plain, vec![Some("x".to_string()), Some("1".to_string())]);
    assert_eq!(nulls, vec![None, None]);
    assert_eq!(
        quoted,
        vec![Some(String::new()), Some("a \"q\" b\\c".to_string())]
    );
    assert_eq!(
        spaced,
        vec![Some(" a ".to_string()), Some(" b ".to_string())]
    );
}

#[test]
fn should_reject_malformed_record_text_with_postgresql_details() {
    // Arrange
    let cases = [
        ("1,2", "Missing left parenthesis."),
        ("(1,2,3)", "Too many columns."),
        ("(1)", "Too few columns."),
        ("(1,2) x", "Junk after right parenthesis."),
        ("(1,\"2", "Unexpected end of input."),
    ];

    for (input, detail) in cases {
        // Act
        let error = split_record_text(input, 2).expect_err("malformed literal");

        // Assert
        assert_eq!(error.sqlstate, "22P02");
        assert_eq!(error.detail.as_deref(), Some(detail));
    }
}

#[test]
fn should_order_a_null_field_after_every_value() {
    // Arrange
    let null_field = text_record(vec![text("a"), Value::Null]);
    let value_field = text_record(vec![text("a"), text("z")]);

    // Act
    let (null_key, value_key) = (record_key(&null_field), record_key(&value_field));

    // Assert
    assert!(value_key < null_key);
}

#[test]
fn should_equate_null_fields() {
    // Arrange
    let first = text_record(vec![text("a"), Value::Null]);
    let second = text_record(vec![text("a"), Value::Null]);

    // Act
    let (first_key, second_key) = (record_key(&first), record_key(&second));

    // Assert
    assert_eq!(first_key, second_key);
}

fn number_record(integer: i64, float: f64) -> RecordValue {
    let descriptor = CompositeRef::new(CompositeType::anonymous(vec![
        DataType::BigInt,
        DataType::Float,
    ]));
    RecordValue::new(
        descriptor,
        vec![Value::Int64(integer), Value::Float64(float)],
    )
    .expect("record")
}

#[test]
fn should_order_integer_fields_by_value() {
    // Arrange
    let records = [
        number_record(-5, 0.0),
        number_record(0, 0.0),
        number_record(3, 0.0),
    ];

    // Act
    let [negative, zero, positive] = records.map(|record| record_key(&record));

    // Assert
    assert!(negative < zero && zero < positive);
}

#[test]
fn should_equate_negative_zero_with_zero() {
    // Arrange
    let records = [number_record(0, 0.0), number_record(0, -0.0)];

    // Act
    let [zero, negative_zero] = records.map(|record| record_key(&record));

    // Assert
    assert_eq!(zero, negative_zero);
}

#[test]
fn should_order_nan_after_every_float() {
    // Arrange
    let records = [number_record(0, 1.0e300), number_record(0, f64::NAN)];

    // Act
    let [big, nan] = records.map(|record| record_key(&record));

    // Assert
    assert!(big < nan);
}

#[test]
fn should_round_trip_binary_record_framing() {
    // Arrange
    let descriptor = CompositeRef::new(CompositeType::anonymous(vec![
        DataType::Text,
        DataType::Int,
        DataType::Text,
    ]));
    let record = RecordValue::new(
        descriptor.clone(),
        vec![text("hi"), Value::Int64(7), Value::Null],
    )
    .expect("record");

    // Act
    let bytes = send_record_binary::<String>(&record, |value, data_type| {
        default_field_binary(value, data_type)
    })
    .expect("send");
    let received = receive_record_binary(&bytes, &descriptor, |data_type, field| match data_type {
        DataType::Int => Ok(Value::Int64(i64::from(i32::from_be_bytes(
            field.try_into().map_err(|_| "width".to_string())?,
        )))),
        _ => Ok(Value::String(String::from_utf8_lossy(field).into_owned())),
    })
    .expect("receive");

    // Assert
    assert_eq!(&bytes[..4], &3_i32.to_be_bytes());
    assert_eq!(received, record);
}

#[test]
fn should_reject_binary_records_with_the_wrong_shape() {
    // Arrange
    let descriptor = CompositeRef::new(CompositeType::anonymous(vec![DataType::Int]));
    let mut wrong_count = 2_i32.to_be_bytes().to_vec();
    wrong_count.extend_from_slice(&[0; 8]);
    let mut wrong_oid = 1_i32.to_be_bytes().to_vec();
    wrong_oid.extend_from_slice(&25_i32.to_be_bytes());
    wrong_oid.extend_from_slice(&(-1_i32).to_be_bytes());
    let truncated = 1_i32.to_be_bytes().to_vec();

    for bytes in [wrong_count, wrong_oid, truncated] {
        // Act
        let result = receive_record_binary(&bytes, &descriptor, |_, _| Ok(Value::Null));

        // Assert
        assert!(result.is_err());
    }
}

#[test]
fn should_round_trip_the_storage_json_form() {
    // Arrange
    let record = text_record(vec![text("a"), Value::Null]);
    let descriptor = record.descriptor().clone();

    // Act
    let json = record_to_storage_json(&record);
    let restored = record_from_storage_json(&json, &descriptor, &mut |value, _| {
        Ok(Value::String(
            value.as_str().unwrap_or_default().to_string(),
        ))
    });

    // Assert
    assert_eq!(restored, Ok(record));
}

#[test]
fn should_distinguish_a_null_record_from_an_all_null_record() {
    // Arrange
    let all_null = text_record(vec![Value::Null, Value::Null]);

    // Act
    let json = record_to_storage_json(&all_null);

    // Assert
    assert!(!json.is_null());
    assert!(all_null.all_fields_null());
    assert!(!all_null.no_field_null());
}

#[test]
fn should_reject_stored_records_that_do_not_match_the_descriptor() {
    // Arrange
    let record = text_record(vec![text("a"), text("b")]);
    let json = record_to_storage_json(&record);
    let mut other_type = (**record.descriptor()).clone();
    other_type.oid += 1;
    let other_oid = CompositeRef::new(other_type);
    let mut short_type = (**record.descriptor()).clone();
    short_type.fields.truncate(1);
    let fewer_fields = CompositeRef::new(short_type);
    let mut convert = |value: &serde_json::Value, _: &DataType| {
        Ok(Value::String(
            value.as_str().unwrap_or_default().to_string(),
        ))
    };

    // Act
    let wrong_oid = record_from_storage_json(&json, &other_oid, &mut convert);
    let wrong_count = record_from_storage_json(&json, &fewer_fields, &mut convert);

    // Assert
    assert!(wrong_oid.is_err());
    assert!(wrong_count.is_err());
}

fn text_converter() -> impl FnMut(&serde_json::Value, &DataType) -> Result<Value, StoredRecordError>
{
    |value, _| {
        Ok(Value::String(
            value.as_str().unwrap_or_default().to_string(),
        ))
    }
}

fn layout(fields: &[&str], dropped: Vec<usize>) -> CompositeRef {
    let mut descriptor = CompositeType::anonymous(vec![DataType::Text; fields.len()]);
    descriptor.oid = 300_001;
    descriptor.name = "layout".to_string();
    for (field, name) in descriptor.fields.iter_mut().zip(fields) {
        field.name = (*name).to_string();
    }
    descriptor.dropped_positions = dropped;
    CompositeRef::new(descriptor)
}

fn stored(oid: u32, slots: &[serde_json::Value]) -> serde_json::Value {
    serde_json::json!({ (RECORD_STORAGE_KEY): { "oid": oid, "revision": 1, "fields": slots } })
}

#[test]
fn should_read_added_attributes_as_null_for_values_written_under_an_older_revision() {
    // Arrange
    let current = layout(&["a", "b", "c"], Vec::new());
    let json = stored(300_001, &[serde_json::json!("x"), serde_json::json!("y")]);

    // Act
    let restored =
        record_from_storage_json(&json, &current, &mut text_converter()).expect("restore");

    // Assert
    assert_eq!(restored.fields(), &[text("x"), text("y"), Value::Null]);
}

#[test]
fn should_hide_dropped_attribute_slots_when_reading_stored_values() {
    // Arrange
    let current = layout(&["a", "c"], vec![1]);
    let json = stored(
        300_001,
        &[
            serde_json::json!("x"),
            serde_json::json!("hidden"),
            serde_json::json!("z"),
        ],
    );

    // Act
    let restored =
        record_from_storage_json(&json, &current, &mut text_converter()).expect("restore");

    // Assert
    assert_eq!(restored.fields(), &[text("x"), text("z")]);
}

#[test]
fn should_write_null_in_dropped_attribute_slots_of_the_storage_json() {
    // Arrange
    let current = layout(&["a", "c"], vec![1]);
    let record = RecordValue::new(current, vec![text("x"), text("z")]).expect("record");

    // Act
    let json = record_to_storage_json(&record);

    // Assert
    let slots = json[RECORD_STORAGE_KEY]["fields"]
        .as_array()
        .expect("slots")
        .clone();
    assert_eq!(
        slots,
        vec![
            serde_json::json!("x"),
            serde_json::Value::Null,
            serde_json::json!("z")
        ]
    );
}

#[test]
fn should_reject_stored_values_with_more_slots_than_the_descriptor() {
    // Arrange
    let current = layout(&["a"], Vec::new());
    let json = stored(300_001, &[serde_json::json!("x"), serde_json::json!("y")]);

    // Act
    let restored = record_from_storage_json(&json, &current, &mut text_converter());

    // Assert
    assert!(restored.is_err());
}
