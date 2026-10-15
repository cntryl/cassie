use super::*;

#[test]
fn should_apply_force_row_security_to_non_superuser_table_owner() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("force_rls_owner_enforcement").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_force_rls_owner NOLOGIN",
                "CREATE TABLE compat_force_owner_rows \
                   (id INT PRIMARY KEY, owner_name TEXT NOT NULL)",
                "INSERT INTO compat_force_owner_rows VALUES \
                   (1, 'compat_force_rls_owner'), (2, 'another_owner')",
                "ALTER TABLE compat_force_owner_rows OWNER TO compat_force_rls_owner",
                "ALTER TABLE compat_force_owner_rows ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY compat_force_owner_policy \
                   ON compat_force_owner_rows FOR SELECT \
                   USING (owner_name = current_user)",
                "SET ROLE compat_force_rls_owner",
            ],
        )
        .await
        .expect("create an RLS table, policy, and non-superuser owner session");

        // Act
        let owner_bypass = client
            .query("SELECT id FROM compat_force_owner_rows ORDER BY id", &[])
            .await
            .expect("table owner bypasses row security before FORCE");
        client
            .batch_execute("ALTER TABLE compat_force_owner_rows FORCE ROW LEVEL SECURITY")
            .await
            .expect("force row security for the table owner");
        let owner_filtered = client
            .query("SELECT id FROM compat_force_owner_rows ORDER BY id", &[])
            .await
            .expect("table owner is subject to the policy after FORCE");
        client
            .batch_execute("ALTER TABLE compat_force_owner_rows NO FORCE ROW LEVEL SECURITY")
            .await
            .expect("remove forced row security for the table owner");
        let owner_bypass_restored = client
            .query("SELECT id FROM compat_force_owner_rows ORDER BY id", &[])
            .await
            .expect("table owner bypasses row security again after NO FORCE");

        // Assert
        let ids = |rows: &[tokio_postgres::Row]| {
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&owner_bypass), vec![1, 2]);
        assert_eq!(ids(&owner_filtered), vec![1]);
        assert_eq!(ids(&owner_bypass_restored), vec![1, 2]);

        drop(client);
        server.shutdown(connection).await;
    });
}
