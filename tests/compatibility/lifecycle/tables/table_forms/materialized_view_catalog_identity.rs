use super::*;

#[test]
fn should_expose_materialized_view_relation_identity_in_catalogs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_catalog_identity").await;
        let (client, connection) = server.connect().await;
        create_catalog_identity_fixture(&client).await;

        // Act
        let relation_oid = assert_initial_materialized_view_catalog_rows(&client).await;
        assert_refresh_and_rename_keep_materialized_view_identity(&client, relation_oid).await;
        assert_drop_removes_materialized_view_catalog_rows(&client).await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
    });
}

async fn create_catalog_identity_fixture(client: &tokio_postgres::Client) {
    execute_statements(
        client,
        &[
            "CREATE SCHEMA compat_mv_catalog",
            "SET search_path TO compat_mv_catalog, public",
            "CREATE TABLE compat_mv_catalog_source (item_id INTEGER, item_label TEXT)",
            "INSERT INTO compat_mv_catalog_source VALUES (1, 'first')",
            "CREATE MATERIALIZED VIEW compat_mv_catalog_identity (item_id, item_label) AS \
             SELECT item_id, item_label FROM compat_mv_catalog_source WITH NO DATA",
        ],
    )
    .await
    .expect("create the SQL materialized view without population");
}

async fn assert_initial_materialized_view_catalog_rows(client: &tokio_postgres::Client) -> i64 {
    let relation = client
        .query_one(
            "SELECT oid::BIGINT, relkind, relam::INT, relnamespace, \
                    relnamespace_oid::BIGINT FROM pg_catalog.pg_class \
             WHERE relname = 'compat_mv_catalog_identity'",
            &[],
        )
        .await
        .expect("read the materialized-view relation identity");
    let relation_oid: i64 = relation.get(0);
    assert_eq!(relation.get::<_, String>(1), "m");
    assert_eq!(relation.get::<_, i32>(2), 2);
    assert_eq!(relation.get::<_, String>(3), "compat_mv_catalog");
    let namespace_oid: i64 = relation.get(4);
    let expected_namespace_oid: i64 = client
        .query_one(
            "SELECT oid::BIGINT FROM pg_catalog.pg_namespace \
             WHERE nspname = 'compat_mv_catalog'",
            &[],
        )
        .await
        .expect("read the materialized-view namespace identity")
        .get(0);
    assert_eq!(namespace_oid, expected_namespace_oid);

    assert_unpopulated_materialized_view(client).await;
    assert_materialized_view_attribute_rows(client, relation_oid).await;
    assert_materialized_view_regclass_identity(client, relation_oid).await;
    relation_oid
}

async fn assert_unpopulated_materialized_view(client: &tokio_postgres::Client) {
    let populated: bool = client
        .query_one(
            "SELECT ispopulated FROM pg_catalog.pg_matviews \
             WHERE matviewname = 'compat_mv_catalog_identity'",
            &[],
        )
        .await
        .expect("read the unpopulated materialized-view state")
        .get(0);
    assert!(!populated);
}

async fn assert_materialized_view_attribute_rows(
    client: &tokio_postgres::Client,
    relation_oid: i64,
) {
    let attributes = client
        .query(
            "SELECT attrelid_oid::BIGINT, attname, attnum, atttypid::INT \
             FROM pg_catalog.pg_attribute \
             WHERE attrelid = 'compat_mv_catalog_identity' ORDER BY attnum",
            &[],
        )
        .await
        .expect("read the materialized-view output columns");
    assert_eq!(attributes.len(), 2);
    assert_attribute(&attributes[0], relation_oid, "item_id", 1, 23);
    assert_attribute(&attributes[1], relation_oid, "item_label", 2, 25);
}

fn assert_attribute(
    attribute: &tokio_postgres::Row,
    relation_oid: i64,
    name: &str,
    number: i32,
    type_oid: i32,
) {
    assert_eq!(attribute.get::<_, i64>(0), relation_oid);
    assert_eq!(attribute.get::<_, String>(1), name);
    assert_eq!(attribute.get::<_, i32>(2), number);
    assert_eq!(attribute.get::<_, i32>(3), type_oid);
}

async fn assert_materialized_view_regclass_identity(
    client: &tokio_postgres::Client,
    relation_oid: i64,
) {
    let regclass = client
        .query_one(
            "SELECT 'compat_mv_catalog_identity'::regclass::oid, \
                    to_regclass('compat_mv_catalog_identity')::oid, \
                    to_regclass('compat_mv_catalog.compat_mv_catalog_identity')::oid",
            &[],
        )
        .await
        .expect("resolve unqualified and schema-qualified regclass names");
    for index in 0..3 {
        assert_eq!(i64::from(regclass.get::<_, u32>(index)), relation_oid);
    }
}

