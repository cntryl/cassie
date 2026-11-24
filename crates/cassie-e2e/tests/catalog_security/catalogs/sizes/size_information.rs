




#[test]
fn should_format_bigint_size_with_null_input() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty-null-bigint", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_size_pretty(1048576::bigint) AS pretty, \
                    pg_catalog.pg_size_pretty(NULL::bigint) AS null_pretty",
        )
        .expect("format a selected bigint size and preserve NULL");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("pretty", 25), ("null_pretty", 25)]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("1024 kB".to_owned()), Value::Null]]
    );
}

#[test]
fn should_format_size_units_at_postgresql_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty-unit-boundaries", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_size_pretty(10239::bigint) AS below_ten_kib, \
                    pg_catalog.pg_size_pretty(10240::bigint) AS ten_kib, \
                    pg_catalog.pg_size_pretty(-10240::bigint) AS negative_ten_kib",
        )
        .expect("format bigint byte counts across unit boundaries");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("below_ten_kib", 25),
            ("ten_kib", 25),
            ("negative_ten_kib", 25),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("10239 bytes".to_owned()),
            Value::String("10 kB".to_owned()),
            Value::String("-10 kB".to_owned()),
        ]]
    );
}
