use super::*;

const VECTOR_METHOD_COUNT_SQL: &str =
    "SELECT count(*) FROM pg_catalog.pg_am WHERE amname IN ('hnsw', 'ivfflat')";

#[test]
fn should_bind_pgvector_methods_and_return_operator_ordered_vector_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_methods").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector WITH SCHEMA public VERSION '0.8.7'",
                "CREATE TABLE pgvector_hnsw_rows (id INT, embedding VECTOR(3))",
                "INSERT INTO pgvector_hnsw_rows VALUES \
                 (1, '[1,0,0]'::vector), (2, '[0,1,0]'::vector), (3, '[-1,0,0]'::vector)",
                "CREATE INDEX pgvector_hnsw_rows_idx ON pgvector_hnsw_rows \
                 USING hnsw (embedding vector_l2_ops) WITH (m = 4, ef_construction = 16)",
            ],
        )
        .await
        .expect("create vectors and an HNSW index through the pgvector SQL profile");

        // Act
        let access_method: String = client
            .query_one(
                "SELECT am.amname FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_am AS am ON am.oid = c.relam \
                 WHERE c.relname = 'pgvector_hnsw_rows_idx'",
                &[],
            )
            .await
            .expect("inspect the HNSW index access method identity")
            .get(0);
        let rows = client
            .query(
                "SELECT id, embedding <-> '[1,0,0]'::vector AS distance \
                 FROM pgvector_hnsw_rows \
                 ORDER BY embedding <-> '[1,0,0]'::vector LIMIT 3",
                &[],
            )
            .await
            .expect("use pgvector L2 operator and ordering semantics");
        let negative_inner_product = client
            .query_one(
                "SELECT embedding <#> '[1,0,0]'::vector \
                 FROM pgvector_hnsw_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("use pgvector negative inner product");
        execute_statements(
            &client,
            &[
                "DROP INDEX pgvector_hnsw_rows_idx",
                "CREATE INDEX pgvector_ivfflat_rows_idx ON pgvector_hnsw_rows \
                 USING ivfflat (embedding vector_ip_ops) WITH (lists = 1)",
            ],
        )
        .await
        .expect("replace the HNSW method with an inner-product IVFFlat index");
        let ivfflat_rows = client
            .query(
                "SELECT id FROM pgvector_hnsw_rows \
                 ORDER BY embedding <#> '[1,0,0]'::vector LIMIT 3",
                &[],
            )
            .await
            .expect("use IVFFlat inner-product ordering");
        execute_statements(
            &client,
            &[
                "DROP INDEX pgvector_ivfflat_rows_idx",
                "CREATE INDEX pgvector_ivfflat_default_rows_idx \
                 ON pgvector_hnsw_rows USING ivfflat (embedding) WITH (lists = 1)",
            ],
        )
        .await
        .expect("replace the explicit IVFFlat opclass with pgvector's default L2 class");
        let default_index_definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'pgvector_ivfflat_default_rows_idx'",
                &[],
            )
            .await
            .expect("read the normalized default operator class")
            .get(0);

        // Assert
        assert_eq!(
            access_method, "hnsw",
            "pg_class relam resolves to the installed HNSW method"
        );
        assert_eq!(row_ids(&rows), vec![1, 2, 3]);
        assert_eq!(rows[0].get::<_, f64>(1), 0.0);
        assert!((rows[1].get::<_, f64>(1) - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(rows[2].get::<_, f64>(1), 2.0);
        assert_eq!(negative_inner_product.get::<_, f64>(0), -1.0);
        assert_eq!(row_ids(&ivfflat_rows), vec![1, 2, 3]);
        assert!(default_index_definition.contains("vector_l2_ops"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_match_exact_l2_top_k_through_pgvector_hnsw_and_default_ivfflat() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_l2_ann_exact_equivalence").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE EXTENSION vector WITH SCHEMA public VERSION '0.8.7'; \
                 CREATE TABLE pgvector_l2_equivalence (id INT, embedding VECTOR(3)); \
                 INSERT INTO pgvector_l2_equivalence VALUES \
                 (1, '[1,0,0]'::vector), (2, '[0,2,0]'::vector), \
                 (3, '[0,0,3]'::vector), (4, '[-4,0,0]'::vector)",
            )
            .await
            .expect("create a small deterministic vector fixture");

        let before_exact = server.cassie.metrics();
        let exact_rows = l2_top_k_rows(&client).await;
        let after_exact = server.cassie.metrics();

        // Act
        client
            .batch_execute(
                "CREATE INDEX pgvector_l2_hnsw_equivalence \
                 ON pgvector_l2_equivalence USING hnsw (embedding vector_l2_ops) \
                 WITH (m = 4, ef_construction = 16)",
            )
            .await
            .expect("create the supported HNSW L2 path");
        let before_hnsw = server.cassie.metrics();
        let hnsw_rows = l2_top_k_rows(&client).await;
        let after_hnsw = server.cassie.metrics();

        client
            .batch_execute("DROP INDEX pgvector_l2_hnsw_equivalence")
            .await
            .expect("remove HNSW before creating the IVFFlat index");
        client
            .batch_execute(
                "CREATE INDEX pgvector_l2_ivfflat_equivalence \
                 ON pgvector_l2_equivalence USING ivfflat (embedding) WITH (lists = 1)",
            )
            .await
            .expect("create the default IVFFlat L2 path");
        let before_ivfflat = server.cassie.metrics();
        let ivfflat_rows = l2_top_k_rows(&client).await;
        let after_ivfflat = server.cassie.metrics();

        // Assert
        let expected_rows = vec![(1, 1.0), (2, 2.0), (3, 3.0)];
        assert_eq!(exact_rows, expected_rows);
        assert_eq!(
            after_exact["vector"]["hnsw_executions"].as_u64().unwrap(),
            before_exact["vector"]["hnsw_executions"].as_u64().unwrap(),
            "the baseline result must be produced without an HNSW index"
        );
        assert_eq!(
            after_exact["vector"]["ivfflat_executions"]
                .as_u64()
                .unwrap(),
            before_exact["vector"]["ivfflat_executions"]
                .as_u64()
                .unwrap(),
            "the baseline result must be produced without an IVFFlat index"
        );
        assert_eq!(hnsw_rows, exact_rows);
        assert_eq!(
            after_hnsw["vector"]["hnsw_executions"].as_u64().unwrap()
                - before_hnsw["vector"]["hnsw_executions"].as_u64().unwrap(),
            1,
            "the HNSW result must come from the ANN execution path"
        );
        assert_eq!(ivfflat_rows, exact_rows);
        assert_eq!(
            after_ivfflat["vector"]["ivfflat_executions"]
                .as_u64()
                .unwrap()
                - before_ivfflat["vector"]["ivfflat_executions"]
                    .as_u64()
                    .unwrap(),
            1,
            "the default IVFFlat result must come from the ANN execution path"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn l2_top_k_rows(client: &tokio_postgres::Client) -> Vec<(i32, f64)> {
    client
        .query(
            "SELECT id, embedding <-> '[0,0,0]'::vector AS distance \
             FROM pgvector_l2_equivalence \
             ORDER BY embedding <-> '[0,0,0]'::vector LIMIT 3",
            &[],
        )
        .await
        .expect("query L2 top-k rows")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

#[test]
fn should_resolve_pgvector_operator_classes_in_the_extension_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_extension_schema").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA vector_ext",
                "CREATE EXTENSION vector WITH SCHEMA vector_ext VERSION '0.8.7'",
                "CREATE TABLE pgvector_extension_schema_rows (embedding vector(3))",
                "SET search_path TO public",
            ],
        )
        .await
        .expect("install vector into a non-default schema");

        // Act
        let unqualified_outside_path = client
            .batch_execute(
                "CREATE INDEX pgvector_hidden_class_idx \
                 ON pgvector_extension_schema_rows USING hnsw (embedding vector_l2_ops)",
            )
            .await
            .expect_err("unqualified operator classes must resolve through search_path");
        let differently_cased_quoted_schema = client
            .batch_execute(
                "CREATE INDEX pgvector_quoted_wrong_schema_idx \
                 ON pgvector_extension_schema_rows \
                 USING hnsw (embedding \"VECTOR_EXT\".vector_l2_ops)",
            )
            .await
            .expect_err("quoted schema names preserve case during operator-class lookup");
        client
            .batch_execute(
                "CREATE INDEX pgvector_qualified_class_idx \
                 ON pgvector_extension_schema_rows \
                 USING hnsw (embedding vector_ext.vector_l2_ops)",
            )
            .await
            .expect("an explicit extension schema resolves outside search_path");
        client
            .batch_execute("DROP INDEX pgvector_qualified_class_idx")
            .await
            .expect("remove the qualified-class probe before the search-path case");
        client
            .batch_execute("SET search_path TO vector_ext, public")
            .await
            .expect("place the extension schema on search_path");
        let differently_cased_quoted_class = client
            .batch_execute(
                "CREATE INDEX pgvector_quoted_wrong_class_idx \
                 ON pgvector_extension_schema_rows \
                 USING hnsw (embedding \"VECTOR_L2_OPS\")",
            )
            .await
            .expect_err("quoted operator-class names preserve case");
        client
            .batch_execute(
                "CREATE INDEX pgvector_visible_class_idx \
                 ON pgvector_extension_schema_rows USING hnsw (embedding vector_ip_ops)",
            )
            .await
            .expect("an unqualified operator class resolves when its schema is visible");
        let selected_schema: String = client
            .query_one(
                "SELECT n.nspname FROM pg_catalog.pg_opclass AS opc \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = opc.opcnamespace \
                 JOIN pg_catalog.pg_am AS am ON am.oid = opc.opcmethod \
                 WHERE opc.opcname = 'vector_ip_ops' AND am.amname = 'hnsw'",
                &[],
            )
            .await
            .expect("inspect the installed operator class namespace")
            .get(0);

        // Assert
        assert!(db_error(&unqualified_outside_path)
            .message()
            .contains("operator class"));
        assert!(db_error(&differently_cased_quoted_schema)
            .message()
            .contains("operator class"));
        assert!(db_error(&differently_cased_quoted_class)
            .message()
            .contains("operator class"));
        assert_eq!(selected_schema, "vector_ext");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_vector_dependents_when_extension_drop_is_rejected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_extension_drop_dependencies").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector VERSION '0.8.7'",
                "CREATE TABLE pgvector_drop_dependencies (id INT, embedding vector(3))",
                "CREATE INDEX pgvector_drop_dependencies_idx \
                 ON pgvector_drop_dependencies USING hnsw (embedding vector_l2_ops)",
                "CREATE VIEW pgvector_drop_cascade_view AS \
                 SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector AS distance",
            ],
        )
        .await
        .expect("create vector type and an index owned by the extension profile");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect_err("RESTRICT must not remove catalog identities used by a vector index");
        let cascade_error = client
            .batch_execute("DROP EXTENSION vector CASCADE")
            .await
            .expect_err("unsupported vector CASCADE must fail before changing dependent objects");
        let access_methods = query_i64(
            &client,
            VECTOR_METHOD_COUNT_SQL,
            "inspect vector methods after rejected extension drops",
        )
        .await;
        let indexes = query_i64(
            &client,
            "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'pgvector_drop_dependencies_idx'",
            "inspect the retained dependent index",
        )
        .await;
        let extension_count = query_i64(
            &client,
            "SELECT count(*) FROM pg_catalog.pg_extension WHERE extname = 'vector'",
            "inspect the retained vector extension",
        )
        .await;
        let vector_type_count = query_i64(
            &client,
            "SELECT count(*) FROM pg_catalog.pg_type WHERE typname = 'vector'",
            "inspect the retained vector type",
        )
        .await;
        let retained_distance: f64 = client
            .query_one("SELECT distance FROM pgvector_drop_cascade_view", &[])
            .await
            .expect("a rejected cascade must preserve regular views it could otherwise remove")
            .get(0);
        let index_identity = client
            .query_one(
                "SELECT c.oid::bigint, c.relam::bigint, am.oid::bigint, \
                        to_regclass('pgvector_drop_dependencies_idx')::oid::bigint \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_am AS am ON am.oid = c.relam \
                 WHERE c.relname = 'pgvector_drop_dependencies_idx'",
                &[],
            )
            .await
            .expect("the retained ANN index keeps one published method identity");

        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert_eq!(db_error(&cascade_error).code().code(), "0A000");
        assert_eq!(access_methods, 2);
        assert_eq!(indexes, 1);
        assert_eq!(extension_count, 1);
        assert_eq!(vector_type_count, 1);
        assert!((retained_distance - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(
            index_identity.get::<_, i64>(0),
            index_identity.get::<_, i64>(3)
        );
        assert_eq!(
            index_identity.get::<_, i64>(1),
            index_identity.get::<_, i64>(2)
        );

        execute_statements(
            &client,
            &[
                "DROP VIEW pgvector_drop_cascade_view",
                "DROP INDEX pgvector_drop_dependencies_idx",
                "DROP TABLE pgvector_drop_dependencies",
                "DROP EXTENSION vector",
            ],
        )
        .await
        .expect("remove dependents before dropping the extension");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_track_vector_operator_dependencies_for_stored_views() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_extension_view_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector VERSION '0.8.7'",
                "CREATE VIEW pgvector_operator_view AS \
                 SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector AS distance",
                "CREATE VIEW pgvector_value_view AS \
                 SELECT '[1,0,0]'::vector AS embedding",
            ],
        )
        .await
        .expect("store a view that uses a vector operator but returns FLOAT8");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect_err("RESTRICT must retain views that depend on vector operators");
        let retained_distance: f64 = client
            .query_one("SELECT distance FROM pgvector_operator_view", &[])
            .await
            .expect("read the retained vector-dependent view")
            .get(0);
        let retained_embedding: String = client
            .query_one("SELECT embedding::text FROM pgvector_value_view", &[])
            .await
            .expect("read the retained vector-typed view")
            .get(0);
        client
            .batch_execute("DROP EXTENSION vector CASCADE")
            .await
            .expect("CASCADE atomically removes a dependent view with its extension");
        let remaining_views: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_views \
                 WHERE viewname IN ('pgvector_operator_view', 'pgvector_value_view')",
                &[],
            )
            .await
            .expect("check that the dependent view was removed")
            .get(0);
        let remaining_methods: i64 = client
            .query_one(VECTOR_METHOD_COUNT_SQL, &[])
            .await
            .expect("check that vector identities were removed")
            .get(0);

        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert!((retained_distance - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(retained_embedding, "[1,0,0]");
        assert_eq!(remaining_views, 0);
        assert_eq!(remaining_methods, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_track_vector_dependencies_in_nested_select_shapes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_nested_view_dependencies").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector VERSION '0.8.7'",
                "CREATE VIEW pgvector_ast_cte_view AS \
                 WITH distances AS (SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector AS distance) \
                 SELECT distance FROM distances",
                "CREATE VIEW pgvector_ast_subquery_view AS \
                 SELECT (SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector) AS distance",
                "CREATE VIEW pgvector_ast_values_view AS \
                 SELECT distance FROM (VALUES ('[1,0,0]'::vector <-> '[0,1,0]'::vector)) \
                 AS distances(distance)",
                "CREATE VIEW pgvector_ast_set_view AS \
                 SELECT 0.0::FLOAT8 AS distance UNION ALL \
                 SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector",
                "CREATE VIEW pgvector_ast_view_parent AS \
                 SELECT '[1,0,0]'::vector AS embedding",
                "CREATE VIEW pgvector_ast_view_child AS \
                 WITH inherited AS (SELECT embedding FROM pgvector_ast_view_parent) \
                 SELECT embedding::text AS embedding_text FROM inherited",
            ],
        )
        .await
        .expect("create stored views with vector operators across nested SELECT forms");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect_err("RESTRICT must retain vector dependencies from nested SELECT forms");
        let preserved_views: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_views \
                 WHERE viewname IN ('pgvector_ast_cte_view', 'pgvector_ast_subquery_view', \
                 'pgvector_ast_values_view', 'pgvector_ast_set_view', \
                 'pgvector_ast_view_parent', 'pgvector_ast_view_child')",
                &[],
            )
            .await
            .expect("inspect nested vector-dependent views after RESTRICT")
            .get(0);
        let chained_text: String = client
            .query_one("SELECT embedding_text FROM pgvector_ast_view_child", &[])
            .await
            .expect("read a text-only view derived from vector output")
            .get(0);
        client
            .batch_execute("DROP EXTENSION vector CASCADE")
            .await
            .expect("CASCADE removes nested vector-dependent views atomically");
        let remaining_views: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_views \
                 WHERE viewname IN ('pgvector_ast_cte_view', 'pgvector_ast_subquery_view', \
                 'pgvector_ast_values_view', 'pgvector_ast_set_view', \
                 'pgvector_ast_view_parent', 'pgvector_ast_view_child')",
                &[],
            )
            .await
            .expect("inspect nested views after extension CASCADE")
            .get(0);

        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert_eq!(preserved_views, 6);
        assert_eq!(chained_text, "[1,0,0]");
        assert_eq!(remaining_views, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_vector_materialized_view_when_extension_cascade_is_rejected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_materialized_view_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector VERSION '0.8.7'",
                "CREATE MATERIALIZED VIEW pgvector_materialized_dependency AS \
                 SELECT '[1,0,0]'::vector AS embedding WITH DATA",
            ],
        )
        .await
        .expect("create a vector-typed materialized view");

        // Act
        let restrict_error = client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect_err("RESTRICT must retain vector-dependent materialized views");
        let cascade_error = client
            .batch_execute("DROP EXTENSION vector CASCADE")
            .await
            .expect_err("CASCADE must not orphan materialized-view output state");
        let retained_rows: i64 = client
            .query_one("SELECT count(*) FROM pgvector_materialized_dependency", &[])
            .await
            .expect("read the materialized view after rejected extension drops")
            .get(0);
        let retained_embedding: String = client
            .query_one(
                "SELECT embedding::text FROM pgvector_materialized_dependency",
                &[],
            )
            .await
            .expect("read the retained materialized vector output")
            .get(0);
        let retained_materialized_view: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'pgvector_materialized_dependency'",
                &[],
            )
            .await
            .expect("inspect retained materialized-view metadata")
            .get(0);
        client
            .batch_execute("DROP MATERIALIZED VIEW pgvector_materialized_dependency")
            .await
            .expect("remove the dependent materialized view explicitly");
        client
            .batch_execute("DROP EXTENSION vector CASCADE")
            .await
            .expect("drop vector after its materialized-view output is removed");

        // Assert
        assert_eq!(db_error(&restrict_error).code().code(), "2BP01");
        assert_eq!(db_error(&cascade_error).code().code(), "0A000");
        assert_eq!(retained_rows, 1);
        assert_eq!(retained_embedding, "[1,0,0]");
        assert_eq!(retained_materialized_view, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_vector_view_dependency_after_local_restart() {
    // Arrange
    let path = support::data_dir("vector_view_dependency_restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie");
        cassie.startup().expect("start local Cassie");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE EXTENSION vector VERSION '0.8.7'",
                Vec::new(),
            )
            .expect("install pgvector catalog identities");
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW pgvector_restart_dependency AS \
                 SELECT '[1,0,0]'::vector <-> '[0,1,0]'::vector AS distance",
                Vec::new(),
            )
            .expect("store a vector-dependent view");
        drop(session);
        cassie.shutdown();
    }

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie");
    reopened
        .startup()
        .expect("restore extension and view metadata");
    let session = reopened.create_session("root", None);
    let retained_view = reopened
        .execute_sql(
            &session,
            "SELECT distance FROM pgvector_restart_dependency",
            Vec::new(),
        )
        .expect("execute the restored vector-dependent view")
        .rows()[0][0]
        .clone();
    let restrict_error = reopened
        .execute_sql(&session, "DROP EXTENSION vector RESTRICT", Vec::new())
        .expect_err("restored vector view identity must protect its extension");
    reopened
        .execute_sql(&session, "DROP EXTENSION vector CASCADE", Vec::new())
        .expect("cascade removes the restored dependent view with vector");
    let remaining_views = reopened
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_views \
             WHERE viewname = 'pgvector_restart_dependency'",
            Vec::new(),
        )
        .expect("read restored view catalog after CASCADE")
        .rows()[0][0]
        .clone();

    // Assert
    let cassie::types::Value::Float64(retained_distance) = retained_view else {
        panic!("restored vector distance should return FLOAT8");
    };
    assert!((retained_distance - 2.0_f64.sqrt()).abs() < 1e-12);
    assert!(matches!(
        restrict_error,
        cassie::app::CassieError::Diagnostic(error) if error.sql_state == "2BP01"
    ));
    assert_eq!(remaining_views, cassie::types::Value::Int64(0));
}

