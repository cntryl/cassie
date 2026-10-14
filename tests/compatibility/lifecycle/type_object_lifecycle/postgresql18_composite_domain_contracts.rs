use super::*;

#[test]
fn should_distinguish_sql_null_from_all_null_composites() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_null_identity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SCHEMA compat_composite_null_identity; \
                 CREATE TYPE compat_composite_null_identity.pair_value \
                   AS (label TEXT, amount INT)",
            )
            .await
            .expect("create a named row type for SQL NULL distinctions");

        // Act
        let row = client
            .query_one(
                "SELECT NULL::compat_composite_null_identity.pair_value \
                          IS DISTINCT FROM \
                          ROW(NULL::TEXT, NULL::INT)::compat_composite_null_identity.pair_value, \
                        ROW(NULL::TEXT, NULL::INT)::compat_composite_null_identity.pair_value \
                          IS NULL, \
                        ROW(NULL::TEXT, NULL::INT)::compat_composite_null_identity.pair_value \
                          IS NOT NULL, \
                        ROW(NULL::TEXT, 1)::compat_composite_null_identity.pair_value \
                          IS NULL, \
                        ROW(NULL::TEXT, 1)::compat_composite_null_identity.pair_value \
                          IS NOT NULL, \
                        NULL::compat_composite_null_identity.pair_value::TEXT, \
                        ROW(NULL::TEXT, NULL::INT)::compat_composite_null_identity.pair_value::TEXT",
                &[],
            )
            .await
            .expect("compare SQL NULL with null-filled and partially null rows");

        // Assert
        assert!(row.get::<_, bool>(0));
        assert!(row.get::<_, bool>(1));
        assert!(!row.get::<_, bool>(2));
        assert!(!row.get::<_, bool>(3));
        assert!(!row.get::<_, bool>(4));
        assert_eq!(row.get::<_, Option<String>>(5), None);
        assert_eq!(row.get::<_, String>(6), "(,)");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_composite_row_oids_with_attribute_typmods() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_row_identity_typmods").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_composite_identity",
                "CREATE TYPE compat_composite_identity.pair_value \
                 AS (label VARCHAR(5), amount NUMERIC(8,2))",
                "CREATE TABLE compat_composite_identity.rows \
                 (id INT, payload compat_composite_identity.pair_value)",
                "INSERT INTO compat_composite_identity.rows VALUES \
                 (1, ROW('Ada', 12.34)::compat_composite_identity.pair_value)",
            ],
        )
        .await
        .expect("create a schema-scoped composite and a dependent relation row type");
        let type_oids = client
            .query_one(
                "SELECT named_type.oid::BIGINT, named_type.typarray::BIGINT, \
                        relation.reltype::BIGINT \
                 FROM pg_catalog.pg_type AS named_type \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = 'compat_composite_identity.rows'::regclass \
                 WHERE named_type.oid = \
                   'compat_composite_identity.pair_value'::regtype",
                &[],
            )
            .await
            .expect("read named and relation composite OIDs");

        // Act
        let relation_row = client
            .query_one(
                "SELECT row_value FROM compat_composite_identity.rows AS row_value",
                &[],
            )
            .await
            .expect("describe a table row as its named relation type");
        let composite_value = client
            .query_one(
                "SELECT payload FROM compat_composite_identity.rows WHERE id = 1",
                &[],
            )
            .await
            .expect("describe a stored named composite value");
        let composite_array = client
            .query_one(
                "SELECT ARRAY[ROW('Ada', 12.34)::compat_composite_identity.pair_value]",
                &[],
            )
            .await
            .expect("describe an array of the named composite type");
        let attributes = client
            .query(
                "SELECT attname, atttypid::BIGINT, atttypmod \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'compat_composite_identity.pair_value'::regclass \
                   AND attnum > 0 AND NOT attisdropped \
                 ORDER BY attnum",
                &[],
            )
            .await
            .expect("read the composite field type identities and typmods");

        // Assert
        let composite_oid = type_oids.get::<_, i64>(0);
        let composite_array_oid = type_oids.get::<_, i64>(1);
        let relation_oid = type_oids.get::<_, i64>(2);
        assert_eq!(
            i64::from(relation_row.columns()[0].type_().oid()),
            relation_oid
        );
        assert_eq!(relation_row.columns()[0].type_().name(), "rows");
        assert_eq!(
            relation_row.columns()[0].type_().schema(),
            "compat_composite_identity"
        );
        assert_eq!(
            i64::from(composite_value.columns()[0].type_().oid()),
            composite_oid
        );
        assert_eq!(
            i64::from(composite_array.columns()[0].type_().oid()),
            composite_array_oid
        );
        assert_eq!(composite_value.columns()[0].type_().name(), "pair_value");
        assert_eq!(composite_array.columns()[0].type_().name(), "_pair_value");
        assert_eq!(
            attributes
                .iter()
                .map(|row| (
                    row.get::<_, String>(0),
                    row.get::<_, i64>(1),
                    row.get::<_, i32>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                ("label".to_owned(), 1_043, 9),
                ("amount".to_owned(), 1_700, 524_294),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_domain_array_identity_with_base_result_type() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_base_and_array_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_domain_identity",
                "CREATE DOMAIN compat_domain_identity.positive_amount \
                 AS BIGINT NOT NULL CHECK (VALUE > 0)",
                "CREATE DOMAIN compat_domain_identity.short_label AS VARCHAR(5)",
                "CREATE TABLE compat_domain_identity.rows \
                 (id INT, amount compat_domain_identity.positive_amount, \
                  label compat_domain_identity.short_label)",
                "INSERT INTO compat_domain_identity.rows VALUES (1, 9, 'ready')",
            ],
        )
        .await
        .expect("create domains with a constraint and a base typmod");
        let amount_metadata = client
            .query_one(
                "SELECT oid::BIGINT, typbasetype::BIGINT, typarray::BIGINT, typnotnull \
                 FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_domain_identity.positive_amount'::regtype",
                &[],
            )
            .await
            .expect("read the domain identity, base type, array OID, and nullability");
        let label_typmod: i32 = client
            .query_one(
                "SELECT typtypmod FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_domain_identity.short_label'::regtype",
                &[],
            )
            .await
            .expect("read the domain's declared VARCHAR typmod")
            .get(0);

        // Act
        let amount = client
            .query_one(
                "SELECT amount, \
                        pg_typeof(amount) = \
                          'compat_domain_identity.positive_amount'::regtype \
                 FROM compat_domain_identity.rows WHERE id = 1",
                &[],
            )
            .await
            .expect("describe the flattened domain output and inspect its SQL identity");
        let amount_array = client
            .query_one(
                "SELECT ARRAY[9::compat_domain_identity.positive_amount, \
                              12::compat_domain_identity.positive_amount]",
                &[],
            )
            .await
            .expect("describe an array retaining the domain element identity");
        let amount_array_text = client
            .query_one(
                "SELECT ARRAY[9::compat_domain_identity.positive_amount, \
                              12::compat_domain_identity.positive_amount]::TEXT",
                &[],
            )
            .await
            .expect("render the domain array values");
        let label = client
            .query_one(
                "SELECT label FROM compat_domain_identity.rows WHERE id = 1",
                &[],
            )
            .await
            .expect("describe the domain over a length-modified base type");

        // Assert
        assert_eq!(amount_metadata.get::<_, i64>(1), 20);
        assert!(amount_metadata.get::<_, bool>(3));
        assert_eq!(amount.columns()[0].type_().oid(), 20);
        assert_eq!(amount.columns()[1].type_().oid(), 16);
        assert_eq!(amount.get::<_, i64>(0), 9);
        assert!(amount.get::<_, bool>(1));
        assert_eq!(
            i64::from(amount_array.columns()[0].type_().oid()),
            amount_metadata.get::<_, i64>(2)
        );
        assert_eq!(amount_array.columns()[0].type_().name(), "_positive_amount");
        assert_eq!(amount_array_text.get::<_, String>(0), "{9,12}");
        assert_eq!(label.columns()[0].type_().oid(), 1_043);
        assert_eq!(label.get::<_, String>(0), "ready");
        assert_eq!(label_typmod, 9);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_domain_constraints_inside_arrays() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_array_constraints").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SCHEMA compat_domain_array_checks; \
                 CREATE DOMAIN compat_domain_array_checks.positive_int \
                   AS INT NOT NULL CHECK (VALUE > 0)",
            )
            .await
            .expect("create a domain whose array elements must satisfy both constraints");

        // Act
        let null_element = client
            .query_one(
                "SELECT ARRAY[NULL::compat_domain_array_checks.positive_int]",
                &[],
            )
            .await
            .expect_err("reject NULL as an element of a NOT NULL domain array");
        let check_violation = client
            .query_one(
                "SELECT ARRAY[0::compat_domain_array_checks.positive_int]",
                &[],
            )
            .await
            .expect_err("reject an element that violates the domain CHECK");
        let valid = client
            .query_one(
                "SELECT ARRAY[1::compat_domain_array_checks.positive_int, \
                              2::compat_domain_array_checks.positive_int]::TEXT",
                &[],
            )
            .await
            .expect("accept domain array elements that satisfy both constraints");

        // Assert
        assert_eq!(db_error(&null_element).code().code(), "23502");
        assert_eq!(db_error(&check_violation).code().code(), "23514");
        assert_eq!(valid.get::<_, String>(0), "{1,2}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_typed_table_column_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("typed_table_column_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_typed_table_options",
                "CREATE TYPE compat_typed_table_options.inventory_item \
                 AS (item_id INT, label TEXT)",
                "CREATE TABLE compat_typed_table_options.inventory \
                 OF compat_typed_table_options.inventory_item ( \
                   item_id WITH OPTIONS CONSTRAINT inventory_item_positive \
                     CHECK (item_id > 0), \
                   label WITH OPTIONS DEFAULT 'pending' NOT NULL \
                 )",
                "INSERT INTO compat_typed_table_options.inventory (item_id) VALUES (1)",
            ],
        )
        .await
        .expect("create a typed table with column defaults, nullability, and a check");

        // Act
        let rejected = client
            .execute(
                "INSERT INTO compat_typed_table_options.inventory (item_id) VALUES (0)",
                &[],
            )
            .await
            .expect_err("reject a typed-table row that violates its column check");
        let rows = client
            .query(
                "SELECT item_id, label FROM compat_typed_table_options.inventory \
                 ORDER BY item_id",
                &[],
            )
            .await
            .expect("read the accepted typed-table row and its default");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "23514");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "pending");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_typed_table_relationship_changes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("typed_table_relationship_changes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_typed_table_relationship",
                "CREATE TYPE compat_typed_table_relationship.item \
                 AS (item_id INT, label TEXT)",
                "CREATE TABLE compat_typed_table_relationship.attach_later \
                 (item_id INT, label TEXT)",
            ],
        )
        .await
        .expect("create a type and a matching ordinary table");
        let item_oid = client
            .query_one(
                "SELECT oid::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = 'compat_typed_table_relationship.item'::regtype",
                &[],
            )
            .await
            .expect("read the named row type OID")
            .get::<_, i64>(0);

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_typed_table_relationship.attach_later \
                 OF compat_typed_table_relationship.item",
            )
            .await
            .expect("attach the matching table to the named row type");
        let attached_oid: i64 = client
            .query_one(
                "SELECT reloftype::BIGINT FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_typed_table_relationship.attach_later'::regclass",
                &[],
            )
            .await
            .expect("read the table's typed-row relationship")
            .get(0);
        client
            .batch_execute("ALTER TABLE compat_typed_table_relationship.attach_later NOT OF")
            .await
            .expect("remove the table's named row type relationship");
        let detached_oid: i64 = client
            .query_one(
                "SELECT reloftype::BIGINT FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_typed_table_relationship.attach_later'::regclass",
                &[],
            )
            .await
            .expect("read the table after detaching its typed-row relationship")
            .get(0);

        // Assert
        assert_eq!(attached_oid, item_oid);
        assert_eq!(detached_oid, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_named_type_owners_after_alteration() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_domain_owner_changes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_type_owner_828",
                "CREATE SCHEMA compat_type_owner_objects",
                "CREATE TYPE compat_type_owner_objects.pair_value AS (id BIGINT)",
                "CREATE DOMAIN compat_type_owner_objects.positive_amount AS BIGINT",
                "ALTER TYPE compat_type_owner_objects.pair_value \
                 OWNER TO compat_type_owner_828",
                "ALTER DOMAIN compat_type_owner_objects.positive_amount \
                 OWNER TO compat_type_owner_828",
            ],
        )
        .await
        .expect("transfer ownership of a composite type and domain");

        // Act
        let owners = client
            .query(
                "SELECT typname, pg_catalog.pg_get_userbyid(typowner) \
                 FROM pg_catalog.pg_type \
                 WHERE oid IN ( \
                   'compat_type_owner_objects.pair_value'::regtype, \
                   'compat_type_owner_objects.positive_amount'::regtype \
                 ) ORDER BY typname",
                &[],
            )
            .await
            .expect("read the actual composite and domain type owners");

        // Assert
        assert_eq!(
            owners
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (
                    "positive_amount".to_owned(),
                    "compat_type_owner_828".to_owned()
                ),
                ("pair_value".to_owned(), "compat_type_owner_828".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_type_identity_when_moving_namespaces() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_domain_schema_moves").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_type_move_a",
                "CREATE SCHEMA compat_type_move_b",
                "CREATE TYPE compat_type_move_a.pair_value \
                 AS (label TEXT, amount BIGINT)",
                "CREATE DOMAIN compat_type_move_a.positive_amount \
                 AS BIGINT CHECK (VALUE > 0)",
                "CREATE TABLE compat_type_move_a.rows \
                 (id INT, item compat_type_move_a.pair_value, \
                  amount compat_type_move_a.positive_amount)",
                "INSERT INTO compat_type_move_a.rows \
                 VALUES (1, ROW('Ada', 7)::compat_type_move_a.pair_value, 9)",
            ],
        )
        .await
        .expect("create schema-scoped composite and domain dependencies");
        let before_move = client
            .query_one(
                "SELECT \
                   (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_type_move_a.pair_value'::regtype), \
                   (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_type_move_a.positive_amount'::regtype)",
                &[],
            )
            .await
            .expect("capture type identities before changing their schemas");

        // Act
        execute_statements(
            &client,
            &[
                "ALTER TYPE compat_type_move_a.pair_value \
                 SET SCHEMA compat_type_move_b",
                "ALTER DOMAIN compat_type_move_a.positive_amount \
                 SET SCHEMA compat_type_move_b",
            ],
        )
        .await
        .expect("move both named types while retaining their dependent table column");
        let after_move = client
            .query_one(
                "SELECT to_regtype('compat_type_move_a.pair_value') IS NULL, \
                        to_regtype('compat_type_move_b.pair_value')::OID::BIGINT, \
                        to_regtype('compat_type_move_a.positive_amount') IS NULL, \
                        to_regtype('compat_type_move_b.positive_amount')::OID::BIGINT",
                &[],
            )
            .await
            .expect("resolve the old and new schema names after both moves");
        let dependent_row = client
            .query_one(
                "SELECT id, (item).label, (item).amount, amount::BIGINT, \
                        pg_typeof(amount) = \
                          'compat_type_move_b.positive_amount'::regtype \
                 FROM compat_type_move_a.rows",
                &[],
            )
            .await
            .expect("read composite and domain columns through their moved type identities");

        // Assert
        assert_eq!(before_move.get::<_, i64>(0), after_move.get::<_, i64>(1));
        assert_eq!(before_move.get::<_, i64>(1), after_move.get::<_, i64>(3));
        assert!(after_move.get::<_, bool>(0));
        assert!(after_move.get::<_, bool>(2));
        assert_eq!(dependent_row.get::<_, i32>(0), 1);
        assert_eq!(dependent_row.get::<_, String>(1), "Ada");
        assert_eq!(dependent_row.get::<_, i64>(2), 7);
        assert_eq!(dependent_row.get::<_, i64>(3), 9);
        assert!(dependent_row.get::<_, bool>(4));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_composite_domain_rows_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("composite-domain-restart");
    let _directory = DataDirectory(path.clone());
    let before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE SCHEMA compat_composite_domain_restart",
            "CREATE DOMAIN compat_composite_domain_restart.positive_amount \
             AS BIGINT NOT NULL CHECK (VALUE > 0)",
            "CREATE TYPE compat_composite_domain_restart.profile \
             AS (label VARCHAR(5), amount compat_composite_domain_restart.positive_amount)",
            "CREATE TABLE compat_composite_domain_restart.rows \
             (id INT PRIMARY KEY, profile compat_composite_domain_restart.profile, \
              score compat_composite_domain_restart.positive_amount)",
            "INSERT INTO compat_composite_domain_restart.rows \
             VALUES (1, ROW('Ada', 7)::compat_composite_domain_restart.profile, 9)",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("execute type restart setup {statement}: {error}"));
        }
        let identities = cassie
            .execute_sql(
                &session,
                "SELECT \
                   (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_composite_domain_restart.profile'::regtype), \
                   (SELECT typarray::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_composite_domain_restart.profile'::regtype), \
                   (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_composite_domain_restart.positive_amount'::regtype), \
                   (SELECT typarray::BIGINT FROM pg_catalog.pg_type \
                    WHERE oid = 'compat_composite_domain_restart.positive_amount'::regtype)",
                vec![],
            )
            .expect("read composite and domain type identities before restart")
            .rows()[0]
            .clone();
        let values = cassie
            .execute_sql(
                &session,
                "SELECT id, (profile).label, (profile).amount::BIGINT, score::BIGINT \
                 FROM compat_composite_domain_restart.rows ORDER BY id",
                vec![],
            )
            .expect("read the stored composite and domain values before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        (identities, values)
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened
        .startup()
        .expect("recover the local Cassie instance");
    let session = reopened.create_session("tester", None);
    let identities_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT \
               (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                WHERE oid = 'compat_composite_domain_restart.profile'::regtype), \
               (SELECT typarray::BIGINT FROM pg_catalog.pg_type \
                WHERE oid = 'compat_composite_domain_restart.profile'::regtype), \
               (SELECT oid::BIGINT FROM pg_catalog.pg_type \
                WHERE oid = 'compat_composite_domain_restart.positive_amount'::regtype), \
               (SELECT typarray::BIGINT FROM pg_catalog.pg_type \
                WHERE oid = 'compat_composite_domain_restart.positive_amount'::regtype)",
            vec![],
        )
        .expect("read composite and domain type identities after restart")
        .rows()[0]
        .clone();
    let values_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT id, (profile).label, (profile).amount::BIGINT, score::BIGINT \
             FROM compat_composite_domain_restart.rows ORDER BY id",
            vec![],
        )
        .expect("read the stored composite and domain values after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(identities_after_restart, before_restart.0);
    assert_eq!(values_after_restart, before_restart.1);
    assert_eq!(
        values_after_restart,
        vec![vec![
            cassie::types::Value::Int64(1),
            cassie::types::Value::String("Ada".to_owned()),
            cassie::types::Value::Int64(7),
            cassie::types::Value::Int64(9),
        ]]
    );
}
