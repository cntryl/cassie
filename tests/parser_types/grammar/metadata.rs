#[test]
fn should_map_float_precision_boundaries_to_postgres_type_oids() {
    // Arrange
    let label = "issue-796-float-precision-boundaries";
    let fixture = sql_fixture(label, &[]);
    let real_value = Value::String("1.2345679".to_owned());
    let float_value = Value::Float64(1.234_567_89_f64);

    // Act
    let cases = [
        (
            "SELECT CAST('1.23456789' AS FLOAT(1)) AS value",
            700,
            real_value.clone(),
        ),
        (
            "SELECT CAST('1.23456789' AS FLOAT (24)) AS value",
            700,
            real_value.clone(),
        ),
        (
            "SELECT CAST('1.23456789' AS FLOAT(25)) AS value",
            701,
            float_value.clone(),
        ),
        (
            "SELECT CAST('1.23456789' AS FLOAT(53)) AS value",
            701,
            float_value.clone(),
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid, expected_value)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![expected_value.clone()]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value {expected_value:?}, got {:?} / {}",
                result.rows(), result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "float precision cases failed:\n{}",
        failures.join("\n")
    );

    let invalid_cases = [
        (
            "SELECT CAST('1.5' AS FLOAT(0)) AS value",
            "FLOAT precision must be at least 1 bit",
        ),
        (
            "SELECT CAST('1.5' AS FLOAT(54)) AS value",
            "FLOAT precision must be less than 54 bits",
        ),
    ];
    for (sql, expected_message) in invalid_cases {
        let error = fixture.error(sql);
        assert!(
            matches!(
                &error,
                cassie::app::CassieError::InvalidParameterValue(message)
                    if message == expected_message
            ),
            "{sql}: expected SQLSTATE 22023 and {expected_message:?}, got {error:?}"
        );
    }
}

#[test]
fn should_apply_float_precision_type_selection_to_declared_columns() {
    // Arrange
    let fixture = sql_fixture("issue-796-float-precision-declarations", &[]);
    fixture
        .execute(
            "CREATE TABLE float_precision_declarations \
             (single_precision FLOAT(24), double_precision FLOAT(25))",
        )
        .expect("FLOAT precision declarations use the selected existing types");
    fixture
        .execute(
            "INSERT INTO float_precision_declarations VALUES \
             (CAST('1.23456789' AS FLOAT(24)), CAST('1.23456789' AS FLOAT(25)))",
        )
        .expect("values insert through both selected float representations");

    // Act
    let result = fixture.execute(
        "SELECT single_precision, double_precision FROM float_precision_declarations",
    );

    // Assert
    let result = result.expect("declared FLOAT precisions remain readable");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("1.2345679".to_owned()),
            Value::Float64(1.234_567_89_f64),
        ]]
    );
    assert_eq!(result.columns()[0].type_oid, 700);
    assert_eq!(result.columns()[1].type_oid, 701);
    assert_eq!(result.columns()[0].atttypmod, -1);
    assert_eq!(result.columns()[1].atttypmod, -1);
}
