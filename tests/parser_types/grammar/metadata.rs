#[test]
fn should_map_float_precision_boundaries_to_postgres_type_oids() {
    // Arrange
    let label = "issue-796-float-precision-boundaries";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('1.5' AS FLOAT(24)) AS value", 700),
        ("SELECT CAST('1.5' AS FLOAT(25)) AS value", 701),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::Float64(1.5)]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value 1.5, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
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
}
