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
fn should_double_quotes_and_backslashes_when_rendering() {
    // Arrange
    let record = text_record(vec![text("say \"hi\"\\")]);

    // Act
    let rendered = format_record(&record, default_session());

    // Assert
    assert_eq!(rendered, "(\"say \"\"hi\"\"\\\\\")");
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
    // Arrange / Act
    let plain = split_record_text("(x,1)", 2).expect("plain");
    let nulls = split_record_text("(,)", 2).expect("nulls");
    let quoted = split_record_text("(\"\",\"a \"\"q\"\" b\\\\c\")", 2).expect("quoted");
    let spaced = split_record_text("  ( a , b ) ", 2).expect("spaced");

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
fn should_order_a_null_field_after_every_value_and_equate_null_fields() {
    // Arrange
    let low = text_record(vec![text("a"), Value::Null]);
    let high = text_record(vec![text("a"), text("z")]);
    let same = text_record(vec![text("a"), Value::Null]);

    // Act
    let (low_key, high_key, same_key) = (record_key(&low), record_key(&high), record_key(&same));

    // Assert
    assert!(high_key < low_key);
    assert_eq!(low_key, same_key);
}

#[test]
fn should_order_integers_and_floats_by_value() {
    // Arrange
    let descriptor = CompositeRef::new(CompositeType::anonymous(vec![
        DataType::BigInt,
        DataType::Float,
    ]));
    let record = |integer: i64, float: f64| {
        RecordValue::new(
            descriptor.clone(),
            vec![Value::Int64(integer), Value::Float64(float)],
        )
        .expect("record")
    };

    // Act
    let negative = record_key(&record(-5, 0.0));
    let positive = record_key(&record(3, 0.0));
    let zero = record_key(&record(0, 0.0));
    let negative_zero = record_key(&record(0, -0.0));
    let nan = record_key(&record(0, f64::NAN));
    let big = record_key(&record(0, 1.0e300));

    // Assert
    assert!(negative < zero && zero < positive);
    assert_eq!(zero, negative_zero);
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
        Value::String(value.as_str().unwrap_or_default().to_string())
    });

    // Assert
    assert_eq!(restored, Some(record));
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
