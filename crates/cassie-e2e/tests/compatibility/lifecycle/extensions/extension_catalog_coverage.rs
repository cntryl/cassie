use super::*;

#[test]
fn should_update_only_the_selected_available_extension_versions_as_installation_changes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("available_extension_versions").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install pgcrypto");
        let after_install = client
            .query(
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions ORDER BY name",
                &[],
            )
            .await
            .expect("selected available extension rows after install");
        client
            .batch_execute("DROP EXTENSION pgcrypto")
            .await
            .expect("remove pgcrypto");
        let after_drop = client
            .query(
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions ORDER BY name",
                &[],
            )
            .await
            .expect("selected available extension rows after drop");

        // Assert
        assert_eq!(after_install.len(), 2);
        assert_eq!(after_install[0].get::<_, String>(0), "pgcrypto");
        assert_eq!(after_install[0].get::<_, String>(1), "1.4");
        assert_eq!(
            after_install[0].get::<_, Option<String>>(2),
            Some("1.4".to_owned())
        );
        assert_eq!(after_install[1].get::<_, String>(0), "vector");
        assert_eq!(after_install[1].get::<_, String>(1), "0.8.7");
        assert_eq!(after_install[1].get::<_, Option<String>>(2), None);
        assert_eq!(after_drop[0].get::<_, Option<String>>(2), None);
        assert_eq!(after_drop[1].get::<_, Option<String>>(2), None);

        drop(client);
        server.shutdown(connection).await;
    });
}
