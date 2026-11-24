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
fn should_omit_cassie_internal_index_methods_from_postgresql_catalogs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-internal-index-catalog-boundary",
        &[
            "CREATE SCHEMA issue_817_internal_index_catalog",
            "CREATE TABLE issue_817_internal_index_catalog.events (\
                 event_at TIMESTAMPTZ NOT NULL, tenant TEXT NOT NULL, payload TEXT)",
            "CREATE INDEX issue_817_events_column_idx \
                 ON issue_817_internal_index_catalog.events USING column (payload) \
                 WITH (segment_size = 1)",
            "CREATE INDEX issue_817_events_timeseries_idx \
                 ON issue_817_internal_index_catalog.events USING time_series (event_at) \
                 WITH (bucket_width = '1 hour', partition_by = tenant)",
            "CREATE INDEX issue_817_events_btree_idx \
                 ON issue_817_internal_index_catalog.events USING btree (tenant)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let class_rows = fixture
        .execute(
            "SELECT relname FROM pg_catalog.pg_class \
             WHERE relname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY relname",
        )
        .expect("read selected pg_class index rows");
    let access_method_rows = fixture
        .execute(
            "SELECT index_class.relname, index_class.relkind, index_class.relam, \
                    access_method.amname \
             FROM pg_catalog.pg_class AS index_class \
             JOIN pg_catalog.pg_am AS access_method \
               ON access_method.oid = index_class.relam \
             WHERE index_class.relname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY index_class.relname",
        )
        .expect("join emitted index relations to genuine access methods");
    let index_rows = fixture
        .execute(
            "SELECT indexrelid FROM pg_catalog.pg_index \
             WHERE indexrelid IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY indexrelid",
        )
        .expect("read selected pg_index rows");
    let index_views = fixture
        .execute(
            "SELECT indexname FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'issue_817_internal_index_catalog' \
               AND indexname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY indexname",
        )
        .expect("read selected pg_indexes rows");

    // Assert
    let expected = vec![vec![Value::String(
        "issue_817_events_btree_idx".to_owned(),
    )]];
    assert_eq!(class_rows.rows(), expected);
    assert_eq!(
        access_method_rows.rows(),
        vec![vec![
            Value::String("issue_817_events_btree_idx".to_owned()),
            Value::String("i".to_owned()),
            Value::Int64(403),
            Value::String("btree".to_owned()),
        ]]
    );
    assert_eq!(index_rows.rows(), expected);
    assert_eq!(index_views.rows(), expected);
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
