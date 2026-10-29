use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

use super::support_sql as support;

const PARENT_CHILD: [&str; 4] = [
        "CREATE TABLE p (id INT PRIMARY KEY, tag TEXT)",
        "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT cfk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE RESTRICT)",
        "INSERT INTO p (id, tag) VALUES (1, 'a')",
        "INSERT INTO c (cid, pid) VALUES (10, 1)",
    ];

fn with_cassie(label: &str, test: impl FnOnce(&str)) {
    let path = support::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async { test(&path) });
    let _ = std::fs::remove_dir_all(&path);
}

fn start(_path: &str) -> (Cassie, CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn start_local(path: &str) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_with_data_dir(path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<QueryResult, CassieError> {
    cassie.execute_sql(session, sql, vec![])
}

fn exec_all(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for sql in statements {
        run(cassie, session, sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    run(cassie, session, sql).expect("select").rows().to_vec()
}

fn assert_foreign_key_error(result: Result<QueryResult, CassieError>, context: &str) {
    let error = result.expect_err(context).to_string();
    assert!(
        error.contains("foreign key"),
        "{context}: unexpected error {error}"
    );
}

#[test]
fn should_enforce_foreign_key_when_table_constraint_column_case_differs() {
    with_cassie("fk-ddl-column-case", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT cfk FOREIGN KEY (PID) REFERENCES p(ID) ON DELETE RESTRICT)",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (2, 1)",
                ],
            );

        // Act
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (1, 999)",
        );
        let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE id = 1");
        let key_columns = rows(
                &cassie,
                &session,
                "SELECT column_name FROM information_schema.key_column_usage WHERE constraint_name = 'cfk'",
            );

        // Assert
        assert_foreign_key_error(orphan_insert, "orphan child insert");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        assert_eq!(key_columns, vec![vec![Value::String("pid".to_string())]]);
    });
}

#[test]
fn should_reject_dropping_a_table_that_a_foreign_key_still_references() {
    with_cassie("fk-ddl-drop-referenced-table", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(&cassie, &session, &PARENT_CHILD);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE tree (id INT PRIMARY KEY, parent INT, CONSTRAINT tree_fk FOREIGN KEY (parent) REFERENCES tree(id))",
                    "INSERT INTO tree (id, parent) VALUES (1, NULL)",
                ],
            );

        // Act
        let referenced_drop = run(&cassie, &session, "DROP TABLE p");
        let child_insert = run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)");
        let self_referencing_drop = run(&cassie, &session, "DROP TABLE tree");
        run(&cassie, &session, "ALTER TABLE c DROP CONSTRAINT cfk").expect("drop fk");
        let released_drop = run(&cassie, &session, "DROP TABLE p");

        // Assert
        assert_foreign_key_error(referenced_drop, "drop of a referenced parent");
        assert!(child_insert.is_ok(), "the child still resolves its parent");
        assert!(
            self_referencing_drop.is_ok(),
            "a self-referencing table can be dropped"
        );
        assert!(released_drop.is_ok(), "the parent is no longer referenced");
        assert!(!cassie.catalog.exists("p"));
    });
}

#[test]
fn should_restore_inbound_foreign_key_after_table_cascade_rollback() {
    with_cassie("fk-ddl-drop-table-cascade-rollback", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        run(&cassie, &session, "BEGIN").expect("begin cascade rollback");
        run(&cassie, &session, "DROP TABLE p CASCADE").expect("stage cascade drop");
        run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (11, 999)",
        )
        .expect("cascade removes the referencing constraint in the transaction");
        run(&cassie, &session, "ROLLBACK").expect("rollback cascade drop");

        // Assert
        assert!(cassie.catalog.exists("p"), "rollback restores the parent");
        assert_foreign_key_error(
            run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (12, 999)",
            ),
            "rollback restores the inbound foreign key",
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT cid, pid FROM c ORDER BY cid"),
            vec![vec![Value::Int64(10), Value::Int64(1)]]
        );
    });
}

#[test]
fn should_preserve_inbound_foreign_key_removal_after_table_cascade_restart() {
    with_cassie("fk-ddl-drop-table-cascade-restart", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        run(&cassie, &session, "BEGIN").expect("begin committed cascade");
        run(&cassie, &session, "DROP TABLE p CASCADE").expect("stage committed cascade");
        run(&cassie, &session, "COMMIT").expect("commit cascade drop");
        drop(cassie);
        let (cassie, session) = start_local(path);
        run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (13, 999)",
        )
        .expect("committed cascade removes the inbound foreign key after restart");

        // Assert
        assert!(
            !cassie.catalog.exists("p"),
            "parent remains dropped after restart"
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT cid, pid FROM c ORDER BY cid"),
            vec![
                vec![Value::Int64(10), Value::Int64(1)],
                vec![Value::Int64(13), Value::Int64(999)],
            ]
        );
    });
}

