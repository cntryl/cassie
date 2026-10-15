use super::*;

#[test]
fn should_catalog_both_jsonb_gin_operator_classes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("jsonb_gin_operator_classes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_jsonb_index_rows (body JSONB)",
                "CREATE INDEX compat_jsonb_ops_idx \
                   ON compat_jsonb_index_rows USING gin (body jsonb_ops)",
                "CREATE INDEX compat_jsonb_path_ops_idx \
                   ON compat_jsonb_index_rows USING gin (body jsonb_path_ops)",
            ],
        )
        .await
        .expect("create GIN indexes with both PostgreSQL JSONB operator classes");

        // Act
        let jsonb_ops = client
            .query_one(
                "SELECT pg_catalog.pg_get_indexdef('compat_jsonb_ops_idx'::regclass), \
                        (SELECT opclass.opcname \
                         FROM pg_catalog.pg_opclass AS opclass \
                         WHERE opclass.oid = (\
                           SELECT index_row.indclass[0] \
                           FROM pg_catalog.pg_index AS index_row \
                           WHERE index_row.indexrelid = \
                             'compat_jsonb_ops_idx'::regclass))",
                &[],
            )
            .await
            .expect("read the default JSONB operator class from the index catalog");
        let jsonb_path_ops = client
            .query_one(
                "SELECT pg_catalog.pg_get_indexdef('compat_jsonb_path_ops_idx'::regclass), \
                        (SELECT opclass.opcname \
                         FROM pg_catalog.pg_opclass AS opclass \
                         WHERE opclass.oid = (\
                           SELECT index_row.indclass[0] \
                           FROM pg_catalog.pg_index AS index_row \
                           WHERE index_row.indexrelid = \
                             'compat_jsonb_path_ops_idx'::regclass))",
                &[],
            )
            .await
            .expect("read the JSONB path operator class from the index catalog");

        // Assert
        assert_eq!(
            jsonb_ops.get::<_, String>(0),
            "CREATE INDEX compat_jsonb_ops_idx \
             ON public.compat_jsonb_index_rows USING gin (body)"
        );
        assert_eq!(jsonb_ops.get::<_, String>(1), "jsonb_ops");
        assert_eq!(
            jsonb_path_ops.get::<_, String>(0),
            "CREATE INDEX compat_jsonb_path_ops_idx \
             ON public.compat_jsonb_index_rows USING gin (body jsonb_path_ops)"
        );
        assert_eq!(jsonb_path_ops.get::<_, String>(1), "jsonb_path_ops");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_build_both_full_text_index_access_methods() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("tsvector_index_access_methods").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_tsvector_index_rows (document TSVECTOR)",
                "CREATE INDEX compat_tsvector_gin_idx \
                   ON compat_tsvector_index_rows USING gin (document)",
                "CREATE INDEX compat_tsvector_gist_idx \
                   ON compat_tsvector_index_rows USING gist (document)",
            ],
        )
        .await
        .expect("create GIN and GiST indexes over tsvector values");

        // Act
        let indexes = client
            .query(
                "SELECT index_class.relname, access_method.amname, \
                        opclass.opcname, pg_catalog.pg_get_indexdef(index_class.oid) \
                 FROM pg_catalog.pg_class AS index_class \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_am AS access_method \
                   ON access_method.oid = index_class.relam \
                 JOIN pg_catalog.pg_opclass AS opclass \
                   ON opclass.oid = index_row.indclass[0] \
                 WHERE index_class.relname IN (\
                   'compat_tsvector_gin_idx', 'compat_tsvector_gist_idx') \
                 ORDER BY index_class.relname",
                &[],
            )
            .await
            .expect("read each access method and tsvector operator class");

        // Assert
        assert_eq!(indexes.len(), 2);
        assert_eq!(
            indexes
                .iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, String>(1),
                        row.get::<_, String>(2),
                        row.get::<_, String>(3),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (
                    "compat_tsvector_gin_idx".to_owned(),
                    "gin".to_owned(),
                    "tsvector_ops".to_owned(),
                    "CREATE INDEX compat_tsvector_gin_idx \
                     ON public.compat_tsvector_index_rows USING gin (document)"
                        .to_owned(),
                ),
                (
                    "compat_tsvector_gist_idx".to_owned(),
                    "gist".to_owned(),
                    "tsvector_ops".to_owned(),
                    "CREATE INDEX compat_tsvector_gist_idx \
                     ON public.compat_tsvector_index_rows USING gist (document)"
                        .to_owned(),
                ),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_build_gist_index_with_explicit_inet_operator_class() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inet_gist_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_inet_gist_index_rows (address INET)",
                "CREATE INDEX compat_inet_gist_idx \
                   ON compat_inet_gist_index_rows USING gist (address inet_ops)",
            ],
        )
        .await
        .expect("create a GiST network index with the inet operator class");

        // Act
        let metadata = client
            .query_one(
                "SELECT access_method.amname, opclass.opcname, \
                        pg_catalog.pg_get_indexdef(index_class.oid) \
                 FROM pg_catalog.pg_class AS index_class \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_am AS access_method \
                   ON access_method.oid = index_class.relam \
                 JOIN pg_catalog.pg_opclass AS opclass \
                   ON opclass.oid = index_row.indclass[0] \
                 WHERE index_class.oid = 'compat_inet_gist_idx'::regclass",
                &[],
            )
            .await
            .expect("read the GiST access method and inet operator class");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 19, 25]
        );
        assert_eq!(metadata.get::<_, String>(0), "gist");
        assert_eq!(metadata.get::<_, String>(1), "inet_ops");
        assert_eq!(
            metadata.get::<_, String>(2),
            "CREATE INDEX compat_inet_gist_idx \
             ON public.compat_inet_gist_index_rows USING gist (address inet_ops)"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_brin_operator_class_parameters_to_index_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("brin_opclass_parameter").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_brin_parameter_rows (id INT)",
                "CREATE INDEX compat_brin_parameter_idx \
                   ON compat_brin_parameter_rows USING brin \
                   (id int4_minmax_multi_ops (values_per_range = 8))",
            ],
        )
        .await
        .expect("create a BRIN index with an operator class parameter");

        // Act
        let metadata = client
            .query_one(
                "SELECT access_method.amname, opclass.opcname, \
                        pg_catalog.pg_get_indexdef(index_class.oid) \
                 FROM pg_catalog.pg_class AS index_class \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_am AS access_method \
                   ON access_method.oid = index_class.relam \
                 JOIN pg_catalog.pg_opclass AS opclass \
                   ON opclass.oid = index_row.indclass[0] \
                 WHERE index_class.oid = 'compat_brin_parameter_idx'::regclass",
                &[],
            )
            .await
            .expect("read the BRIN method and parameterized operator class");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 19, 25]
        );
        assert_eq!(metadata.get::<_, String>(0), "brin");
        assert_eq!(metadata.get::<_, String>(1), "int4_minmax_multi_ops");
        assert_eq!(
            metadata.get::<_, String>(2),
            "CREATE INDEX compat_brin_parameter_idx \
             ON public.compat_brin_parameter_rows USING brin \
             (id int4_minmax_multi_ops (values_per_range='8'))"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_build_spgist_index_with_default_inet_operator_class() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("spgist_inet_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_spgist_inet_index_rows (address INET)",
                "CREATE INDEX compat_spgist_inet_idx \
                   ON compat_spgist_inet_index_rows USING spgist (address)",
            ],
        )
        .await
        .expect("create an SP-GiST index with its default inet operator class");

        // Act
        let metadata = client
            .query_one(
                "SELECT access_method.amname, opclass.opcname, \
                        pg_catalog.pg_get_indexdef(index_class.oid) \
                 FROM pg_catalog.pg_class AS index_class \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_am AS access_method \
                   ON access_method.oid = index_class.relam \
                 JOIN pg_catalog.pg_opclass AS opclass \
                   ON opclass.oid = index_row.indclass[0] \
                 WHERE index_class.oid = 'compat_spgist_inet_idx'::regclass",
                &[],
            )
            .await
            .expect("read the SP-GiST method and inferred inet operator class");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 19, 25]
        );
        assert_eq!(metadata.get::<_, String>(0), "spgist");
        assert_eq!(metadata.get::<_, String>(1), "inet_ops");
        assert_eq!(
            metadata.get::<_, String>(2),
            "CREATE INDEX compat_spgist_inet_idx \
             ON public.compat_spgist_inet_index_rows USING spgist (address)"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
