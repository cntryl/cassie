#[test]
fn should_zip_multiple_unnest_arrays_with_ordinality_null_padding() {
    // Arrange
    let fixture = sql_fixture("issue-808-multiple-unnest-ordinality-padding", &[]);

    // Act
    let result = fixture.execute(
        "SELECT expanded.number, expanded.label, expanded.ordinality FROM unnest('{10,20}'::INT[], '{a,b,c}'::TEXT[]) WITH ORDINALITY AS expanded(number, label, ordinality) ORDER BY expanded.ordinality",
    );

    // Assert
    let result = result.expect("multi-array UNNEST zips inputs and pads shorter arrays with NULL");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(10),
                Value::String("a".to_owned()),
                Value::Int64(1),
            ],
            vec![
                Value::Int64(20),
                Value::String("b".to_owned()),
                Value::Int64(2),
            ],
            vec![Value::Null, Value::String("c".to_owned()), Value::Int64(3),],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25, 20]
    );
}