#[test]
fn should_enforce_foreign_key_added_by_alter_table_when_column_case_differs() {
    with_cassie("fk-ddl-alter-column-case", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT)",
                    "ALTER TABLE c ADD CONSTRAINT cfk FOREIGN KEY (PID) REFERENCES p(ID) ON DELETE RESTRICT",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (2, 1)",
                ],
            );

        // Act
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (1, 999)",
        );
        let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE id = 1");

        // Assert
        assert_foreign_key_error(orphan_insert, "orphan child insert");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
    });
}

#[test]
fn should_reject_dropping_a_column_that_a_foreign_key_references() {
    with_cassie("fk-ddl-drop-referenced", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        let drop_column = run(&cassie, &session, "ALTER TABLE p DROP COLUMN id");
        let restricted_delete = run(&cassie, &session, "DELETE FROM p");

        // Assert
        let error = drop_column.expect_err("referenced column drop");
        assert!(
            matches!(
                error,
                CassieError::Diagnostic(ref diagnostic)
                    if diagnostic.sql_state == "2BP01"
                        && diagnostic
                            .detail
                            .as_deref()
                            .is_some_and(|detail| detail.contains("cfk"))
            ),
            "unexpected error {error:?}"
        );
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        assert_eq!(
            rows(&cassie, &session, "SELECT id FROM p"),
            vec![vec![Value::Int64(1)]]
        );
    });
}

#[test]
fn should_drop_foreign_key_with_the_child_column_it_is_declared_on() {
    with_cassie("fk-ddl-drop-child", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        run(&cassie, &session, "ALTER TABLE c DROP COLUMN pid").expect("drop child column");
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        let foreign_keys = rows(
                &cassie,
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE constraint_type = 'FOREIGN KEY'",
            );
        assert!(foreign_keys.is_empty(), "stale constraint {foreign_keys:?}");
        run(&cassie, &session, "DELETE FROM p WHERE id = 1").expect("parent is unreferenced");
    });
}

#[test]
fn should_carry_foreign_key_when_the_referenced_column_is_renamed() {
    with_cassie("fk-ddl-rename-column", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        run(
            &cassie,
            &session,
            "ALTER TABLE p RENAME COLUMN id TO key_id",
        )
        .expect("rename");
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE key_id = 1");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
            .expect("child of existing parent");
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (12, 999)",
        );
        assert_foreign_key_error(orphan_insert, "orphan child insert");
    });
}

#[test]
fn should_carry_foreign_key_when_the_referenced_table_is_renamed() {
    with_cassie("fk-ddl-rename-table", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        run(&cassie, &session, "ALTER TABLE p RENAME TO p_new").expect("rename");
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        let restricted_delete = run(&cassie, &session, "DELETE FROM p_new WHERE id = 1");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
            .expect("child of existing parent");
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (12, 999)",
        );
        assert_foreign_key_error(orphan_insert, "orphan child insert");
    });
}

fn create_schema_move_foreign_key_fixture(cassie: &Cassie, session: &CassieSession) {
    exec_all(
        cassie,
        session,
        &[
            "CREATE SCHEMA move_source",
            "CREATE SCHEMA move_target",
            "CREATE TABLE move_source.p (id INT PRIMARY KEY)",
            "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT cfk FOREIGN KEY (pid) REFERENCES move_source.p(id) ON DELETE RESTRICT)",
            "INSERT INTO move_source.p (id) VALUES (1)",
            "INSERT INTO c (cid, pid) VALUES (10, 1)",
        ],
    );
}

#[test]
fn should_restore_foreign_key_to_source_after_referenced_table_schema_move_rollback() {
    with_cassie("fk-ddl-schema-move-rollback", |path| {
        // Arrange
        let (cassie, session) = start(path);
        create_schema_move_foreign_key_fixture(&cassie, &session);

        // Act
        run(&cassie, &session, "BEGIN").expect("begin schema-move rollback");
        run(
            &cassie,
            &session,
            "ALTER TABLE move_source.p SET SCHEMA move_target",
        )
        .expect("stage parent schema move");
        run(&cassie, &session, "ROLLBACK").expect("rollback parent schema move");

        // Assert
        assert!(cassie.catalog.exists("move_source.p"));
        assert!(!cassie.catalog.exists("move_target.p"));
        run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
            .expect("source-schema parent remains referenced");
        assert_foreign_key_error(
            run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (12, 999)",
            ),
            "rollback restores the source-schema reference",
        );
        assert_foreign_key_error(
            run(&cassie, &session, "DELETE FROM move_source.p WHERE id = 1"),
            "rollback restores the referenced parent",
        );
    });
}

