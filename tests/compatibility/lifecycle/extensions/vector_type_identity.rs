use super::*;

#[test]
fn should_infer_bare_vector_dimensions_and_preserve_fixed_typmod() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vector_bare_dimensions").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE EXTENSION vector VERSION '0.8.7'; \
                 CREATE TABLE vector_bare_values (fixed vector(3), inferred vector)",
            )
            .await
            .expect("install vector and create fixed/bare columns");

        // Act
        client
            .batch_execute(
                "INSERT INTO vector_bare_values VALUES \
                 ('[1,2,3]'::vector, '[4,5]'::vector)",
            )
            .await
            .expect("infer each bare vector value dimension");
        let values = client
            .query("SELECT fixed, inferred FROM vector_bare_values", &[])
            .await
            .expect("read fixed and inferred vector values");

        // Assert
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].get::<_, String>(0), "[1,2,3]");
        assert_eq!(values[0].get::<_, String>(1), "[4,5]");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_one_canonical_vector_oid_with_declared_and_bare_typmods() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vector_type_identity").await;
        let (client, connection) = server.connect().await;
        let absent_types = client
            .query(
                "SELECT oid FROM pg_catalog.pg_type WHERE typname = 'vector'",
                &[],
            )
            .await
            .expect("vector type is absent before extension installation");
        let uninstalled_type = client
            .batch_execute("CREATE TABLE vector_types_before_install (value vector(3))")
            .await
            .expect_err("vector SQL types require the installed extension");
        client
            .batch_execute(
                "CREATE EXTENSION vector WITH SCHEMA public VERSION '0.8.7'; \
                 CREATE TABLE vector_types (fixed vector(3), inferred vector)",
            )
            .await
            .expect("install vector and create fixed/bare vector columns");

        // Act
        let columns = client
            .query(
                "SELECT attname, atttypid, atttypmod \
                 , pg_catalog.format_type(atttypid, atttypmod) \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'vector_types'::regclass AND attnum > 0 \
                 ORDER BY attnum",
                &[],
            )
            .await
            .expect("vector column identities");
        let types = client
            .query(
                "SELECT oid FROM pg_catalog.pg_type WHERE typname = 'vector'",
                &[],
            )
            .await
            .expect("single canonical vector type row");

        // Assert
        assert!(absent_types.is_empty());
        assert_eq!(db_error(&uninstalled_type).code().code(), "42704");
        assert_eq!(columns.len(), 2);
        assert_eq!(types.len(), 1);
        assert_eq!(columns[0].get::<_, String>(0), "fixed");
        assert_eq!(columns[1].get::<_, String>(0), "inferred");
        let vector_oid = types[0].get::<_, i64>(0);
        assert!(vector_oid >= 16_384);
        assert!(!(100_001..=132_767).contains(&vector_oid));
        assert_eq!(columns[0].get::<_, i64>(1), vector_oid);
        assert_eq!(columns[1].get::<_, i64>(1), vector_oid);
        assert_eq!(columns[0].get::<_, i32>(2), 3);
        assert_eq!(columns[1].get::<_, i32>(2), -1);
        assert_eq!(columns[0].get::<_, String>(3), "vector(3)");
        assert_eq!(columns[1].get::<_, String>(3), "vector");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_describe_sql_prepared_vector_parameters_with_the_active_database_oid() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_vector_parameter_identity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION vector VERSION '0.8.7'")
            .await
            .expect("install vector in the active database");
        let vector_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_type WHERE typname = 'vector'",
                &[],
            )
            .await
            .expect("read the installed vector identity")
            .get::<_, i64>(0);
        client
            .batch_execute("PREPARE prepared_vector_identity(vector) AS SELECT $1")
            .await
            .expect("prepare a statement with a declared vector parameter");

        // Act
        let result = client
            .query_one("EXECUTE prepared_vector_identity('[1,2]'::vector)", &[])
            .await
            .expect("execute the SQL-prepared vector statement");

        // Assert
        assert_eq!(
            i64::from(result.columns()[0].type_().oid()),
            vector_oid,
            "PREPARE result descriptors use the active database's vector OID"
        );
        assert_eq!(result.get::<_, String>(0), "[1,2]");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_pgvector_catalog_identities_after_local_restart() {
    // Arrange
    let path = support::data_dir("vector_catalog_identities_restart");
    let _directory = DataDirectory(path.clone());
    let before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE EXTENSION vector VERSION '0.8.7'",
                Vec::new(),
            )
            .expect("persist pgvector catalog identities");
        let identities = vector_catalog_identity_snapshot(&cassie, &session);
        drop(session);
        cassie.shutdown();
        identities
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie");
    reopened.startup().expect("restore pgvector metadata");
    let session = reopened.create_session("root", None);
    let after_restart = vector_catalog_identity_snapshot(&reopened, &session);

    // Assert
    assert_eq!(after_restart, before_restart);
    assert_eq!(after_restart[0].len(), 1, "one installed vector type");
    assert_eq!(after_restart[1].len(), 2, "two selected vector methods");
    assert_eq!(
        after_restart[2].len(),
        3,
        "three selected distance operators"
    );
    assert_eq!(after_restart[3].len(), 6, "six selected operator families");
    assert_eq!(after_restart[4].len(), 6, "six selected operator classes");
    assert_eq!(after_restart[5].len(), 6, "six selected ordering members");
    drop(session);
    reopened.shutdown();
}

fn vector_catalog_identity_snapshot(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
) -> Vec<Vec<Vec<cassie::types::Value>>> {
    [
        "SELECT oid::BIGINT FROM pg_catalog.pg_type WHERE typname = 'vector'",
        "SELECT oid::BIGINT, amname FROM pg_catalog.pg_am \
         WHERE amname IN ('hnsw', 'ivfflat') ORDER BY amname",
        "SELECT oid::BIGINT, oprname FROM pg_catalog.pg_operator \
         WHERE oprname IN ('<->', '<#>', '<=>') ORDER BY oprname",
        "SELECT opf.oid::BIGINT, am.amname, opf.opfname \
         FROM pg_catalog.pg_opfamily AS opf \
         JOIN pg_catalog.pg_am AS am ON am.oid = opf.opfmethod \
         WHERE am.amname IN ('hnsw', 'ivfflat') ORDER BY am.amname, opf.opfname",
        "SELECT opc.oid::BIGINT, am.amname, opc.opcname, opc.opcfamily::BIGINT \
         FROM pg_catalog.pg_opclass AS opc \
         JOIN pg_catalog.pg_am AS am ON am.oid = opc.opcmethod \
         WHERE am.amname IN ('hnsw', 'ivfflat') ORDER BY am.amname, opc.opcname",
        "SELECT amopfamily::BIGINT, amoplefttype::BIGINT, amoprighttype::BIGINT, \
         amopstrategy::INTEGER, amoppurpose, amopopr::BIGINT, amopmethod::BIGINT, \
         amopsortfamily::BIGINT FROM pg_catalog.pg_amop \
         WHERE amopmethod IN (SELECT oid FROM pg_catalog.pg_am \
         WHERE amname IN ('hnsw', 'ivfflat')) \
         ORDER BY amopmethod, amopfamily, amopopr",
    ]
    .into_iter()
    .map(|sql| {
        cassie
            .execute_sql(session, sql, Vec::new())
            .unwrap_or_else(|error| panic!("read persisted pgvector catalog identity: {error}"))
            .rows()
            .to_vec()
    })
    .collect()
}
