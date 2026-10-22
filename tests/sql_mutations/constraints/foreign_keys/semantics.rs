#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

fn run_sql(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    sql: &str,
) -> Result<(), cassie::app::CassieError> {
    cassie.execute_sql(session, sql, vec![]).map(|_| ())
}

#[test]
fn should_match_float_foreign_keys_across_all_write_paths() {
    // Arrange
    let path = data_dir("foreign_key_float_numeric_equivalence");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_float_parents (id FLOAT PRIMARY KEY)",
                "CREATE TABLE fk_float_children (cid INT PRIMARY KEY, parent_id FLOAT REFERENCES fk_float_parents(id))",
                "INSERT INTO fk_float_parents VALUES (20.0)",
            ] {
                run_sql(&cassie, &session, sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
            }

            // Act
            let literal_decimal =
                run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (1, 20.0)");
            let literal_integer =
                run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (2, 20)");
            let bound_parameter = cassie.execute_sql(
                &session,
                "INSERT INTO fk_float_children VALUES (3, $1)",
                vec![Value::Float64(20.0)],
            ).map(|_| ());
            let updated_child = run_sql(
                &cassie,
                &session,
                "UPDATE fk_float_children SET parent_id = 20.0 WHERE cid = 3",
            );
            let delete_referenced_parent =
                run_sql(&cassie, &session, "DELETE FROM fk_float_parents WHERE id = 20.0");
            run_sql(&cassie, &session, "BEGIN").expect("begin transaction");
            let staged_parent = run_sql(&cassie, &session, "INSERT INTO fk_float_parents VALUES (30.0)");
            let staged_child = run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (10, 30.0)");
            let committed = run_sql(&cassie, &session, "COMMIT");
            let post_commit_child = run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (11, 30.0)");
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT cid FROM fk_float_children ORDER BY cid",
                    vec![],
                )
                .expect("read children");

            // Assert
            literal_decimal.expect("decimal literal must match the FLOAT parent key");
            literal_integer.expect("integer literal must match the FLOAT parent key");
            bound_parameter.expect("bound parameter must match the FLOAT parent key");
            updated_child.expect("updated child key must match the FLOAT parent key");
            assert!(delete_referenced_parent
                .expect_err("referenced FLOAT parent must not be deleted")
                .to_string()
                .contains("foreign key constraint"));
            staged_parent.expect("insert parent inside transaction");
            staged_child.expect("transactional child key must match staged parent key");
            committed.expect("commit transaction");
            post_commit_child.expect("committed parent key must remain referenceable");
            assert_eq!(
                rows.rows(),
                vec![
                    vec![Value::Int64(1)],
                    vec![Value::Int64(2)],
                    vec![Value::Int64(3)],
                    vec![Value::Int64(10)],
                    vec![Value::Int64(11)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_insert_when_foreign_key_parent_is_missing() {
    // Arrange
    let path = data_dir("foreign_key_missing_parent");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_children (parent_id INT REFERENCES fk_parents(id), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_children (parent_id, title) VALUES (1, 'child')",
                vec![],
            )
            .unwrap();

        // Act
        let missing_parent = cassie.execute_sql(
            &session,
            "INSERT INTO fk_children (parent_id, title) VALUES (2, 'missing')",
            vec![],
        );

        // Assert
        assert!(missing_parent.is_err());
        assert!(missing_parent
            .unwrap_err()
            .to_string()
            .contains("foreign key constraint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_parent_mutation_when_foreign_key_children_exist() {
    // Arrange
    let path = data_dir("foreign_key_referenced_parent");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_children (parent_id INT REFERENCES fk_parents(id), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_children (parent_id, title) VALUES (1, 'child')",
                vec![],
            )
            .unwrap();

        // Act
        let delete_parent = cassie.execute_sql(
            &session,
            "DELETE FROM fk_parents WHERE title = 'alpha'",
            vec![],
        );
        let update_parent = cassie.execute_sql(
            &session,
            "UPDATE fk_parents SET id = 2 WHERE title = 'alpha'",
            vec![],
        );
        let constraints = cassie
            .execute_sql(
                &session,
                "SELECT constraint_type FROM information_schema.table_constraints WHERE table_name = 'fk_children' ORDER BY constraint_type",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(delete_parent.is_err());
        assert!(delete_parent
            .unwrap_err()
            .to_string()
            .contains("foreign key constraint"));
        assert!(update_parent.is_err());
        assert!(update_parent
            .unwrap_err()
            .to_string()
            .contains("foreign key constraint"));
        assert!(constraints.rows().iter().any(|row| {
            row == &vec![Value::String("FOREIGN KEY".to_string())]
        }));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_foreign_key_delete_actions() {
    // Arrange
    let path = data_dir("foreign_key_delete_actions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_delete_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_delete_cascade_children (parent_id INT, title TEXT, CONSTRAINT fk_delete_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_delete_parents(id) ON DELETE CASCADE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_delete_null_children (parent_id INT, title TEXT, CONSTRAINT fk_delete_null_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_delete_parents(id) ON DELETE SET NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_cascade_children (parent_id, title) VALUES (1, 'cascade')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_null_children (parent_id, title) VALUES (1, 'nullable')",
                vec![],
            )
            .unwrap();

        // Act
        let delete_parent = cassie
            .execute_sql(
                &session,
                "DELETE FROM fk_delete_parents WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let cascade_children = cassie
            .execute_sql(
                &session,
                "SELECT title FROM fk_delete_cascade_children",
                vec![],
            )
            .unwrap();
        let null_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_delete_null_children",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(delete_parent.command(), "DELETE 1");
        assert_eq!(cascade_children.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert_eq!(
            null_children.rows(),
            vec![vec![
                Value::Null,
                Value::String("nullable".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_foreign_key_update_actions() {
    // Arrange
    let path = data_dir("foreign_key_update_actions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_update_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_update_cascade_children (parent_id INT, title TEXT, CONSTRAINT fk_update_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_update_parents(id) ON UPDATE CASCADE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_update_null_children (parent_id INT, title TEXT, CONSTRAINT fk_update_null_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_update_parents(id) ON UPDATE SET NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_cascade_children (parent_id, title) VALUES (1, 'cascade')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_null_children (parent_id, title) VALUES (1, 'nullable')",
                vec![],
            )
            .unwrap();

        // Act
        let update_parent = cassie
            .execute_sql(
                &session,
                "UPDATE fk_update_parents SET id = 2 WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let cascade_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_update_cascade_children",
                vec![],
            )
            .unwrap();
        let null_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_update_null_children",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(update_parent.command(), "UPDATE 1");
        assert_eq!(
            cascade_children.rows(),
            vec![vec![
                Value::Int64(2),
                Value::String("cascade".to_string())
            ]]
        );
        assert_eq!(
            null_children.rows(),
            vec![vec![
                Value::Null,
                Value::String("nullable".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cascade_referenced_key_update_through_a_two_level_chain() {
    // Arrange
    let path = data_dir("foreign_key_update_cascade_two_levels");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_chain_a (code TEXT PRIMARY KEY)",
                "CREATE TABLE fk_chain_b (code TEXT PRIMARY KEY, CONSTRAINT fk_chain_b_a FOREIGN KEY (code) REFERENCES fk_chain_a(code) ON UPDATE CASCADE)",
                "CREATE TABLE fk_chain_c (id INT PRIMARY KEY, bcode TEXT, CONSTRAINT fk_chain_c_b FOREIGN KEY (bcode) REFERENCES fk_chain_b(code) ON UPDATE CASCADE)",
                "INSERT INTO fk_chain_a VALUES ('x')",
                "INSERT INTO fk_chain_b VALUES ('x')",
                "INSERT INTO fk_chain_c VALUES (1, 'x')",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare two-level foreign key chain");
            }

            // Act
            cassie
                .execute_sql(&session, "UPDATE fk_chain_a SET code = 'y'", vec![])
                .expect("update root key");
            let grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read cascaded grandchild key");
            cassie
                .execute_sql(&session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .execute_sql(&session, "UPDATE fk_chain_a SET code = 'z'", vec![])
                .expect("update root key in transaction");
            let staged_grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read staged cascaded grandchild key");
            cassie
                .execute_sql(&session, "COMMIT", vec![])
                .expect("commit recursive cascade");
            let committed_grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read committed cascaded grandchild key");

            // Assert
            assert_eq!(
                grandchild.rows(),
                vec![vec![Value::String("y".to_string())]],
                "a cascaded child update must recurse to its own dependents"
            );
            assert_eq!(
                staged_grandchild.rows(),
                vec![vec![Value::String("z".to_string())]],
                "recursive cascades must remain visible in the updating transaction"
            );
            assert_eq!(
                committed_grandchild.rows(),
                vec![vec![Value::String("z".to_string())]],
                "the transaction must commit the entire recursive cascade"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_propagate_set_null_after_parent_key_update() {
    // Arrange
    let path = data_dir("foreign_key_set_null_two_levels");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_setnull_update_a (id INT PRIMARY KEY)",
                "CREATE TABLE fk_setnull_update_b (id INT PRIMARY KEY, code INT UNIQUE, CONSTRAINT fk_setnull_update_b_a FOREIGN KEY (code) REFERENCES fk_setnull_update_a(id) ON UPDATE SET NULL)",
                "CREATE TABLE fk_setnull_update_c (id INT PRIMARY KEY, code INT, CONSTRAINT fk_setnull_update_c_b FOREIGN KEY (code) REFERENCES fk_setnull_update_b(code) ON UPDATE SET NULL)",
                "INSERT INTO fk_setnull_update_a VALUES (1)",
                "INSERT INTO fk_setnull_update_b VALUES (10, 1)",
                "INSERT INTO fk_setnull_update_c VALUES (100, 1)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare SET NULL foreign key chains");
            }

            // Act
            cassie
                .execute_sql(
                    &session,
                    "UPDATE fk_setnull_update_a SET id = 2",
                    vec![],
                )
                .expect("update parent with SET NULL action");
            let updated_grandchild = cassie
                .execute_sql(&session, "SELECT code FROM fk_setnull_update_c", vec![])
                .expect("read update grandchild");

            // Assert
            assert_eq!(updated_grandchild.rows(), vec![vec![Value::Null]]);

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_propagate_set_null_after_parent_delete() {
    // Arrange
    let path = data_dir("foreign_key_delete_set_null_two_levels");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_setnull_delete_a (id INT PRIMARY KEY)",
                "CREATE TABLE fk_setnull_delete_b (id INT PRIMARY KEY, code INT UNIQUE, CONSTRAINT fk_setnull_delete_b_a FOREIGN KEY (code) REFERENCES fk_setnull_delete_a(id) ON DELETE SET NULL)",
                "CREATE TABLE fk_setnull_delete_c (id INT PRIMARY KEY, code INT, CONSTRAINT fk_setnull_delete_c_b FOREIGN KEY (code) REFERENCES fk_setnull_delete_b(code) ON UPDATE SET NULL)",
                "INSERT INTO fk_setnull_delete_a VALUES (1)",
                "INSERT INTO fk_setnull_delete_b VALUES (10, 1)",
                "INSERT INTO fk_setnull_delete_c VALUES (100, 1)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare SET NULL foreign key chain");
            }

            // Act
            cassie
                .execute_sql(&session, "DELETE FROM fk_setnull_delete_a", vec![])
                .expect("delete parent with SET NULL action");
            let grandchild = cassie
                .execute_sql(&session, "SELECT code FROM fk_setnull_delete_c", vec![])
                .expect("read delete grandchild");

            // Assert
            assert_eq!(grandchild.rows(), vec![vec![Value::Null]]);

            let _ = std::fs::remove_dir_all(path);
        });
}