#[test]
fn should_preserve_foreign_key_to_referenced_table_after_schema_move_restart() {
    with_cassie("fk-ddl-schema-move-restart", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        create_schema_move_foreign_key_fixture(&cassie, &session);

        // Act
        run(&cassie, &session, "BEGIN").expect("begin schema move");
        run(
            &cassie,
            &session,
            "ALTER TABLE move_source.p SET SCHEMA move_target",
        )
        .expect("move referenced parent schema");
        run(&cassie, &session, "COMMIT").expect("commit parent schema move");
        drop(session);
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        assert!(!cassie.catalog.exists("move_source.p"));
        assert!(cassie.catalog.exists("move_target.p"));
        run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
            .expect("restarted child retains its renamed reference");
        assert_foreign_key_error(
            run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (12, 999)",
            ),
            "restarted child rejects an orphan",
        );
        assert_foreign_key_error(
            run(&cassie, &session, "DELETE FROM move_target.p WHERE id = 1"),
            "restarted parent remains referenced",
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT cid, pid FROM c ORDER BY cid"),
            vec![
                vec![Value::Int64(10), Value::Int64(1)],
                vec![Value::Int64(11), Value::Int64(1)],
            ]
        );
    });
}

#[test]
fn should_carry_self_referencing_foreign_key_through_renames() {
    with_cassie("fk-ddl-self-reference", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE nodes (id INT PRIMARY KEY, parent INT)",
                    "ALTER TABLE nodes ADD CONSTRAINT nodes_parent_fk FOREIGN KEY (parent) REFERENCES nodes(id) ON DELETE RESTRICT",
                    "INSERT INTO nodes (id) VALUES (1)",
                    "INSERT INTO nodes (id, parent) VALUES (2, 1)",
                ],
            );

        // Act
        run(
            &cassie,
            &session,
            "ALTER TABLE nodes RENAME COLUMN id TO node_id",
        )
        .expect("rename column");
        run(&cassie, &session, "ALTER TABLE nodes RENAME TO tree").expect("rename table");
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        let restricted_delete = run(&cassie, &session, "DELETE FROM tree WHERE node_id = 1");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        run(
            &cassie,
            &session,
            "INSERT INTO tree (node_id, parent) VALUES (3, 2)",
        )
        .expect("child of existing parent");
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO tree (node_id, parent) VALUES (4, 999)",
        );
        assert_foreign_key_error(orphan_insert, "orphan child insert");
        let drop_referenced = run(&cassie, &session, "ALTER TABLE tree DROP COLUMN node_id");
        assert!(drop_referenced.is_err(), "self-referenced column drop");
    });
}

#[test]
fn should_resolve_referenced_relation_when_a_foreign_key_is_added_by_alter_table() {
    with_cassie("fk-ddl-alter-schema-reference", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
            &cassie,
            &session,
            &[
                "CREATE SCHEMA app",
                "CREATE TABLE app.p (id INT PRIMARY KEY)",
                "CREATE TABLE app.ch (cid INT PRIMARY KEY, pid INT)",
                "INSERT INTO app.p (id) VALUES (1)",
            ],
        );

        // Act
        let off_path = run(
                &cassie,
                &session,
                "ALTER TABLE app.ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE",
            );
        exec_all(
                &cassie,
                &session,
                &[
                    "SET search_path = app",
                    "ALTER TABLE app.ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE",
                    "SET search_path = public",
                ],
            );
        let child_insert = run(
            &cassie,
            &session,
            "INSERT INTO app.ch (cid, pid) VALUES (10, 1)",
        );
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO app.ch (cid, pid) VALUES (11, 999)",
        );
        let cascade_delete = run(&cassie, &session, "DELETE FROM app.p WHERE id = 1");

        // Assert
        let error = off_path
            .expect_err("reference outside search_path")
            .to_string();
        assert!(error.contains("does not exist"), "unexpected error {error}");
        child_insert.expect("child of existing parent");
        assert_foreign_key_error(orphan_insert, "orphan child insert");
        cascade_delete.expect("cascade delete");
        assert_eq!(
            rows(&cassie, &session, "SELECT cid FROM app.ch"),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );
    });
}

