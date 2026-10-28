#[test]
fn should_list_created_relations_through_pg_tables() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-tables-created-relation",
        &[
            "CREATE SCHEMA issue_817_tables",
            "CREATE TABLE issue_817_tables.visible (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename FROM pg_catalog.pg_tables \
             WHERE schemaname = 'issue_817_tables' AND tablename = 'visible'",
        )
        .expect("query pg_tables for the created relation");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_tables".to_owned()),
            Value::String("visible".to_owned()),
        ]]
    );
}

#[test]
fn should_list_utc_in_pg_timezone_names() {
    // Arrange
    let fixture = sql_fixture("issue-817-utc-timezone-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT name, abbrev, utc_offset, is_dst \
             FROM pg_catalog.pg_timezone_names WHERE name = 'UTC'",
        )
        .expect("read the UTC timezone entry");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("UTC".to_owned()),
            Value::String("UTC".to_owned()),
            Value::String("00:00:00".to_owned()),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_render_a_created_view_definition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-view-definition",
        &["CREATE VIEW issue_817_definition_view AS SELECT 42 AS answer"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_viewdef('issue_817_definition_view')")
        .expect("read the created view definition");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(" SELECT 42 AS answer;".to_owned())]]
    );
}

#[test]
fn should_resolve_view_definition_through_the_session_search_path() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-search-path-view-definition",
        &[
            "CREATE SCHEMA view_definition_first",
            "CREATE SCHEMA view_definition_second",
            "CREATE VIEW view_definition_second.answer AS SELECT 99 AS value",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("SET search_path TO view_definition_first, view_definition_second")
        .expect("set the view lookup path");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_viewdef('answer')")
        .expect("resolve a view in the session search path");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(" SELECT 99 AS value;".to_owned())]]
    );
}

#[test]
fn should_return_null_for_null_view_definition_input() {
    // Arrange
    let fixture = sql_fixture("issue-817-null-view-definition", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_viewdef(NULL)")
        .expect("evaluate a null view name");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Null]]);
}

#[test]
fn should_list_created_views_in_pg_views() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-views-created-view",
        &["CREATE VIEW issue_817_catalog_view AS SELECT 42 AS answer"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, viewname FROM pg_catalog.pg_views \
             WHERE schemaname = 'public' AND viewname = 'issue_817_catalog_view'",
        )
        .expect("query pg_views for the created view");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("public".to_owned()),
            Value::String("issue_817_catalog_view".to_owned()),
        ]]
    );
}

#[test]
fn should_list_effective_search_path_schemas() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-current-schemas",
        &["CREATE SCHEMA issue_817_search_path"],
    );
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("SET search_path TO issue_817_search_path, public")
        .expect("set a deterministic visible schema path");

    // Act
    let selected = fixture
        .execute(
            "SELECT current_schemas(true)::text, \
                    current_schemas(false)::text, \
                    pg_catalog.pg_typeof(current_schemas(true))::text",
        )
        .expect("read the effective schemas and array type");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("{pg_catalog,issue_817_search_path,public}".to_owned()),
            Value::String("{issue_817_search_path,public}".to_owned()),
            Value::String("name[]".to_owned()),
        ]]
    );
}

#[test]
fn should_keep_explicit_pg_catalog_position_in_current_schemas() {
    // Arrange
    let fixture = sql_fixture("issue-817-explicit-pg-catalog", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("SET search_path TO public, pg_catalog")
        .expect("set an explicit pg_catalog position");

    // Act
    let selected = fixture
        .execute("SELECT current_schemas(true)::text, current_schemas(false)::text")
        .expect("read explicit schema order");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("{public,pg_catalog}".to_owned()),
            Value::String("{public,pg_catalog}".to_owned()),
        ]]
    );
}
