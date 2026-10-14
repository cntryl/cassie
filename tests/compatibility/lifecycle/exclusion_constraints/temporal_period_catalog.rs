use super::*;

#[test]
fn should_report_false_period_metadata_for_regular_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_period_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_period_metadata_parent (\
                   id INT CONSTRAINT compat_period_metadata_pk PRIMARY KEY)",
                "CREATE TABLE compat_period_metadata_child (\
                   parent_id INT CONSTRAINT compat_period_metadata_fk \
                   REFERENCES compat_period_metadata_parent(id))",
            ],
        )
        .await
        .expect("create ordinary primary-key and foreign-key metadata fixtures");

        // Act
        let constraints = client
            .query(
                "SELECT conname, contype::TEXT, conperiod, convalidated \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname IN (\
                   'compat_period_metadata_fk', 'compat_period_metadata_pk') \
                 ORDER BY conname",
                &[],
            )
            .await
            .expect("read PostgreSQL 18 temporal-period catalog metadata");

        // Assert
        assert_eq!(constraints.len(), 2);
        let period_column = &constraints[0].columns()[2];
        assert_eq!(period_column.name(), "conperiod");
        assert_eq!(period_column.type_().oid(), 16);

        assert_eq!(
            constraints[0].get::<_, String>(0),
            "compat_period_metadata_fk"
        );
        assert_eq!(constraints[0].get::<_, String>(1), "f");
        assert!(!constraints[0].get::<_, bool>(2));
        assert!(constraints[0].get::<_, bool>(3));
        assert_eq!(
            constraints[1].get::<_, String>(0),
            "compat_period_metadata_pk"
        );
        assert_eq!(constraints[1].get::<_, String>(1), "p");
        assert!(!constraints[1].get::<_, bool>(2));
        assert!(constraints[1].get::<_, bool>(3));

        drop(client);
        server.shutdown(connection).await;
    });
}
