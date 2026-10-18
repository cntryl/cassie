use super::*;

struct GrantCascadeObservations {
    owner: String,
    restrict_error: tokio_postgres::Error,
    after_restrict: tokio_postgres::Row,
    after_restrict_catalog: Vec<tokio_postgres::Row>,
    after_cascade: tokio_postgres::Row,
    after_cascade_catalog: Vec<tokio_postgres::Row>,
}

async fn read_grant_effective_state(client: &tokio_postgres::Client) -> tokio_postgres::Row {
    client
        .query_one(
            "SELECT has_table_privilege( \
                      'compat_grant_option_giver', \
                      'public.compat_grant_option_rows', 'SELECT'), \
                    has_table_privilege( \
                      'compat_grant_option_reader', \
                      'public.compat_grant_option_rows', 'SELECT')",
            &[],
        )
        .await
        .expect("inspect the effective privilege chain")
}

async fn read_grant_catalog_state(client: &tokio_postgres::Client) -> Vec<tokio_postgres::Row> {
    client
        .query(
            "SELECT grantee, grantor, privilege_type, is_grantable, with_hierarchy \
             FROM information_schema.table_privileges \
             WHERE table_schema = 'public' \
               AND table_name = 'compat_grant_option_rows' \
               AND grantee IN ( \
                 'compat_grant_option_giver', 'compat_grant_option_reader' \
               ) \
             ORDER BY grantee, privilege_type",
            &[],
        )
        .await
        .expect("inspect grant-option metadata")
}

async fn observe_grant_option_cascade(client: &tokio_postgres::Client) -> GrantCascadeObservations {
    let owner: String = client
        .query_one("SELECT current_user", &[])
        .await
        .expect("read the grantor identity")
        .try_get(0)
        .expect("current role name");
    client
        .batch_execute(
            "GRANT SELECT ON TABLE public.compat_grant_option_rows \
             TO compat_grant_option_giver WITH GRANT OPTION",
        )
        .await
        .expect("grant SELECT with grant option to the intermediate role");
    client
        .batch_execute("SET ROLE compat_grant_option_giver")
        .await
        .expect("assume the intermediate grantor role");
    client
        .batch_execute(
            "GRANT SELECT ON TABLE public.compat_grant_option_rows \
             TO compat_grant_option_reader",
        )
        .await
        .expect("let the intermediate role grant SELECT to a dependent reader");
    client
        .batch_execute("RESET ROLE")
        .await
        .expect("restore the owning role before revocation");
    let restrict_error = client
        .batch_execute(
            "REVOKE GRANT OPTION FOR SELECT ON TABLE public.compat_grant_option_rows \
             FROM compat_grant_option_giver RESTRICT",
        )
        .await
        .expect_err("RESTRICT must reject revoking an option with a dependent grant");
    let after_restrict = read_grant_effective_state(client).await;
    let after_restrict_catalog = read_grant_catalog_state(client).await;
    client
        .batch_execute(
            "REVOKE GRANT OPTION FOR SELECT ON TABLE public.compat_grant_option_rows \
             FROM compat_grant_option_giver CASCADE",
        )
        .await
        .expect("cascade revocation through the dependent table grant");
    let after_cascade = read_grant_effective_state(client).await;
    let after_cascade_catalog = read_grant_catalog_state(client).await;
    GrantCascadeObservations {
        owner,
        restrict_error,
        after_restrict,
        after_restrict_catalog,
        after_cascade,
        after_cascade_catalog,
    }
}

fn grant_catalog_rows(
    rows: &[tokio_postgres::Row],
) -> Vec<(String, String, String, String, String)> {
    rows.iter()
        .map(|row| {
            (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, String>(3),
                row.get::<_, String>(4),
            )
        })
        .collect()
}

fn assert_grant_option_cascade(observations: &GrantCascadeObservations) {
    assert_eq!(
        db_error(&observations.restrict_error).code().code(),
        "2BP01"
    );
    assert_eq!(
        (
            observations.after_restrict.get::<_, bool>(0),
            observations.after_restrict.get::<_, bool>(1),
        ),
        (true, true),
        "RESTRICT must leave the privilege chain unchanged",
    );
    assert_eq!(
        grant_catalog_rows(&observations.after_restrict_catalog),
        vec![
            (
                "compat_grant_option_giver".to_owned(),
                observations.owner.clone(),
                "SELECT".to_owned(),
                "YES".to_owned(),
                "YES".to_owned(),
            ),
            (
                "compat_grant_option_reader".to_owned(),
                "compat_grant_option_giver".to_owned(),
                "SELECT".to_owned(),
                "NO".to_owned(),
                "YES".to_owned(),
            ),
        ]
    );
    assert_eq!(
        (
            observations.after_cascade.get::<_, bool>(0),
            observations.after_cascade.get::<_, bool>(1),
        ),
        (true, false),
        "CASCADE must preserve SELECT for the grantor and remove it from the reader",
    );
    assert_eq!(
        grant_catalog_rows(&observations.after_cascade_catalog),
        vec![(
            "compat_grant_option_giver".to_owned(),
            observations.owner.clone(),
            "SELECT".to_owned(),
            "NO".to_owned(),
            "YES".to_owned(),
        )]
    );
}

#[test]
fn should_cascade_dependent_grants_when_revoking_grant_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_grant_option_cascade").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_grant_option_giver NOLOGIN",
                "CREATE ROLE compat_grant_option_reader NOLOGIN",
                "CREATE TABLE public.compat_grant_option_rows \
                   (id INTEGER PRIMARY KEY, label TEXT)",
            ],
        )
        .await
        .expect("create the table and roles for a dependent grant chain");
        // Act
        let observations = observe_grant_option_cascade(&client).await;

        // Assert
        assert_grant_option_cascade(&observations);

        drop(client);
        server.shutdown(connection).await;
    });
}
