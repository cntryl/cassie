use super::*;

mod postgresql18_composite_attribute_type_alter;
mod postgresql18_composite_domain_contracts;
mod postgresql18_enum_catalog_snapshot_visibility;
mod postgresql18_enum_identity_lifecycle;
mod postgresql18_enum_record_domain_wave6;
mod postgresql18_enum_support_function_identity;
mod postgresql18_record_comparison_residual;

#[test]
fn should_render_relation_record_null_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("relation_record_null_semantics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_record_rows (a INT, b INT)",
                "INSERT INTO compat_record_rows VALUES (NULL, NULL), (NULL, 1), (1, 2)",
            ],
        )
        .await
        .expect("create relation rows with all-null, partial-null, and populated fields");

        // Act
        let rows = client
            .query(
                "SELECT record_value IS NULL, record_value IS NOT NULL, record_value::TEXT \
                 FROM compat_record_rows AS record_value \
                 ORDER BY a NULLS FIRST, b NULLS FIRST",
                &[],
            )
            .await
            .expect("inspect the relation's whole-row record values");

        // Assert
        assert_eq!(rows.len(), 3);
        assert!(rows[0].get::<_, bool>(0));
        assert!(!rows[0].get::<_, bool>(1));
        assert_eq!(rows[0].get::<_, String>(2), "(,)");
        assert!(!rows[1].get::<_, bool>(0));
        assert!(!rows[1].get::<_, bool>(1));
        assert_eq!(rows[1].get::<_, String>(2), "(,1)");
        assert!(!rows[2].get::<_, bool>(0));
        assert!(rows[2].get::<_, bool>(1));
        assert_eq!(rows[2].get::<_, String>(2), "(1,2)");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_order_enum_first_last_range_by_label() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_support_functions").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TYPE compat_priority AS ENUM ('low', 'medium', 'high')")
            .await
            .expect("create an enum with ordered labels");
        execute_statements(
            &client,
            &[
                "ALTER TYPE compat_priority ADD VALUE IF NOT EXISTS 'medium_high' AFTER 'medium'",
                "ALTER TYPE compat_priority ADD VALUE IF NOT EXISTS 'medium_high' AFTER 'medium'",
            ],
        )
        .await
        .expect("add an enum label after its neighbor idempotently");

        // Act
        let row = client
            .query_one(
                "SELECT enum_first(NULL::compat_priority)::TEXT, \
                 enum_last(NULL::compat_priority)::TEXT, \
                 enum_range(NULL::compat_priority)::TEXT, \
                 enum_range('low'::compat_priority, 'medium'::compat_priority)::TEXT",
                &[],
            )
            .await
            .expect("read enum boundary and range functions");

        // Assert
        assert_eq!(row.get::<_, String>(0), "low");
        assert_eq!(row.get::<_, String>(1), "high");
        assert_eq!(row.get::<_, String>(2), "{low,medium,medium_high,high}");
        assert_eq!(row.get::<_, String>(3), "{low,medium}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_update_existing_rows_after_enum_label_rename() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_rename_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_mood_rename AS ENUM ('sad', 'okay', 'happy')",
                "CREATE TABLE compat_mood_rows (id INT PRIMARY KEY, mood compat_mood_rename)",
                "INSERT INTO compat_mood_rows VALUES (1, 'okay'), (2, 'happy')",
            ],
        )
        .await
        .expect("create an enum and rows that use its labels");

        // Act
        client
            .batch_execute("ALTER TYPE compat_mood_rename RENAME VALUE 'okay' TO 'calm'")
            .await
            .expect("rename an enum label without replacing its stored values");
        let rows = client
            .query(
                "SELECT id, mood::TEXT FROM compat_mood_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read rows stored before the enum label rename");
        let labels: String = client
            .query_one("SELECT enum_range(NULL::compat_mood_rename)::TEXT", &[])
            .await
            .expect("read the enum labels in sort order")
            .get(0);

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "calm");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "happy");
        assert_eq!(labels, "{sad,calm,happy}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_enum_value_addition_to_commit_before_use() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_commit_before_use").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_tx_mood AS ENUM ('waiting')",
                "CREATE TABLE compat_tx_mood_rows (mood compat_tx_mood)",
            ],
        )
        .await
        .expect("create an enum and table for transactional label use");

        // Act
        client
            .batch_execute("BEGIN; ALTER TYPE compat_tx_mood ADD VALUE 'ready'")
            .await
            .expect("add a new enum label inside a transaction");
        let unsafe_use = client
            .execute("INSERT INTO compat_tx_mood_rows VALUES ('ready')", &[])
            .await
            .expect_err("reject use of an enum value before its transaction commits");
        let unsafe_use_sqlstate = db_error(&unsafe_use).code().code().to_owned();
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the failed enum-label transaction");
        let labels_after_rollback: String = client
            .query_one("SELECT enum_range(NULL::compat_tx_mood)::TEXT", &[])
            .await
            .expect("read enum labels after rolling back their addition")
            .get(0);
        client
            .batch_execute("ALTER TYPE compat_tx_mood ADD VALUE 'ready'")
            .await
            .expect("commit the label addition in its own statement");
        client
            .execute("INSERT INTO compat_tx_mood_rows VALUES ('ready')", &[])
            .await
            .expect("use the label after the addition committed");
        let stored: String = client
            .query_one("SELECT mood::TEXT FROM compat_tx_mood_rows", &[])
            .await
            .expect("read the enum value after commit")
            .get(0);

        // Assert
        assert_eq!(unsafe_use_sqlstate, "55P04");
        assert_eq!(labels_after_rollback, "{waiting}");
        assert_eq!(stored, "ready");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_composite_values_across_attribute_type_renames() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_type_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_address AS (street TEXT, postal_code INT)",
                "CREATE TABLE compat_composite_rows (id INT PRIMARY KEY, address compat_address)",
                "INSERT INTO compat_composite_rows VALUES \
                   (1, ROW('Main', 12345)::compat_address)",
                "UPDATE compat_composite_rows SET address.street = 'Main Street' WHERE id = 1",
                "ALTER TYPE compat_address RENAME ATTRIBUTE street TO road",
                "ALTER TYPE compat_address ADD ATTRIBUTE country TEXT",
            ],
        )
        .await
        .expect("create and alter a named composite type");

        // Act
        let before_type_rename = client
            .query(
                "SELECT (address).road, (address).postal_code, (address).country \
                 FROM compat_composite_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read fields after composite attribute changes");
        client
            .batch_execute("ALTER TYPE compat_address RENAME TO compat_address_v2")
            .await
            .expect("rename the composite type without changing stored values");
        let after_type_rename = client
            .query_one(
                "SELECT (address).road, (address).postal_code, (address).country \
                 FROM compat_composite_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read composite fields through the renamed type");
        let dependent_drop = client
            .batch_execute("DROP TYPE compat_address_v2")
            .await
            .expect_err("the table column must retain a dependency on its type");
        execute_statements(
            &client,
            &[
                "DROP TABLE compat_composite_rows",
                "DROP TYPE compat_address_v2",
            ],
        )
        .await
        .expect("remove the composite dependency and type");

        // Assert
        assert_eq!(before_type_rename.len(), 1);
        assert_eq!(before_type_rename[0].get::<_, String>(0), "Main Street");
        assert_eq!(before_type_rename[0].get::<_, i32>(1), 12345);
        assert_eq!(before_type_rename[0].get::<_, Option<String>>(2), None);
        assert_eq!(after_type_rename.get::<_, String>(0), "Main Street");
        assert_eq!(after_type_rename.get::<_, i32>(1), 12345);
        assert_eq!(after_type_rename.get::<_, Option<String>>(2), None);
        assert_eq!(db_error(&dependent_drop).code().code(), "2BP01");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_drop_a_composite_attribute_from_stored_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_drop_attribute_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_profile_drop AS (name TEXT, nickname TEXT, score INT)",
                "CREATE TABLE compat_profile_rows (id INT PRIMARY KEY, profile compat_profile_drop)",
                "INSERT INTO compat_profile_rows VALUES \
                   (1, ROW('Ada', 'Enchantress', 8)::compat_profile_drop)",
            ],
        )
        .await
        .expect("create a composite type and store a fully populated row");

        // Act
        client
            .batch_execute("ALTER TYPE compat_profile_drop DROP ATTRIBUTE nickname")
            .await
            .expect("drop one composite attribute while retaining dependent row values");
        let row = client
            .query_one(
                "SELECT (profile).name, (profile).score FROM compat_profile_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the remaining attributes from the stored composite value");

        // Assert
        assert_eq!(row.get::<_, String>(0), "Ada");
        assert_eq!(row.get::<_, i32>(1), 8);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_update_composite_fields_in_typed_tables() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("typed_table_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_inventory_item AS (name TEXT, quantity INT)",
                "CREATE TABLE compat_inventory OF compat_inventory_item",
                "INSERT INTO compat_inventory VALUES ('pear', 7), ('apple', 2)",
                "UPDATE compat_inventory SET quantity = quantity + 1 WHERE name = 'pear'",
                "CREATE TABLE compat_nested_inventory (id INT, item compat_inventory_item)",
                "INSERT INTO compat_nested_inventory VALUES \
                   (1, ROW('initial', 5)::compat_inventory_item)",
                "UPDATE compat_nested_inventory SET item.name = 'changed' WHERE id = 1",
            ],
        )
        .await
        .expect("create and mutate typed and composite tables");

        // Act
        let typed_rows = client
            .query(
                "SELECT name, quantity FROM compat_inventory ORDER BY name",
                &[],
            )
            .await
            .expect("read rows from the typed table");
        let nested_row = client
            .query_one(
                "SELECT (item).name, (item).quantity FROM compat_nested_inventory WHERE id = 1",
                &[],
            )
            .await
            .expect("read fields after composite-field assignment");

        // Assert
        assert_eq!(typed_rows.len(), 2);
        assert_eq!(typed_rows[0].get::<_, String>(0), "apple");
        assert_eq!(typed_rows[0].get::<_, i32>(1), 2);
        assert_eq!(typed_rows[1].get::<_, String>(0), "pear");
        assert_eq!(typed_rows[1].get::<_, i32>(1), 8);
        assert_eq!(nested_row.get::<_, String>(0), "changed");
        assert_eq!(nested_row.get::<_, i32>(1), 5);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_domain_defaults_nullability_added_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_alter_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE DOMAIN compat_label AS TEXT NOT NULL DEFAULT 'fallback'",
                "CREATE TABLE compat_domain_defaults (id INT PRIMARY KEY, label compat_label)",
                "INSERT INTO compat_domain_defaults VALUES (1, DEFAULT)",
                "ALTER DOMAIN compat_label SET DEFAULT 'updated'",
                "INSERT INTO compat_domain_defaults VALUES (2, DEFAULT)",
                "ALTER DOMAIN compat_label DROP NOT NULL",
                "ALTER DOMAIN compat_label DROP DEFAULT",
                "ALTER DOMAIN compat_label ADD CONSTRAINT compat_label_nonempty CHECK (VALUE <> '')",
                "ALTER DOMAIN compat_label RENAME CONSTRAINT compat_label_nonempty TO compat_label_present",
                "INSERT INTO compat_domain_defaults VALUES (3, NULL)",
            ],
        )
        .await
        .expect("alter domain defaults, nullability, and constraints");

        // Act
        let invalid = client
            .execute("INSERT INTO compat_domain_defaults VALUES (4, '')", &[])
            .await
            .expect_err("the renamed domain check rejects an empty value");
        let rows = client
            .query(
                "SELECT id, label::TEXT FROM compat_domain_defaults ORDER BY id",
                &[],
            )
            .await
            .expect("read values using the altered domain");

        // Assert
        assert_eq!(db_error(&invalid).code().code(), "23514");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "fallback");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "updated");
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(rows[2].get::<_, Option<String>>(1), None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_validate_existing_domain_values_before_adding_a_check() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_existing_value_validation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE DOMAIN compat_domain_validation AS TEXT",
                "CREATE TABLE compat_domain_validation_rows \
                   (id INT PRIMARY KEY, label compat_domain_validation)",
                "INSERT INTO compat_domain_validation_rows VALUES (1, 'valid'), (2, '')",
            ],
        )
        .await
        .expect("create a domain table containing a value that violates the proposed check");

        // Act
        let rejected = client
            .batch_execute(
                "ALTER DOMAIN compat_domain_validation \
                 ADD CONSTRAINT compat_domain_nonempty CHECK (VALUE <> '')",
            )
            .await;
        let rejected_sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let constraint_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE contypid = 'compat_domain_validation'::regtype \
                   AND conname = 'compat_domain_nonempty'",
                &[],
            )
            .await
            .expect("inspect whether the failed domain constraint was published")
            .try_get(0)
            .expect("domain constraint count");
        let invalid_existing_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_domain_validation_rows WHERE id = 2",
                &[],
            )
            .await
            .expect("verify the failed constraint addition retained its source row")
            .try_get(0)
            .expect("invalid existing row count");
        let mut valid_addition_succeeded = false;
        let mut invalid_insert_sqlstate = None;
        if rejected_sqlstate.as_deref() == Some("23514") {
            client
                .execute(
                    "DELETE FROM compat_domain_validation_rows WHERE id = 2",
                    &[],
                )
                .await
                .expect("remove the existing row that violates the check");
            valid_addition_succeeded = client
                .batch_execute(
                    "ALTER DOMAIN compat_domain_validation \
                     ADD CONSTRAINT compat_domain_nonempty CHECK (VALUE <> '')",
                )
                .await
                .is_ok();
            invalid_insert_sqlstate = client
                .execute(
                    "INSERT INTO compat_domain_validation_rows VALUES (3, '')",
                    &[],
                )
                .await
                .err()
                .map(|error| db_error(&error).code().code().to_owned());
        }

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(rejected_sqlstate.as_deref(), Some("23514"));
        assert_eq!(constraint_count, 0);
        assert_eq!(invalid_existing_rows, 1);
        assert!(valid_addition_succeeded);
        assert_eq!(invalid_insert_sqlstate.as_deref(), Some("23514"));
    });
}

#[test]
fn should_roundtrip_named_type_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_type_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_comment_kind (id INT)")
            .await
            .expect("create a table with a named composite row type");

        // Act
        client
            .batch_execute("COMMENT ON TYPE compat_comment_kind IS 'type note'")
            .await
            .expect("set a comment on the named type");
        let comment: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_kind'::regtype, 'pg_type')",
                &[],
            )
            .await
            .expect("read the type comment from PostgreSQL object metadata")
            .try_get(0)
            .expect("type comment value");

        // Assert
        assert_eq!(comment.as_deref(), Some("type note"));

        drop(client);
        server.shutdown(connection).await;
    });
}
