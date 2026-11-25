#[test]
fn should_bind_postgres_character_type_forms() {
    // Arrange
    let label = "issue-796-character-type-aliases";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('word' AS character varying(4)) AS value", 1043),
        ("SELECT CAST('word' AS char varying(4)) AS value", 1043),
        (
            "SELECT CAST('word' AS national character varying(4)) AS value",
            1043,
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::String("word".to_owned())]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value word, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "character type cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_bpchar_aliases() {
    // Arrange
    let cases = [
        ("bpchar", -1),
        ("pg_catalog.bpchar", -1),
        ("bpchar(3)", 7),
        ("pg_catalog.bpchar(3)", 7),
    ];

    // Act
    let parsed = cases.map(|(spelling, _)| cassie::types::DataType::parse_sql(spelling));

    // Assert
    for ((spelling, typmod), parsed_type) in cases.into_iter().zip(parsed) {
        let data_type = parsed_type.unwrap_or_else(|error| panic!("{spelling}: {error}"));
        assert_eq!(data_type.type_oid(), 1_042, "{spelling} OID");
        assert_eq!(data_type.atttypmod(), typmod, "{spelling} typmod");
    }
    assert_eq!(
        cassie::types::DataType::parse_sql("CHAR")
            .expect("bare CHAR remains length one")
            .atttypmod(),
        5
    );
    assert_eq!(
        cassie::types::DataType::parse_sql("pg_catalog.unknown").unwrap_err(),
        "unsupported data type 'pg_catalog.unknown'"
    );
}

#[test]
fn should_bind_postgres_bpchar_cast_type_spelling() {
    // Arrange
    let fixture = sql_fixture("issue-796-bpchar-cast-type-spelling", &[]);

    // Act
    let result = fixture
        .execute("SELECT CAST('x' AS bpchar) AS value")
        .expect("bpchar CAST");

    // Assert
    assert_eq!(result.rows(), [vec![Value::String("x".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 1_042);
}

#[test]
fn should_bind_selected_bpchar_type_spelling() {
    // Arrange
    let label = "issue-796-type-synonyms";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [("SELECT CAST('x' AS bpchar) AS value", 1042)];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::String("x".to_owned())]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value x, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "selected character type spelling failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_reject_unselected_postgres_char_type_spelling() {
    // Arrange
    let fixture = sql_fixture("issue-796-unselected-type-names", &[]);
    let cases = [
        "SELECT NULL::\"char\"",
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|sql| match fixture.execute(sql) {
            Err(cassie::app::CassieError::Unsupported(_)) => None,
            Err(error) => Some(format!("{sql}: expected unsupported type, got {error}")),
            Ok(result) => Some(format!(
                "{sql}: expected unsupported type, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "unselected PostgreSQL type spellings were not rejected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_bind_selected_postgres_type_spellings_with_postgres_array_identity() {
    // Arrange
    let fixture = sql_fixture("issue-796-type-name-oids", &[]);
    let cases = [
        ("SELECT NULL::character(2)", 1042),
        ("SELECT NULL::bit varying", 1562),
        ("SELECT NULL::time with time zone", 1266),
        ("SELECT NULL::time without time zone", 1083),
        ("SELECT NULL::timestamp with time zone", 1184),
        ("SELECT NULL::timestamp without time zone", 1114),
        ("SELECT NULL::national character varying(4)", 1043),
        ("SELECT NULL::nchar(4)", 1042),
        ("SELECT NULL::char varying(4)", 1043),
        ("SELECT NULL::integer[]", 1_007),
        ("SELECT NULL::float4", 700),
        ("SELECT NULL::float8", 701),
        ("SELECT NULL::real", 700),
        ("SELECT NULL::float(24)", 700),
        ("SELECT NULL::float(25)", 701),
        ("SELECT NULL::dec", 1700),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result) if result.columns()[0].type_oid == *expected_oid => None,
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid}, got {}",
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL type-name cases failed:\n{}",
        failures.join("\n")
    );
}
