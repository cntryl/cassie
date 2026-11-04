use super::*;
use crate::catalog::FieldMeta;
use crate::types::SqlArray;
use std::time::Instant;

#[test]
fn should_hydrate_character_images_using_exact_returning_field_types() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let character = DataType::Array(Box::new(DataType::Char { length: Some(3) }));
    let integers = DataType::Array(Box::new(DataType::Int));
    let schema = CollectionSchema {
        collection: "target".into(),
        fields: [
            ("items", character.clone()),
            ("o.items", character.clone()),
            ("src.items", integers.clone()),
        ]
        .map(|(name, data_type)| FieldMeta {
            name: name.into(),
            data_type,
            is_indexed: false,
            boost: None,
        })
        .to_vec(),
    };
    let image = SqlArray::from_elements(vec![Value::String("x".into()), Value::Null])
        .expect("canonical image");
    let source = SqlArray::from_elements(vec![Value::Int64(7)]).expect("native source");
    let mut rows = vec![
        BatchRow::from_projected_values(vec![
            ("o.items".into(), Value::Json(image.to_storage_json())),
            ("src.items".into(), Value::Array(source.clone())),
        ]),
        BatchRow::from_projected_values(vec![
            (
                crate::types::row_identity::ROW_IDENTITY_COLUMN.into(),
                Value::String("stored".into()),
            ),
            ("o.items".into(), Value::Json(image.to_storage_json())),
            ("src.items".into(), Value::Array(source)),
        ]),
    ];
    let capacity = rows.capacity();

    // Act
    prepare_inputs(&mut rows, capacity, Some(&schema), &controls)
        .expect("controlled returning inputs");
    let metadata = METADATA_OWNER
        .with(|slot| slot.borrow().clone())
        .expect("typed metadata owner");

    // Assert
    assert_eq!(rows[0].data_types(), &[character.clone(), integers.clone()]);
    assert_eq!(&rows[1].data_types()[1..], &[character, integers]);
    assert!(metadata.upgrade().is_some());
    for row in &rows {
        let Some(Value::Array(array)) = row.get("o.items") else {
            panic!("native image");
        };
        assert_eq!(
            array.elements(),
            &[Value::String("x  ".into()), Value::Null]
        );
    }
    drop(rows);
    assert!(metadata.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