#[test]
fn should_reject_adding_a_foreign_key_when_existing_rows_have_no_parent() {
    with_cassie("fk-ddl-existing-orphan", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
            &cassie,
            &session,
            &[
                "CREATE TABLE p (id INT PRIMARY KEY)",
                "CREATE TABLE ch (cid INT PRIMARY KEY, pid INT)",
                "INSERT INTO p (id) VALUES (1)",
                "INSERT INTO ch (cid, pid) VALUES (9, 1)",
                "INSERT INTO ch (cid, pid) VALUES (10, 999)",
            ],
        );

        // Act
        let add_constraint = run(
            &cassie,
            &session,
            "ALTER TABLE ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id)",
        );
        let later_orphan = run(
            &cassie,
            &session,
            "INSERT INTO ch (cid, pid) VALUES (11, 999)",
        );
        let delete_first_orphan = run(&cassie, &session, "DELETE FROM ch WHERE cid = 10");
        let delete_second_orphan = run(&cassie, &session, "DELETE FROM ch WHERE cid = 11");
        let add_after_orphans_removed = run(
            &cassie,
            &session,
            "ALTER TABLE ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id)",
        );
        let orphan_after_successful_add = run(
            &cassie,
            &session,
            "INSERT INTO ch (cid, pid) VALUES (12, 999)",
        );
        let child_rows = rows(&cassie, &session, "SELECT cid, pid FROM ch ORDER BY cid");

        // Assert
        assert_foreign_key_error(add_constraint, "adding foreign key over an orphan row");
        later_orphan.expect("failed DDL must not persist the foreign key");
        delete_first_orphan.expect("remove first orphan");
        delete_second_orphan.expect("remove second orphan");
        add_after_orphans_removed.expect("add foreign key over valid existing rows");
        assert_foreign_key_error(orphan_after_successful_add, "orphan insert after DDL");
        assert_eq!(child_rows, vec![vec![Value::Int64(9), Value::Int64(1)]]);
    });
}

#[test]
fn should_report_the_declared_foreign_key_name_on_violation() {
    with_cassie("fk-ddl-declared-name", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT my_named_fk FOREIGN KEY (pid) REFERENCES p(id))",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (10, 1)",
                ],
            );

        // Act
        let child_error = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (1, 999)",
        )
        .expect_err("orphan child insert")
        .to_string();
        let parent_error = run(&cassie, &session, "DELETE FROM p WHERE id = 1")
            .expect_err("referenced parent delete")
            .to_string();

        // Assert
        assert!(
            child_error.contains("'my_named_fk'"),
            "child error {child_error}"
        );
        assert!(
            parent_error.contains("'my_named_fk'"),
            "parent error {parent_error}"
        );
    });
}

#[test]
fn should_keep_enforcing_foreign_key_when_both_referencing_columns_are_renamed() {
    with_cassie("fk-ddl-rename-both-columns", |path| {
        // Arrange
        let (cassie, session) = start_local(path);
        exec_all(&cassie, &session, &PARENT_CHILD);

        // Act
        exec_all(
            &cassie,
            &session,
            &[
                "ALTER TABLE c RENAME COLUMN PID TO parent_id",
                "ALTER TABLE p RENAME COLUMN ID TO key_id",
            ],
        );
        drop(cassie);
        let (cassie, session) = start_local(path);

        // Assert
        let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE key_id = 1");
        assert_foreign_key_error(restricted_delete, "restricted parent delete");
        run(
            &cassie,
            &session,
            "INSERT INTO c (cid, parent_id) VALUES (11, 1)",
        )
        .expect("child of existing parent");
        let orphan_insert = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, parent_id) VALUES (12, 999)",
        );
        assert_foreign_key_error(orphan_insert, "orphan child insert");
    });
}

#[test]
fn should_report_the_declared_foreign_key_name_when_a_transaction_violates_it() {
    with_cassie("fk-ddl-declared-name-transaction", |path| {
        // Arrange
        let (cassie, session) = start(path);
        exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT my_named_fk FOREIGN KEY (pid) REFERENCES p(id))",
                    "BEGIN",
                ],
            );

        // Act
        let error = run(
            &cassie,
            &session,
            "INSERT INTO c (cid, pid) VALUES (1, 999)",
        )
        .and_then(|_| run(&cassie, &session, "COMMIT"))
        .expect_err("orphan child in transaction")
        .to_string();

        // Assert
        assert!(error.contains("'my_named_fk'"), "transaction error {error}");
    });
}
