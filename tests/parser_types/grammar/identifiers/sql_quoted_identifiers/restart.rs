#[test]
fn should_preserve_case_distinct_column_names_across_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("quoted_ident_case_distinct_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        run(
            &cassie,
            &session,
            "CREATE TABLE persisted_case_distinct (\"a\" INT, \"A\" INT)",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO persisted_case_distinct (\"a\", \"A\") VALUES (11, 22)",
        );
    }

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("restart Cassie");
    let session = restarted.create_session("tester", None);
    let lower_exact = run(
        &restarted,
        &session,
        "SELECT \"a\" FROM persisted_case_distinct",
    );
    let upper_exact = run(
        &restarted,
        &session,
        "SELECT \"A\" FROM persisted_case_distinct",
    );
    let unquoted_upper = run(
        &restarted,
        &session,
        "SELECT A FROM persisted_case_distinct",
    );

    // Assert
    assert_eq!(lower_exact.rows(), vec![vec![Value::Int64(11)]]);
    assert_eq!(upper_exact.rows(), vec![vec![Value::Int64(22)]]);
    assert_eq!(unquoted_upper.rows(), vec![vec![Value::Int64(11)]]);

    let _ = std::fs::remove_dir_all(path);
}
