use cassie::app::Cassie;
use cassie::sql::ast::{CopyFormat, CopyStatement};

use super::support_sql as support;

#[test]
fn should_not_commit_an_orphaned_child_during_concurrent_parent_delete() {
    // Arrange
    let path = support::data_dir("foreign_key_concurrent_parent_delete");
    let cassie =
        std::sync::Arc::new(crate::support_sql::new_in_memory_cassie().expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE fk_race_parents (pid INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE fk_race_children (parent_id INT REFERENCES fk_race_parents(pid))",
            vec![],
        )
        .expect("create child table");

    for attempt in 0..32 {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_race_parents (pid) VALUES (1)",
                vec![],
            )
            .expect("insert parent");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let insert_cassie = std::sync::Arc::clone(&cassie);
        let insert_barrier = std::sync::Arc::clone(&barrier);
        let child_insert = std::thread::spawn(move || {
            let session = insert_cassie.create_session("tester", None);
            insert_barrier.wait();
            insert_cassie.execute_sql(
                &session,
                "INSERT INTO fk_race_children (parent_id) VALUES (1)",
                vec![],
            )
        });
        let delete_cassie = std::sync::Arc::clone(&cassie);
        let delete_barrier = std::sync::Arc::clone(&barrier);
        let parent_delete = std::thread::spawn(move || {
            let session = delete_cassie.create_session("tester", None);
            delete_barrier.wait();
            delete_cassie.execute_sql(
                &session,
                "DELETE FROM fk_race_parents WHERE pid = 1",
                vec![],
            )
        });
        barrier.wait();

        // Act
        let child_result = child_insert.join().expect("child insert worker completed");
        let parent_result = parent_delete
            .join()
            .expect("parent delete worker completed");
        let parent_collection = support::canonical_test_collection(&cassie, "fk_race_parents");
        let child_collection = support::canonical_test_collection(&cassie, "fk_race_children");
        let parents = cassie
            .midge
            .scan_documents(&parent_collection)
            .expect("scan parents");
        let children = cassie
            .midge
            .scan_documents(&child_collection)
            .expect("scan children");

        // Assert
        let no_rows_remain = parents.is_empty() && children.is_empty();
        let parent_and_child_remain = parents.len() == 1 && children.len() == 1;
        assert!(
            no_rows_remain || parent_and_child_remain,
            "attempt {attempt} committed an orphaned child: child={child_result:?}, parent={parent_result:?}"
        );

        cassie
            .execute_sql(&session, "DELETE FROM fk_race_children", vec![])
            .expect("clear children");
        cassie
            .execute_sql(&session, "DELETE FROM fk_race_parents", vec![])
            .expect("clear parents");
    }

    let _ = std::fs::remove_dir_all(path);
}

fn copy_child_statement() -> CopyStatement {
    CopyStatement {
        table: "fk_copy_children".to_string(),
        columns: vec!["parent_pid".to_string()],
        format: CopyFormat::Csv,
        header: false,
    }
}

fn create_copy_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for sql in [
        "CREATE TABLE fk_copy_parents (pid INT PRIMARY KEY)",
        "CREATE TABLE fk_copy_children (parent_pid INT REFERENCES fk_copy_parents(pid))",
        "INSERT INTO fk_copy_parents (pid) VALUES (1)",
    ] {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("prepare tables");
    }
}

fn staged_parent_change_commit(
    name: &str,
    parent_changes: &[&str],
    concurrent_child: fn(&Cassie, &cassie::app::CassieSession) -> Result<(), String>,
) -> (
    Result<String, cassie::app::CassieError>,
    usize,
    usize,
    String,
) {
    let path = support::data_dir(name);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    create_copy_tables(&cassie, &session);
    let writer = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    for parent_change in parent_changes {
        cassie
            .execute_sql(&session, parent_change, vec![])
            .expect("stage parent change without children");
    }
    concurrent_child(&cassie, &writer).expect("concurrent child write sees the parent");
    let commit = cassie
        .execute_sql(&session, "COMMIT", vec![])
        .map(|result| result.command().to_owned());
    let parents = cassie
        .midge
        .scan_documents(&support::canonical_test_collection(
            &cassie,
            "fk_copy_parents",
        ))
        .expect("scan parents")
        .into_iter()
        .filter(|parent| parent.payload.get("pid") == Some(&serde_json::json!(1)))
        .count();
    let children = cassie
        .midge
        .scan_documents(&support::canonical_test_collection(
            &cassie,
            "fk_copy_children",
        ))
        .expect("scan children")
        .len();
    (commit, parents, children, path)
}

