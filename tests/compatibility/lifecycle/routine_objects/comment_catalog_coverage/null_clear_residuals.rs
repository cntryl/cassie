use super::*;

#[test]
fn should_clear_table_comment_from_relation_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE public.compat845_table_comment_clear (id INT)",
                "COMMENT ON TABLE public.compat845_table_comment_clear \
                   IS 'table note'",
            ],
        )
        .await
        .expect("create a table and attach its initial description");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description, objsubid::SMALLINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = 'public.compat845_table_comment_clear'::regclass \
                   AND classoid = 'pg_catalog.pg_class'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the table's relation description identity");
        client
            .batch_execute("COMMENT ON TABLE public.compat845_table_comment_clear IS NULL")
            .await
            .expect("clear the table description");
        let after_clear = client
            .query_one(
                "SELECT \
                   obj_description(\
                     'public.compat845_table_comment_clear'::regclass, 'pg_class') IS NULL, \
                   count(*)::BIGINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = 'public.compat845_table_comment_clear'::regclass \
                   AND classoid = 'pg_catalog.pg_class'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the cleared description through its relation identity");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 21]
        );
        assert_eq!(before_clear.get::<_, String>(0), "table note");
        assert_eq!(before_clear.get::<_, i16>(1), 0);
        assert_eq!(
            after_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![16, 20]
        );
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_clear_column_comment_from_attribute_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE public.compat845_column_comment_clear (id INT)",
                "COMMENT ON COLUMN public.compat845_column_comment_clear.id \
                   IS 'column note'",
            ],
        )
        .await
        .expect("create a table and attach a column description");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description.objsubid::SMALLINT, description.description, \
                        attribute.attnum::SMALLINT \
                 FROM pg_catalog.pg_description AS description \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = description.objoid \
                 JOIN pg_catalog.pg_attribute AS attribute \
                   ON attribute.attrelid = relation.oid \
                  AND attribute.attnum = description.objsubid \
                 WHERE relation.oid = \
                   'public.compat845_column_comment_clear'::regclass \
                   AND description.classoid = 'pg_catalog.pg_class'::regclass \
                   AND attribute.attname = 'id'",
                &[],
            )
            .await
            .expect("read the column description keyed by its attribute number");
        client
            .batch_execute("COMMENT ON COLUMN public.compat845_column_comment_clear.id IS NULL")
            .await
            .expect("clear the column description");
        let after_clear = client
            .query_one(
                "SELECT \
                   col_description(relation.oid, attribute.attnum) IS NULL, \
                   (SELECT count(*)::BIGINT \
                    FROM pg_catalog.pg_description AS description \
                    WHERE description.objoid = relation.oid \
                      AND description.classoid = 'pg_catalog.pg_class'::regclass \
                      AND description.objsubid = attribute.attnum) \
                 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_attribute AS attribute \
                   ON attribute.attrelid = relation.oid \
                 WHERE relation.oid = \
                   'public.compat845_column_comment_clear'::regclass \
                   AND attribute.attname = 'id'",
                &[],
            )
            .await
            .expect("read the cleared column description by attribute identity");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![21, 25, 21]
        );
        assert_eq!(before_clear.get::<_, i16>(0), 1);
        assert_eq!(before_clear.get::<_, String>(1), "column note");
        assert_eq!(before_clear.get::<_, i16>(2), 1);
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_clear_composite_type_comment_from_type_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_type_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE public.compat845_composite_comment_clear \
                   AS (id INT)",
                "COMMENT ON TYPE public.compat845_composite_comment_clear \
                   IS 'composite note'",
            ],
        )
        .await
        .expect("create a named composite type and attach its description");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description.classoid::regclass::TEXT, \
                        description.objsubid::SMALLINT, description.description \
                 FROM pg_catalog.pg_description AS description \
                 WHERE description.objoid = \
                   'public.compat845_composite_comment_clear'::regtype \
                   AND description.classoid = 'pg_catalog.pg_type'::regclass",
                &[],
            )
            .await
            .expect("read the type description's class and object identity");
        client
            .batch_execute("COMMENT ON TYPE public.compat845_composite_comment_clear IS NULL")
            .await
            .expect("clear the composite type description");
        let after_clear = client
            .query_one(
                "SELECT \
                   obj_description(\
                     'public.compat845_composite_comment_clear'::regtype, 'pg_type') IS NULL, \
                   count(*)::BIGINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = \
                   'public.compat845_composite_comment_clear'::regtype \
                   AND classoid = 'pg_catalog.pg_type'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the cleared comment from type metadata");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 21, 25]
        );
        assert_eq!(before_clear.get::<_, String>(0), "pg_type");
        assert_eq!(before_clear.get::<_, i16>(1), 0);
        assert_eq!(before_clear.get::<_, String>(2), "composite note");
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_clear_generic_routine_comment_by_identity_signature() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("generic_routine_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION public.compat845_routine_comment_clear(input INT) \
                   RETURNS INT LANGUAGE SQL IMMUTABLE AS $$ SELECT input $$",
                "COMMENT ON ROUTINE public.compat845_routine_comment_clear(INT) \
                   IS 'routine note'",
            ],
        )
        .await
        .expect("create and annotate a routine with an explicit identity signature");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description.classoid::regclass::TEXT, \
                        description.objsubid::SMALLINT, description.description \
                 FROM pg_catalog.pg_description AS description \
                 WHERE description.objoid = \
                   'public.compat845_routine_comment_clear(integer)'::regprocedure \
                   AND description.classoid = 'pg_catalog.pg_proc'::regclass",
                &[],
            )
            .await
            .expect("read the routine description using its identity signature");
        client
            .batch_execute(
                "COMMENT ON ROUTINE \
                 public.compat845_routine_comment_clear(integer) IS NULL",
            )
            .await
            .expect("clear the generic routine description");
        let after_clear = client
            .query_one(
                "SELECT \
                   obj_description(\
                     'public.compat845_routine_comment_clear(integer)'::regprocedure, \
                     'pg_proc') IS NULL, \
                   count(*)::BIGINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = \
                   'public.compat845_routine_comment_clear(integer)'::regprocedure \
                   AND classoid = 'pg_catalog.pg_proc'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the cleared routine description from pg_proc");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 21, 25]
        );
        assert_eq!(before_clear.get::<_, String>(0), "pg_proc");
        assert_eq!(before_clear.get::<_, i16>(1), 0);
        assert_eq!(before_clear.get::<_, String>(2), "routine note");
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
