#[test]
fn should_return_null_for_an_uncompressed_text_value() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-compression", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_column_compression(CAST('plain' AS text)) IS NULL")
        .expect("read the compression method for plain text");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_a_positive_current_transaction_id() {
    // Arrange
    let fixture = sql_fixture("issue-817-current-transaction-id", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.txid_current() > 0")
        .expect("read the current transaction ID");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