fn insert_child(cassie: &Cassie, session: &cassie::app::CassieSession) -> Result<(), String> {
    cassie
        .execute_sql(
            session,
            "INSERT INTO fk_copy_children (parent_pid) VALUES (1)",
            vec![],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn copy_child(cassie: &Cassie, session: &cassie::app::CassieSession) -> Result<(), String> {
    cassie
        .copy_from_csv_stdin(session, &copy_child_statement(), b"1\n")
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn assert_parent_change_rejected(
    commit: &Result<String, cassie::app::CassieError>,
    parents: usize,
    children: usize,
) {
    assert!(
        matches!(
            commit,
            Err(cassie::app::CassieError::ForeignKeyViolation { .. })
        ),
        "commit orphaning a concurrent child was not rejected with 23503: {commit:?}"
    );
    assert_eq!(
        parents, 1,
        "the referenced parent key must survive the rollback"
    );
    assert_eq!(children, 1, "the concurrently committed child must remain");
}

#[test]
fn should_reject_a_parent_delete_commit_after_a_concurrent_child_insert() {
    // Arrange
    let (commit, parents, children, path) = staged_parent_change_commit(
        "foreign_key_staged_parent_delete_insert",
        &["DELETE FROM fk_copy_parents WHERE pid = 1"],
        insert_child,
    );

    // Act
    let rejected = commit.is_err();

    // Assert
    assert!(rejected, "COMMIT unexpectedly succeeded");
    assert_parent_change_rejected(&commit, parents, children);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_a_parent_delete_commit_after_a_concurrent_child_copy() {
    // Arrange
    let (commit, parents, children, path) = staged_parent_change_commit(
        "foreign_key_staged_parent_delete_copy",
        &["DELETE FROM fk_copy_parents WHERE pid = 1"],
        copy_child,
    );

    // Act
    let rejected = commit.is_err();

    // Assert
    assert!(rejected, "COMMIT unexpectedly succeeded");
    assert_parent_change_rejected(&commit, parents, children);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_a_parent_key_update_commit_after_a_concurrent_child_insert() {
    // Arrange
    let (commit, parents, children, path) = staged_parent_change_commit(
        "foreign_key_staged_parent_key_update_insert",
        &["UPDATE fk_copy_parents SET pid = 2 WHERE pid = 1"],
        insert_child,
    );

    // Act
    let rejected = commit.is_err();

    // Assert
    assert!(rejected, "COMMIT unexpectedly succeeded");
    assert_parent_change_rejected(&commit, parents, children);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_a_copy_whose_middle_row_references_a_missing_parent() {
    // Arrange
    let path = support::data_dir("foreign_key_copy_missing_parent");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    create_copy_tables(&cassie, &session);

    // Act
    let result = cassie.copy_from_csv_stdin(&session, &copy_child_statement(), b"1\n2\n1\n");

    // Assert
    let child_collection = support::canonical_test_collection(&cassie, "fk_copy_children");
    let children = cassie
        .midge
        .scan_documents(&child_collection)
        .expect("scan children");
    assert!(
        matches!(
            result,
            Err(cassie::app::CassieError::ForeignKeyViolation { .. })
        ),
        "COPY with a missing parent was not rejected: {result:?}"
    );
    assert!(children.is_empty(), "COPY applied child rows: {children:?}");

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_a_transactional_copy_commit_after_its_parent_is_deleted() {
    // Arrange
    let path = support::data_dir("foreign_key_transactional_copy_parent_delete");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    create_copy_tables(&cassie, &session);
    let deleter = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .copy_from_csv_stdin(&session, &copy_child_statement(), b"1\n")
        .expect("stage child copy");
    cassie
        .execute_sql(
            &deleter,
            "DELETE FROM fk_copy_parents WHERE pid = 1",
            vec![],
        )
        .expect("delete parent outside the transaction");

    // Act
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);

    // Assert
    let child_collection = support::canonical_test_collection(&cassie, "fk_copy_children");
    let children = cassie
        .midge
        .scan_documents(&child_collection)
        .expect("scan children");
    assert!(
        matches!(
            commit,
            Err(cassie::app::CassieError::ForeignKeyViolation { .. })
        ),
        "commit of an orphaned COPY row was not rejected: {:?}",
        commit.map(|result| result.command().to_owned())
    );
    assert!(
        children.is_empty(),
        "orphaned child committed: {children:?}"
    );

    let _ = std::fs::remove_dir_all(path);
}
