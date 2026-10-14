use super::*;

#[test]
fn should_retain_function_after_extension_membership_changes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_member_membership").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE FUNCTION public.compat_extension_member(input integer) \
                   RETURNS integer LANGUAGE SQL IMMUTABLE AS $$ SELECT input + 1 $$",
            ],
        )
        .await
        .expect("install pgcrypto and create an independent SQL function");
        let membership_before: i64 = client
            .query_one(
                "SELECT count(*) \
                 FROM pg_catalog.pg_depend AS dependency \
                 JOIN pg_catalog.pg_extension AS extension \
                   ON extension.oid = dependency.refobjid \
                 WHERE extension.extname = 'pgcrypto' \
                   AND dependency.classid = 'pg_catalog.pg_proc'::regclass \
                   AND dependency.objid = \
                     'public.compat_extension_member(integer)'::regprocedure \
                   AND dependency.refclassid = 'pg_catalog.pg_extension'::regclass \
                   AND dependency.deptype = 'e'",
                &[],
            )
            .await
            .expect("check that the function is not yet an extension member")
            .try_get(0)
            .expect("extension membership count");

        // Act
        client
            .batch_execute(
                "ALTER EXTENSION pgcrypto \
                 ADD FUNCTION public.compat_extension_member(integer)",
            )
            .await
            .expect("add the SQL function to pgcrypto");
        let membership_after_add: i64 = client
            .query_one(
                "SELECT count(*) \
                 FROM pg_catalog.pg_depend AS dependency \
                 JOIN pg_catalog.pg_extension AS extension \
                   ON extension.oid = dependency.refobjid \
                 WHERE extension.extname = 'pgcrypto' \
                   AND dependency.classid = 'pg_catalog.pg_proc'::regclass \
                   AND dependency.objid = \
                     'public.compat_extension_member(integer)'::regprocedure \
                   AND dependency.refclassid = 'pg_catalog.pg_extension'::regclass \
                   AND dependency.deptype = 'e'",
                &[],
            )
            .await
            .expect("read extension membership after ALTER EXTENSION ADD")
            .try_get(0)
            .expect("extension membership count");
        let result: i32 = client
            .query_one("SELECT public.compat_extension_member(41)", &[])
            .await
            .expect("call the function while it belongs to pgcrypto")
            .try_get(0)
            .expect("function result");
        client
            .batch_execute(
                "ALTER EXTENSION pgcrypto \
                 DROP FUNCTION public.compat_extension_member(integer)",
            )
            .await
            .expect("remove the function from pgcrypto without dropping it");
        let membership_after_drop: i64 = client
            .query_one(
                "SELECT count(*) \
                 FROM pg_catalog.pg_depend AS dependency \
                 JOIN pg_catalog.pg_extension AS extension \
                   ON extension.oid = dependency.refobjid \
                 WHERE extension.extname = 'pgcrypto' \
                   AND dependency.classid = 'pg_catalog.pg_proc'::regclass \
                   AND dependency.objid = \
                     'public.compat_extension_member(integer)'::regprocedure \
                   AND dependency.refclassid = 'pg_catalog.pg_extension'::regclass \
                   AND dependency.deptype = 'e'",
                &[],
            )
            .await
            .expect("read extension membership after ALTER EXTENSION DROP")
            .try_get(0)
            .expect("extension membership count");
        let function_after_drop = client
            .query_one(
                "SELECT to_regprocedure(\
                   'public.compat_extension_member(integer)') IS NOT NULL, \
                 public.compat_extension_member(41)",
                &[],
            )
            .await
            .expect("confirm the standalone function remains callable after membership removal");

        // Assert
        assert_eq!(membership_before, 0);
        assert_eq!(membership_after_add, 1);
        assert_eq!(result, 42);
        assert_eq!(membership_after_drop, 0);
        assert!(function_after_drop.get::<_, bool>(0));
        assert_eq!(function_after_drop.get::<_, i32>(1), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
