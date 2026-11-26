use super::{key_is_valid, key_of, KeyPart};
use crate::types::{DataType, Value};

const UUID: &str = "00112233-4455-6677-8899-AABBCCDDEEFF";

fn read_values<'a>(
    columns: &'a [String],
    values: &'a [Option<Value>],
    reads: &'a mut Vec<usize>,
) -> impl FnMut(&str) -> Option<&'a Value> + 'a {
    move |name| {
        let index = columns
            .iter()
            .position(|column| column == name)
            .expect("mapped key column");
        reads.push(index);
        values[index].as_ref()
    }
}

#[test]
fn should_accept_a_mixed_key_tuple_with_exact_scalar_components() {
    // Arrange
    let columns = ["small", "regular", "large", "label", "id"].map(str::to_owned);
    let types = [
        DataType::SmallInt,
        DataType::Int,
        DataType::BigInt,
        DataType::Text,
        DataType::Uuid,
    ];
    let values = [
        Value::Int64(i64::from(i16::MIN)),
        Value::Int64(i64::from(i32::MAX)),
        Value::Int64(i64::MIN),
        Value::String("λ".to_owned()),
        Value::String(UUID.to_owned()),
    ]
    .map(Some);
    let mut owned_reads = Vec::new();
    let mut validity_reads = Vec::new();

    // Act
    let owned = key_of(
        read_values(&columns, &values, &mut owned_reads),
        &columns,
        &types,
    );
    let valid = key_is_valid(
        read_values(&columns, &values, &mut validity_reads),
        &columns,
        &types,
    );

    // Assert
    assert!(valid);
    assert_eq!(validity_reads, owned_reads);
    assert_eq!(
        owned.expect("valid key"),
        [
            KeyPart::SmallInt(i16::MIN),
            KeyPart::Integer(i32::MAX),
            KeyPart::BigInt(i64::MIN),
            KeyPart::Text("λ".to_owned()),
            KeyPart::Uuid(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff_u128.to_be_bytes()),
        ],
    );
    assert_eq!(owned_reads, [0, 1, 2, 3, 4]);
}

#[test]
fn should_stop_at_the_first_invalid_component_of_a_mixed_graph_key() {
    // Arrange
    let columns = ["label", "number", "id"].map(str::to_owned);
    let uuid = Some(Value::String(UUID.to_owned()));
    let cases = [
        (DataType::Int, None, uuid.clone(), 2),
        (DataType::Int, Some(Value::Null), uuid.clone(), 2),
        (
            DataType::SmallInt,
            Some(Value::Int64(i64::from(i16::MIN) - 1)),
            uuid.clone(),
            2,
        ),
        (
            DataType::Int,
            Some(Value::Int64(i64::from(i32::MAX) + 1)),
            uuid.clone(),
            2,
        ),
        (
            DataType::BigInt,
            Some(Value::String("1".to_owned())),
            uuid.clone(),
            2,
        ),
        (DataType::Boolean, Some(Value::Bool(true)), uuid, 2),
        (
            DataType::BigInt,
            Some(Value::Int64(1)),
            Some(Value::String("malformed uuid".to_owned())),
            3,
        ),
    ];

    for (number_type, number, uuid, examined) in cases {
        let types = [DataType::Text, number_type, DataType::Uuid];
        let values = [Some(Value::String("key".to_owned())), number, uuid];
        let mut owned_reads = Vec::new();
        let mut validity_reads = Vec::new();

        // Act
        let owned = key_of(
            read_values(&columns, &values, &mut owned_reads),
            &columns,
            &types,
        );
        let valid = key_is_valid(
            read_values(&columns, &values, &mut validity_reads),
            &columns,
            &types,
        );

        // Assert
        assert!(!valid, "{types:?}: {values:?}");
        assert_eq!(validity_reads, owned_reads);
        assert!(owned.is_none(), "{types:?}: {values:?}");
        assert_eq!(owned_reads, (0..examined).collect::<Vec<_>>());
    }
}

#[test]
fn should_read_only_paired_columns_in_the_graph_key_tuple() {
    // Arrange
    let columns = ["mapped", "unpaired"].map(str::to_owned);
    let values = [Some(Value::Int64(3)), None];
    let mut owned_reads = Vec::new();
    let mut validity_reads = Vec::new();

    // Act
    let owned = key_of(
        read_values(&columns, &values, &mut owned_reads),
        &columns,
        &[DataType::Int],
    );
    let valid = key_is_valid(
        read_values(&columns, &values, &mut validity_reads),
        &columns,
        &[DataType::Int],
    );

    // Assert
    assert!(valid);
    assert_eq!(validity_reads, owned_reads);
    assert_eq!(
        owned.expect("only the paired column is required"),
        [KeyPart::Integer(3)]
    );
    assert_eq!(owned_reads, [0]);
}
