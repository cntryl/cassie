#[test]
fn should_report_created_index_definition_in_pg_indexes() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-indexes-created-index",
        &[
            "CREATE SCHEMA issue_817_index_catalog",
            "CREATE TABLE issue_817_index_catalog.items (amount INTEGER)",
            "CREATE INDEX issue_817_catalog_amount_idx \
             ON issue_817_index_catalog.items USING btree (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename, indexname, indexdef \
             FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'issue_817_index_catalog' \
               AND indexname = 'issue_817_catalog_amount_idx'",
        )
        .expect("query the created index catalog row");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_index_catalog".to_owned()),
            Value::String("items".to_owned()),
            Value::String("issue_817_catalog_amount_idx".to_owned()),
            Value::String(
                "CREATE INDEX issue_817_catalog_amount_idx ON issue_817_index_catalog.items USING btree (amount)"
                    .to_owned(),
            ),
        ]]
    );
}

#[test]
fn should_quote_index_definition_identifiers_that_need_quoting() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-indexes-quoted-identifiers",
        &[
            "CREATE SCHEMA \"Issue817MixedCase\"",
            "CREATE TABLE \"Issue817MixedCase\".\"SelectTable\" (\
             \"select\" INTEGER, \"CamelInclude\" INTEGER)",
            "CREATE INDEX \"IndexName\" \
             ON \"Issue817MixedCase\".\"SelectTable\" (\"select\") \
             INCLUDE (\"CamelInclude\")",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT indexdef FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'Issue817MixedCase' \
               AND tablename = 'SelectTable' \
               AND indexname = 'IndexName'",
        )
        .expect("query the quoted index definition");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "CREATE INDEX \"IndexName\" ON \"Issue817MixedCase\".\"SelectTable\" USING btree (\"select\") INCLUDE (\"CamelInclude\")"
                .to_owned(),
        )]]
    );
}

#[test]
fn should_list_created_indexes_in_pg_stat_user_indexes() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-stat-user-indexes",
        &[
            "CREATE SCHEMA issue_817_index_stats",
            "CREATE TABLE issue_817_index_stats.items (amount INTEGER)",
            "CREATE INDEX issue_817_stats_amount_idx \
             ON issue_817_index_stats.items USING btree (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, relname, indexrelname \
             FROM pg_catalog.pg_stat_user_indexes \
             WHERE schemaname = 'issue_817_index_stats' \
               AND indexrelname = 'issue_817_stats_amount_idx'",
        )
        .expect("query the created index statistics row");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_index_stats".to_owned()),
            Value::String("items".to_owned()),
            Value::String("issue_817_stats_amount_idx".to_owned()),
        ]]
    );
}

#[test]
fn should_list_builtin_access_methods_in_pg_am() {
    // Arrange
    let fixture = sql_fixture("issue-817-access-method-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT amname FROM pg_catalog.pg_am \
             WHERE amname IN ('heap', 'btree', 'hash') ORDER BY amname",
        )
        .expect("read PostgreSQL table and index access methods");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![Value::String("btree".to_owned())],
            vec![Value::String("hash".to_owned())],
            vec![Value::String("heap".to_owned())],
        ]
    );
}

#[test]
fn should_list_access_method_operator_entries() {
    // Arrange
    let fixture = sql_fixture("issue-817-amop-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) > 0 FROM pg_catalog.pg_amop")
        .expect("query operator access method metadata");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_list_access_method_support_function_entries() {
    // Arrange
    let fixture = sql_fixture("issue-817-amproc-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) > 0 FROM pg_catalog.pg_amproc")
        .expect("query access method support function metadata");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_render_a_created_index_definition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-index-definition",
        &[
            "CREATE TABLE issue_817_definition_items (amount INTEGER)",
            "CREATE INDEX issue_817_definition_idx \
             ON issue_817_definition_items USING btree (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_indexdef(oid) \
             FROM pg_catalog.pg_class \
             WHERE relname = 'issue_817_definition_idx'",
        )
        .expect("render the created index");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "CREATE INDEX issue_817_definition_idx ON public.issue_817_definition_items USING btree (amount)"
                .to_owned()
        )]]
    );
}