#[test]
fn should_publish_only_installed_vector_access_methods_and_operator_classes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_catalog_rows").await;
        let (client, connection) = server.connect().await;
        let absent_methods = query_i64(
            &client,
            VECTOR_METHOD_COUNT_SQL,
            "inspect methods before vector installation",
        )
        .await;
        client
            .batch_execute("CREATE EXTENSION vector VERSION '0.8.7'")
            .await
            .expect("install vector catalog identities");

        // Act
        let methods = client
            .query(
                "SELECT am.amname, opc.opcname, opc.opcdefault \
                 FROM pg_catalog.pg_am AS am \
                 JOIN pg_catalog.pg_opclass AS opc ON opc.opcmethod = am.oid \
                 WHERE am.amname IN ('hnsw', 'ivfflat') \
                 ORDER BY am.amname, opc.opcname",
                &[],
            )
            .await
            .expect("inspect installed vector method/operator-class identities");
        let operator_members = client
            .query(
                "SELECT am.amname, opc.opcname, amop.amopstrategy, amop.amoppurpose, \
                 amop.amopsortfamily, oper.oprname \
                 FROM pg_catalog.pg_amop AS amop \
                 JOIN pg_catalog.pg_opfamily AS opf ON opf.oid = amop.amopfamily \
                 JOIN pg_catalog.pg_opclass AS opc ON opc.opcfamily = opf.oid \
                 JOIN pg_catalog.pg_am AS am ON am.oid = amop.amopmethod \
                 JOIN pg_catalog.pg_operator AS oper ON oper.oid = amop.amopopr \
                 WHERE am.amname IN ('hnsw', 'ivfflat') \
                 ORDER BY am.amname, opc.opcname",
                &[],
            )
            .await
            .expect("inspect pgvector ordering operator family members");
        let operators = client
            .query(
                "SELECT oprname FROM pg_catalog.pg_operator \
                 WHERE oprname IN ('<->', '<#>', '<=>') ORDER BY oprname",
                &[],
            )
            .await
            .expect("inspect installed vector distance operators");

        // Assert
        assert_eq!(absent_methods, 0);
        assert_eq!(
            methods
                .iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, String>(1),
                        row.get::<_, bool>(2),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("hnsw".to_owned(), "vector_cosine_ops".to_owned(), false),
                ("hnsw".to_owned(), "vector_ip_ops".to_owned(), false),
                ("hnsw".to_owned(), "vector_l2_ops".to_owned(), false),
                ("ivfflat".to_owned(), "vector_cosine_ops".to_owned(), false),
                ("ivfflat".to_owned(), "vector_ip_ops".to_owned(), false),
                ("ivfflat".to_owned(), "vector_l2_ops".to_owned(), true),
            ]
        );
        assert_eq!(operator_members.len(), 6);
        for (row, operator) in operator_members
            .iter()
            .zip(["<=>", "<#>", "<->", "<=>", "<#>", "<->"])
        {
            assert_eq!(row.get::<_, i16>(2), 1);
            assert_eq!(row.get::<_, String>(3), "o");
            assert_eq!(row.get::<_, i64>(4), 1_970);
            assert_eq!(row.get::<_, String>(5), operator);
        }
        assert_eq!(
            operators
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>(),
            vec!["<#>".to_owned(), "<->".to_owned(), "<=>".to_owned()]
        );
        client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect("remove vector-owned identities");
        let removed_methods = query_i64(
            &client,
            VECTOR_METHOD_COUNT_SQL,
            "check vector methods after extension removal",
        )
        .await;
        assert_eq!(removed_methods, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_ann_indexes_above_the_pgvector_vector_dimension_limit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgvector_ann_dimension_limit").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector VERSION '0.8.7'",
                "CREATE TABLE pgvector_large_vectors (embedding vector(2001))",
            ],
        )
        .await
        .expect("allow storage of a vector above the ANN limit");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX pgvector_large_vectors_idx \
                 ON pgvector_large_vectors USING hnsw (embedding vector_l2_ops)",
            )
            .await
            .expect_err("pgvector ANN methods stop at 2,000 vector dimensions");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'pgvector_large_vectors_idx'",
                &[],
            )
            .await
            .expect("check that rejected ANN metadata was not published")
            .get(0);

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "54000");
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn query_i64(client: &tokio_postgres::Client, sql: &str, context: &str) -> i64 {
    client.query_one(sql, &[]).await.expect(context).get(0)
}

fn row_ids(rows: &[tokio_postgres::Row]) -> Vec<i32> {
    rows.iter().map(|row| row.get::<_, i32>(0)).collect()
}
