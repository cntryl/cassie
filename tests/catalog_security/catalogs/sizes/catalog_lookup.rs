#[test]
fn should_parse_postgresql_size_strings_into_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-bytes", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_size_bytes('1 kB'), pg_catalog.pg_size_bytes('1.5 MB')")
        .expect("parse PostgreSQL size strings");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1_024), Value::Int64(1_572_864)]]
    );
}

#[test]
fn should_report_positive_size_for_the_active_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-database-size", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_database_size(current_database()) > 0")
        .expect("read the active database size");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
