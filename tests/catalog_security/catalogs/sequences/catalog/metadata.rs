#[test]
fn should_report_sequence_catalog_option_values() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-sequence-populated-row", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE SEQUENCE issue_817_catalog_sequence \
             START WITH 7 INCREMENT BY 3 MINVALUE 2 MAXVALUE 41 CACHE 5 CYCLE",
        )
        .expect("create a sequence with distinguishing catalog options");

    // Act
    let rows = fixture
        .execute(
            "SELECT seqtypid, seqstart, seqincrement, seqmax, seqmin, seqcache, seqcycle \
             FROM pg_catalog.pg_sequence \
             WHERE seqrelid = 'issue_817_catalog_sequence'::regclass",
        )
        .expect("read the created sequence's pg_sequence row")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(20),
            Value::Int64(7),
            Value::Int64(3),
            Value::Int64(41),
            Value::Int64(2),
            Value::Int64(5),
            Value::Bool(true),
        ]]
    );
}
