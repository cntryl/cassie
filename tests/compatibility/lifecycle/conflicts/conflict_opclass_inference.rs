use super::*;

#[test]
fn should_infer_named_operator_class_for_conflict_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_operator_class_inference").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_opclass (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_conflict_opclass VALUES (1, 'old')",
            ],
        )
        .await
        .expect("create a default integer B-tree key and seed the conflicting row");

        // Act
        let upsert = client
            .query(
                "INSERT INTO compat_conflict_opclass VALUES (1, 'new') \
                 ON CONFLICT (id int4_ops) DO UPDATE SET label = EXCLUDED.label \
                 RETURNING id, label",
                &[],
            )
            .await;
        let returned = upsert.as_ref().ok().and_then(|rows| {
            rows.first().map(|row| {
                (
                    row.columns()
                        .iter()
                        .map(|column| column.type_().oid())
                        .collect::<Vec<_>>(),
                    row.get::<_, i32>(0),
                    row.get::<_, String>(1),
                )
            })
        });
        let stored = client
            .query_one(
                "SELECT id, label FROM compat_conflict_opclass WHERE id = 1",
                &[],
            )
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;

        assert!(upsert.is_ok(), "opclass conflict inference: {upsert:?}");
        assert_eq!(
            returned.expect("the conflict update returns one target row"),
            (vec![23, 25], 1, "new".to_string())
        );
        let stored = stored.expect("read the persisted conflict-update value");
        assert_eq!(stored.get::<_, i32>(0), 1);
        assert_eq!(stored.get::<_, String>(1), "new");
    });
}
