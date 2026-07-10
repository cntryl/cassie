use cassie::app::Cassie;

#[path = "support/sql.rs"]
mod support;

#[test]
fn should_not_commit_an_orphaned_child_during_concurrent_parent_delete() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("foreign_key_concurrent_parent_delete");
    let cassie = std::sync::Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE fk_race_parents (id INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE fk_race_children (parent_id INT REFERENCES fk_race_parents(id))",
            vec![],
        )
        .expect("create child table");

    for attempt in 0..32 {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_race_parents (id) VALUES (1)",
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
            delete_cassie.execute_sql(&session, "DELETE FROM fk_race_parents WHERE id = 1", vec![])
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
