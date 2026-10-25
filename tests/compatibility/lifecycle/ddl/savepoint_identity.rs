use super::*;

struct RelationState {
    identity: tokio_postgres::Row,
    amount_metadata: tokio_postgres::Row,
    rows: Vec<tokio_postgres::Row>,
}

struct CommittedState {
    identity: tokio_postgres::Row,
    rows: Vec<tokio_postgres::Row>,
    default: tokio_postgres::Row,
}

async fn read_relation_identity(client: &tokio_postgres::Client) -> tokio_postgres::Row {
    client
        .query_one(
            "SELECT relation.oid::BIGINT, index_relation.oid::BIGINT \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS relation_schema \
               ON relation_schema.nspname = relation.relnamespace \
             JOIN pg_catalog.pg_index AS index_metadata \
               ON index_metadata.indrelid = relation.relname \
             JOIN pg_catalog.pg_class AS index_relation \
               ON index_relation.relname = index_metadata.indexrelid \
             WHERE relation_schema.nspname = 'public' \
               AND relation.relname = 'compat_issue839_savepoint_rows' \
               AND index_metadata.indisprimary",
            &[],
        )
        .await
        .expect("read relation and primary-key index identity")
}

async fn read_state_after_savepoint_rollback(client: &tokio_postgres::Client) -> RelationState {
    let identity = read_relation_identity(client).await;
    let amount_metadata = client
        .query_one(
            "SELECT attribute.atttypid::BIGINT, attribute.atttypmod, \
             NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute AS added \
               WHERE added.attrelid = relation.oid \
                 AND added.attname = 'transient' AND NOT added.attisdropped), \
             pg_catalog.to_regclass(\
               'public.compat_issue839_savepoint_renamed') IS NULL \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS relation_schema \
               ON relation_schema.oid = relation.relnamespace \
             JOIN pg_catalog.pg_attribute AS attribute \
               ON attribute.attrelid = relation.oid \
             WHERE relation_schema.nspname = 'public' \
               AND relation.relname = 'compat_issue839_savepoint_rows' \
               AND attribute.attname = 'amount'",
            &[],
        )
        .await
        .expect("read original column metadata after savepoint rollback");
    let rows = client
        .query(
            "SELECT id, amount FROM compat_issue839_savepoint_rows ORDER BY id",
            &[],
        )
        .await
        .expect("read original values after savepoint rollback");
    RelationState {
        identity,
        amount_metadata,
        rows,
    }
}

async fn read_committed_state(client: &tokio_postgres::Client) -> CommittedState {
    let identity = read_relation_identity(client).await;
    let rows = client
        .query(
            "SELECT id, amount, committed_after_rollback \
             FROM compat_issue839_savepoint_rows ORDER BY id",
            &[],
        )
        .await
        .expect("read rows after the outer commit");
    let default = client
        .query_one(
            "SELECT column_default FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'compat_issue839_savepoint_rows' \
               AND column_name = 'committed_after_rollback'",
            &[],
        )
        .await
        .expect("read the committed column default");
    CommittedState {
        identity,
        rows,
        default,
    }
}

fn assert_lifecycle_identity(before: &tokio_postgres::Row, after: &tokio_postgres::Row) {
    assert_eq!(
        (before.get::<_, i64>(0), before.get::<_, i64>(1),),
        (after.get::<_, i64>(0), after.get::<_, i64>(1),),
        "savepoint rollback must preserve the original relation and index OIDs"
    );
}

fn assert_savepoint_rollback_state(before: &tokio_postgres::Row, state: &RelationState) {
    assert_lifecycle_identity(before, &state.identity);
    assert_eq!(state.amount_metadata.get::<_, i64>(0), 23);
    assert_eq!(state.amount_metadata.get::<_, i32>(1), -1);
    assert!(state.amount_metadata.get::<_, bool>(2));
    assert!(state.amount_metadata.get::<_, bool>(3));
    assert_eq!(
        state
            .rows
            .iter()
            .map(|row| (row.get::<_, i32>(0), row.get::<_, i32>(1)))
            .collect::<Vec<_>>(),
        vec![(1, 41), (2, 42)]
    );
}

fn assert_committed_state(before: &tokio_postgres::Row, state: &CommittedState) {
    assert_lifecycle_identity(before, &state.identity);
    assert_eq!(
        state
            .rows
            .iter()
            .map(|row| (
                row.get::<_, i32>(0),
                row.get::<_, i32>(1),
                row.get::<_, i32>(2),
            ))
            .collect::<Vec<_>>(),
        vec![(1, 41, 9), (2, 42, 9)]
    );
    assert_eq!(state.default.get::<_, String>(0), "9");
}

#[test]
fn should_restore_existing_relation_identity_after_lifecycle_savepoint_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_savepoint_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_savepoint_rows \
                 (id INT PRIMARY KEY, amount INT NOT NULL)",
                "INSERT INTO compat_issue839_savepoint_rows VALUES (1, 41), (2, 42)",
            ],
        )
        .await
        .expect("create a populated relation with a primary-key index");
        let identity_before = read_relation_identity(&client).await;

        // Act
        execute_statements(
            &client,
            &[
                "BEGIN",
                "SAVEPOINT before_lifecycle",
                "ALTER TABLE compat_issue839_savepoint_rows \
                 RENAME TO compat_issue839_savepoint_renamed",
                "ALTER TABLE compat_issue839_savepoint_renamed \
                 ALTER COLUMN amount TYPE NUMERIC(20,0) USING amount::numeric, \
                 ADD COLUMN transient TEXT DEFAULT 'temporary'",
                "DROP TABLE compat_issue839_savepoint_renamed",
                "ROLLBACK TO SAVEPOINT before_lifecycle",
            ],
        )
        .await
        .expect("roll back rename, ALTER, and DROP to the savepoint");
        let after_rollback = read_state_after_savepoint_rollback(&client).await;
        execute_statements(
            &client,
            &[
                "RELEASE SAVEPOINT before_lifecycle",
                "ALTER TABLE compat_issue839_savepoint_rows \
                 ADD COLUMN committed_after_rollback INT DEFAULT 9",
                "COMMIT",
            ],
        )
        .await
        .expect("continue the transaction and commit DDL after savepoint rollback");
        let after_commit = read_committed_state(&client).await;

        // Assert
        assert_savepoint_rollback_state(&identity_before, &after_rollback);
        assert_committed_state(&identity_before, &after_commit);
        drop(client);
        server.shutdown(connection).await;
    });
}
