#[test]
fn should_describe_pg_sequence_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-sequence-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_sequence");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "seqrelid",
            "seqtypid",
            "seqstart",
            "seqincrement",
            "seqmax",
            "seqmin",
            "seqcache",
            "seqcycle",
        ])
    );
}
