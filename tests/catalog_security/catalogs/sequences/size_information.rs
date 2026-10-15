#[test]
fn should_report_sizes_for_a_sequence_relation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-sequence-relation-sizes",
        &["CREATE SEQUENCE issue_817_size_sequence"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_relation_size('issue_817_size_sequence'::regclass) \
                        AS relation_bytes, \
                    pg_catalog.pg_table_size('issue_817_size_sequence'::regclass) \
                        AS table_bytes, \
                    pg_catalog.pg_total_relation_size('issue_817_size_sequence'::regclass) \
                        AS total_relation_bytes",
        )
        .expect("read relation sizes for the created sequence");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("relation_bytes", 20),
            ("table_bytes", 20),
            ("total_relation_bytes", 20),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(8192),
            Value::Int64(8192),
            Value::Int64(8192),
        ]]
    );
}
