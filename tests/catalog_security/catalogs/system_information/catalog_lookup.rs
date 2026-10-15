#[test]
fn should_map_postgresql_character_encoding_names() {
    // Arrange
    let fixture = sql_fixture("issue-817-character-encoding-ids", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_char_to_encoding('UTF8'), \
                    pg_catalog.pg_char_to_encoding('SQL_ASCII'), \
                    pg_catalog.pg_char_to_encoding('not-an-encoding')",
        )
        .expect("resolve PostgreSQL character encoding identifiers");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(6), Value::Int64(0), Value::Int64(-1)]]
    );
}

#[test]
fn should_describe_reserved_keywords_from_the_server_keyword_table() {
    // Arrange
    let fixture = sql_fixture("issue-817-keyword-catalog-row", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT word, catcode, barelabel \
             FROM pg_catalog.pg_get_keywords() \
             WHERE word = 'select'",
        )
        .expect("read the server's SELECT keyword metadata");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 18, 16]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("select".to_owned()),
            Value::String("R".to_owned()),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_report_the_single_node_recovery_state() {
    // Arrange
    let fixture = sql_fixture("issue-817-recovery-state", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_is_in_recovery()")
        .expect("read recovery state");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_return_zero_when_the_session_has_no_temporary_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-no-temporary-schema", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_my_temp_schema()")
        .expect("read the current session temporary schema OID");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_zero_is_not_another_session_temporary_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-other-temporary-schema", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_is_other_temp_schema(0)")
        .expect("check whether the schema belongs to another session");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_report_postmaster_start_time_as_available() {
    // Arrange
    let fixture = sql_fixture("issue-817-postmaster-start-time", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_postmaster_start_time() IS NOT NULL")
        .expect("read the server start time");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_a_loaded_configuration_timestamp() {
    // Arrange
    let fixture = sql_fixture("issue-817-configuration-load-time", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_conf_load_time() IS NOT NULL")
        .expect("read the configuration load timestamp");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
