#[test]
fn should_preserve_declared_type_identity_with_typmod_after_restart() {
    // Arrange
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("issue-904-type-catalog-restart");
    let _directory = TypeCatalogDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_904_persisted_type (amount NUMERIC(5, 2))",
                vec![],
            )
            .expect("persisted numeric column");
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let selected = cassie
        .execute_sql(
            &session,
            "SELECT atttypid, atttypmod FROM pg_attribute WHERE attrelid = 'issue_904_persisted_type' AND attname = 'amount'",
            vec![],
        )
        .expect("persisted type descriptor");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1_700), Value::Int64(327_686)]]
    );
}