async fn assert_refresh_and_rename_keep_materialized_view_identity(
    client: &tokio_postgres::Client,
    relation_oid: i64,
) {
    client
        .batch_execute("INSERT INTO compat_mv_catalog_source VALUES (2, 'second')")
        .await
        .expect("change the materialized-view source");
    client
        .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_catalog_identity")
        .await
        .expect("refresh the materialized view");
    let refreshed_oid: i64 = client
        .query_one(
            "SELECT oid::BIGINT FROM pg_catalog.pg_class \
             WHERE relname = 'compat_mv_catalog_identity' AND relkind = 'm'",
            &[],
        )
        .await
        .expect("read the refreshed relation identity")
        .get(0);

    client
        .batch_execute(
            "ALTER MATERIALIZED VIEW compat_mv_catalog_identity \
             RENAME TO compat_mv_catalog_renamed",
        )
        .await
        .expect("rename the materialized view");
    assert_renamed_materialized_view_identity(client, relation_oid, refreshed_oid).await;
}

async fn assert_renamed_materialized_view_identity(
    client: &tokio_postgres::Client,
    relation_oid: i64,
    refreshed_oid: i64,
) {
    let renamed = client
        .query_one(
            "SELECT oid::BIGINT, relkind FROM pg_catalog.pg_class \
             WHERE relname = 'compat_mv_catalog_renamed'",
            &[],
        )
        .await
        .expect("read the renamed relation identity");
    assert_eq!(renamed.get::<_, i64>(0), relation_oid);
    assert_eq!(renamed.get::<_, String>(1), "m");
    assert_eq!(refreshed_oid, relation_oid);

    let renamed_regclass: u32 = client
        .query_one("SELECT to_regclass('compat_mv_catalog_renamed')::oid", &[])
        .await
        .expect("resolve the renamed materialized view through regclass")
        .get(0);
    assert_eq!(i64::from(renamed_regclass), relation_oid);
    assert_eq!(
        resolve_regclass_label(client, "compat_mv_catalog_renamed").await,
        "compat_mv_catalog_renamed"
    );
    assert_eq!(
        resolve_oid_regclass_label(client, relation_oid).await,
        "compat_mv_catalog_renamed"
    );
    let old_name_missing: bool = client
        .query_one(
            "SELECT to_regclass('compat_mv_catalog_identity') IS NULL",
            &[],
        )
        .await
        .expect("check that the old relation name no longer resolves")
        .get(0);
    assert!(old_name_missing);
}

async fn resolve_regclass_label(client: &tokio_postgres::Client, name: &str) -> String {
    client
        .query_one(&format!("SELECT to_regclass('{name}')::TEXT"), &[])
        .await
        .expect("render the renamed materialized-view regclass identity")
        .get(0)
}

async fn resolve_oid_regclass_label(client: &tokio_postgres::Client, relation_oid: i64) -> String {
    client
        .query_one(&format!("SELECT {relation_oid}::oid::regclass::TEXT"), &[])
        .await
        .expect("resolve the stable relation OID after rename")
        .get(0)
}

async fn assert_drop_removes_materialized_view_catalog_rows(client: &tokio_postgres::Client) {
    client
        .batch_execute("DROP MATERIALIZED VIEW compat_mv_catalog_renamed")
        .await
        .expect("drop the materialized view");
    let dropped_relation: bool = client
        .query_one(
            "SELECT count(*) = 0 FROM pg_catalog.pg_class \
             WHERE relname = 'compat_mv_catalog_renamed'",
            &[],
        )
        .await
        .expect("confirm relation catalog cleanup")
        .get(0);
    let dropped_attributes: bool = client
        .query_one(
            "SELECT count(*) = 0 FROM pg_catalog.pg_attribute \
             WHERE attrelid = 'compat_mv_catalog_renamed'",
            &[],
        )
        .await
        .expect("confirm relation-column catalog cleanup")
        .get(0);
    let dropped_regclass: bool = client
        .query_one(
            "SELECT to_regclass('compat_mv_catalog_renamed') IS NULL",
            &[],
        )
        .await
        .expect("confirm the dropped relation no longer resolves")
        .get(0);
    assert!(dropped_relation);
    assert!(dropped_attributes);
    assert!(dropped_regclass);
}
