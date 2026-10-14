use super::*;

#[test]
fn should_publish_created_relation_to_observers_only_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_create_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin relation-creation transaction");

        // Act
        owner
            .batch_execute("CREATE TABLE compat_transactional_visibility (id INT PRIMARY KEY)")
            .await
            .expect("create relation inside the transaction");
        let owner_relation: Option<String> = owner
            .query_one(
                "SELECT to_regclass('public.compat_transactional_visibility')::TEXT",
                &[],
            )
            .await
            .expect("read the relation in its creating session")
            .get(0);
        let observer_before_commit: Option<String> = observer
            .query_one(
                "SELECT to_regclass('public.compat_transactional_visibility')::TEXT",
                &[],
            )
            .await
            .expect("read catalog state from another session before commit")
            .get(0);
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit relation creation");
        let observer_after_commit: Option<String> = observer
            .query_one(
                "SELECT to_regclass('public.compat_transactional_visibility')::TEXT",
                &[],
            )
            .await
            .expect("read catalog state from another session after commit")
            .get(0);

        // Assert
        assert_eq!(
            owner_relation.as_deref(),
            Some("compat_transactional_visibility")
        );
        assert_eq!(observer_before_commit, None);
        assert_eq!(
            observer_after_commit.as_deref(),
            Some("compat_transactional_visibility")
        );

        drop(owner);
        drop(observer);
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_hide_rolled_back_column_changes_from_other_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_transactional_alter (id INT PRIMARY KEY)")
            .await
            .expect("create the relation before the DDL transaction");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin column-change transaction");

        // Act
        owner
            .batch_execute("ALTER TABLE compat_transactional_alter ADD COLUMN label TEXT")
            .await
            .expect("add a column inside the transaction");
        let owner_columns: i64 = owner
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name = 'compat_transactional_alter'",
                &[],
            )
            .await
            .expect("read columns from the altering session")
            .get(0);
        let observer_columns_before_rollback: i64 = observer
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name = 'compat_transactional_alter'",
                &[],
            )
            .await
            .expect("read columns from another session before rollback")
            .get(0);
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the column change");
        let observer_columns_after_rollback: i64 = observer
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_name = 'compat_transactional_alter'",
                &[],
            )
            .await
            .expect("verify rolled-back columns from another session")
            .get(0);

        // Assert
        assert_eq!(owner_columns, 2);
        assert_eq!(observer_columns_before_rollback, 1);
        assert_eq!(observer_columns_after_rollback, 1);

        drop(owner);
        drop(observer);
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
