#[test]
fn should_report_exact_postgresql_value_sizes() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-size-values", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_column_size(CAST(0 AS INTEGER)), \
                    pg_catalog.pg_column_size(CAST(0 AS BIGINT)), \
                    pg_catalog.pg_column_size(CAST('foo' AS TEXT))",
        )
        .expect("query PostgreSQL datum sizes");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(4), Value::Int64(8), Value::Int64(7)]]
    );
}

#[test]
fn should_format_integer_byte_sizes_using_postgresql_units() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_size_pretty(CAST(1024 AS BIGINT))")
        .expect("format the byte count");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("1024 bytes".to_owned())]]
    );
}
