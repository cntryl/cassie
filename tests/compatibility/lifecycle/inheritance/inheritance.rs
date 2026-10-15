use super::*;

#[path = "alter_existing_child.rs"]
mod alter_existing_child;
#[path = "../mutations/detach_existing_child.rs"]
mod detach_existing_child;
#[path = "restart.rs"]
mod restart;

include!("../mutations/inheritance.rs");

#[test]
fn should_bound_only_scans_at_each_inheritance_level() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_descendant_only_scans").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_inherit_root (id INT, label TEXT)",
                "CREATE TABLE compat_inherit_child (branch TEXT) \
                   INHERITS (compat_inherit_root)",
                "CREATE TABLE compat_inherit_grandchild (leaf INT) \
                   INHERITS (compat_inherit_child)",
                "INSERT INTO compat_inherit_root VALUES (1, 'root')",
                "INSERT INTO compat_inherit_child VALUES (2, 'child', 'branch')",
                "INSERT INTO compat_inherit_grandchild \
                   VALUES (3, 'grandchild', 'branch', 30)",
            ],
        )
        .await
        .expect("create a three-level inheritance tree and seed each level");

        // Act
        let all_root_rows = client
            .query("SELECT id FROM compat_inherit_root ORDER BY id", &[])
            .await
            .expect("scan the root and every inherited descendant");
        let only_root_rows = client
            .query("SELECT id FROM ONLY compat_inherit_root ORDER BY id", &[])
            .await
            .expect("scan only the root relation");
        let all_child_rows = client
            .query("SELECT id FROM compat_inherit_child ORDER BY id", &[])
            .await
            .expect("scan the child and its inherited descendants");
        let only_child_rows = client
            .query("SELECT id FROM ONLY compat_inherit_child ORDER BY id", &[])
            .await
            .expect("scan only the child relation");
        let inheritance_edges = client
            .query(
                "SELECT parent.relname, child.relname, i.inhseqno \
                 FROM pg_catalog.pg_inherits AS i \
                 JOIN pg_catalog.pg_class AS parent ON parent.oid = i.inhparent \
                 JOIN pg_catalog.pg_class AS child ON child.oid = i.inhrelid \
                 WHERE parent.relname IN \
                   ('compat_inherit_root', 'compat_inherit_child') \
                 ORDER BY parent.relname, i.inhseqno",
                &[],
            )
            .await
            .expect("inspect the two direct inheritance edges");

        // Assert
        let ids = |rows: &[tokio_postgres::Row]| {
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&all_root_rows), vec![1, 2, 3]);
        assert_eq!(ids(&only_root_rows), vec![1]);
        assert_eq!(ids(&all_child_rows), vec![2, 3]);
        assert_eq!(ids(&only_child_rows), vec![2]);
        assert_eq!(
            inheritance_edges
                .iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, String>(1),
                        row.get::<_, i32>(2),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (
                    "compat_inherit_child".to_owned(),
                    "compat_inherit_grandchild".to_owned(),
                    1,
                ),
                (
                    "compat_inherit_root".to_owned(),
                    "compat_inherit_child".to_owned(),
                    1,
                ),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_nulls_in_inherited_not_null_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_not_null_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_not_null_parent (id INT NOT NULL)",
                "CREATE TABLE compat_constraint_child (child_note TEXT) \
                   INHERITS (compat_not_null_parent)",
                "CREATE TABLE compat_constraint_grandchild (leaf INT) \
                   INHERITS (compat_constraint_child)",
            ],
        )
        .await
        .expect("create a three-level tree with an inherited NOT NULL column");

        // Act
        let child_null = client
            .execute(
                "INSERT INTO compat_constraint_child (id, child_note) VALUES (NULL, 'null')",
                &[],
            )
            .await;
        let grandchild_null = client
            .execute(
                "INSERT INTO compat_constraint_grandchild \
                   (id, child_note, leaf) VALUES (NULL, 'null', 7)",
                &[],
            )
            .await;
        let inherited_not_null = client
            .query(
                "SELECT relation.relname, attribute.attnotnull, \
                        attribute.attislocal, attribute.attinhcount \
                 FROM pg_catalog.pg_attribute AS attribute \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = attribute.attrelid \
                 WHERE relation.relname IN \
                   ('compat_not_null_parent', 'compat_constraint_child', \
                    'compat_constraint_grandchild') \
                   AND attribute.attname = 'id' AND attribute.attnum > 0 \
                 ORDER BY relation.relname",
                &[],
            )
            .await
            .expect("inspect inherited NOT NULL and column inheritance flags");

        // Assert
        assert_eq!(
            db_error(
                child_null
                    .as_ref()
                    .expect_err("inherited NOT NULL rejects NULL")
            )
            .code()
            .code(),
            "23502"
        );
        assert_eq!(
            db_error(
                grandchild_null
                    .as_ref()
                    .expect_err("inherited NOT NULL rejects NULL")
            )
            .code()
            .code(),
            "23502"
        );
        assert_eq!(
            inherited_not_null
                .iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, bool>(1),
                        row.get::<_, bool>(2),
                        row.get::<_, i16>(3),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("compat_constraint_child".to_owned(), true, false, 1),
                ("compat_constraint_grandchild".to_owned(), true, false, 1),
                ("compat_not_null_parent".to_owned(), true, true, 0),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_propagate_inherited_check_constraint_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_check_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_check_parent (id INT \
                   CONSTRAINT compat_positive_id CHECK (id > 0))",
                "CREATE TABLE compat_check_child (child_note TEXT) \
                   INHERITS (compat_check_parent)",
                "CREATE TABLE compat_check_grandchild (leaf INT) \
                   INHERITS (compat_check_child)",
            ],
        )
        .await
        .expect("create a three-level inherited CHECK constraint");

        // Act
        let child_check = client
            .execute(
                "INSERT INTO compat_check_child (id, child_note) VALUES (-1, 'bad')",
                &[],
            )
            .await;
        let grandchild_check = client
            .execute(
                "INSERT INTO compat_check_grandchild \
                   (id, child_note, leaf) VALUES (-2, 'bad', 7)",
                &[],
            )
            .await;
        let inherited_constraints = client
            .query(
                "SELECT relation.relname, constraint_row.conislocal, \
                        constraint_row.coninhcount, constraint_row.contype::TEXT \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = constraint_row.conrelid \
                 WHERE relation.relname IN \
                   ('compat_check_parent', 'compat_check_child', \
                    'compat_check_grandchild') \
                   AND constraint_row.conname = 'compat_positive_id' \
                 ORDER BY relation.relname",
                &[],
            )
            .await
            .expect("inspect local versus inherited CHECK constraint catalog state");

        // Assert
        assert_eq!(
            db_error(
                child_check
                    .as_ref()
                    .expect_err("inherited CHECK rejects child row")
            )
            .code()
            .code(),
            "23514"
        );
        assert_eq!(
            db_error(
                grandchild_check
                    .as_ref()
                    .expect_err("inherited CHECK rejects grandchild row")
            )
            .code()
            .code(),
            "23514"
        );
        assert_eq!(
            inherited_constraints
                .iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, bool>(1),
                        row.get::<_, i16>(2),
                        row.get::<_, String>(3),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("compat_check_child".to_owned(), false, 1, "c".to_owned()),
                (
                    "compat_check_grandchild".to_owned(),
                    false,
                    1,
                    "c".to_owned(),
                ),
                ("compat_check_parent".to_owned(), true, 0, "c".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_leave_parent_unique_constraints_off_inherited_children() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_unique_constraint_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_unique_parent (id INT, label TEXT \
                   CONSTRAINT compat_parent_label_unique UNIQUE)",
                "CREATE TABLE compat_unique_child (id INT, label TEXT, child_note TEXT)",
                "ALTER TABLE compat_unique_child INHERIT compat_unique_parent",
                "INSERT INTO compat_unique_parent VALUES (1, 'duplicate')",
            ],
        )
        .await
        .expect("create a parent unique constraint and inherited child");

        // Act
        let child_insert = client
            .execute(
                "INSERT INTO compat_unique_child VALUES \
                   (2, 'duplicate', 'first'), (3, 'duplicate', 'second')",
                &[],
            )
            .await;
        let inherited_scan = client
            .query(
                "SELECT id, label FROM compat_unique_parent ORDER BY id",
                &[],
            )
            .await
            .expect("scan parent and child rows with duplicate inherited values");
        let child_unique_constraint_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = constraint_row.conrelid \
                 WHERE relation.relname = 'compat_unique_child' \
                   AND constraint_row.conname = 'compat_parent_label_unique'",
                &[],
            )
            .await
            .expect("inspect child constraints")
            .try_get(0)
            .expect("child unique constraint count");
        let child_unique_index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_index AS index_row \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = index_row.indrelid \
                 WHERE relation.relname = 'compat_unique_child' \
                   AND index_row.indisunique",
                &[],
            )
            .await
            .expect("inspect child indexes")
            .try_get(0)
            .expect("child unique index count");

        // Assert
        assert_eq!(
            child_insert.expect("unique constraints apply to the parent only"),
            2
        );
        assert_eq!(
            inherited_scan
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "duplicate".to_owned()),
                (2, "duplicate".to_owned()),
                (3, "duplicate".to_owned()),
            ]
        );
        assert_eq!(child_unique_constraint_count, 0);
        assert_eq!(child_unique_index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_update_inherited_rows_when_mutating_parent() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_parent_update_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_update_parent \
                   (id INT PRIMARY KEY, parent_value TEXT NOT NULL)",
                "CREATE TABLE compat_update_child \
                   (id INT PRIMARY KEY, parent_value TEXT NOT NULL, child_note TEXT)",
                "ALTER TABLE compat_update_child INHERIT compat_update_parent",
                "INSERT INTO compat_update_parent VALUES (2, 'parent')",
                "INSERT INTO compat_update_child VALUES (1, 'child', 'detail')",
            ],
        )
        .await
        .expect("create matching parent and child relations and attach inheritance");

        // Act
        let inherited_update = client
            .execute(
                "UPDATE compat_update_parent \
                 SET parent_value = parent_value || '-all' WHERE id IN (1, 2)",
                &[],
            )
            .await
            .expect("update matching rows in the parent and inherited child");
        let rows = client
            .query(
                "SELECT id, parent_value FROM compat_update_parent ORDER BY id",
                &[],
            )
            .await
            .expect("read both physical relations through the inherited parent scan");

        // Assert
        assert_eq!(inherited_update, 2);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "child-all".to_owned()), (2, "parent-all".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
