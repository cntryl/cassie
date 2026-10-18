#[test]
fn should_report_created_table_default_expression() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-table-default");

    // Act
    let defaults = fixture
        .execute(
            "SELECT column_name, column_default \
             FROM information_schema.columns \
             WHERE table_schema = 'issue_817_catalog_probe' \
               AND table_name = 'items' AND column_name = 'amount'",
        )
        .expect("read stored column default");

    // Assert
    assert_eq!(
        defaults.rows(),
        vec![vec![
            Value::String("amount".to_owned()),
            Value::String("1.25".to_owned()),
        ]]
    );
}

#[test]
fn should_list_the_default_collation_in_pg_collation() {
    // Arrange
    let fixture = sql_fixture("issue-817-default-collation", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT collname, collprovider FROM pg_catalog.pg_collation \
             WHERE collname = 'default'",
        )
        .expect("read the default PostgreSQL collation");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("default".to_owned()),
            Value::String("d".to_owned()),
        ]]
    );
}

#[test]
fn should_list_the_default_tablespace_in_pg_tablespace() {
    // Arrange
    let fixture = sql_fixture("issue-817-tablespace-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT spcname FROM pg_catalog.pg_tablespace WHERE spcname = 'pg_default'")
        .expect("query the PostgreSQL tablespace catalog");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("pg_default".to_owned())]]
    );
}

#[test]
fn should_list_default_text_search_configuration_mappings() {
    // Arrange
    let fixture = sql_fixture("issue-817-text-search-map-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) > 0 FROM pg_catalog.pg_ts_config_map")
        .expect("query the text search configuration mappings");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
