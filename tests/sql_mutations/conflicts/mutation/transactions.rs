#[test]
fn should_rollback_update_cte_when_its_outer_insert_hits_a_unique_conflict() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-update-cte-atomicity-wave4",
        &[
            "CREATE TABLE issue_834_update_cte_source_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "CREATE TABLE issue_834_update_cte_target_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_cte_source_wave4 VALUES (1, 'source-one'), (2, 'source-two')",
            "INSERT INTO issue_834_update_cte_target_wave4 VALUES (2, 'existing-target')",
        ],
    );

    // Act
    let failed = fixture.execute(
        "WITH changed AS ( \
             UPDATE issue_834_update_cte_source_wave4 SET payload = 'updated' \
             RETURNING id, payload \
         ) \
         INSERT INTO issue_834_update_cte_target_wave4 (id, payload) \
         SELECT id, payload FROM changed ORDER BY id",
    );
    let source =
        fixture.execute("SELECT id, payload FROM issue_834_update_cte_source_wave4 ORDER BY id");
    let target =
        fixture.execute("SELECT id, payload FROM issue_834_update_cte_target_wave4 ORDER BY id");

    // Assert
    assert!(
        matches!(failed, Err(CassieError::UniqueViolation { .. })),
        "outer INSERT must report the 23505 duplicate key, got {failed:?}"
    );
    assert_eq!(
        source
            .expect("read source after rejected outer INSERT")
            .rows(),
        &[
            vec![Value::Int64(1), Value::String("source-one".to_owned())],
            vec![Value::Int64(2), Value::String("source-two".to_owned())],
        ],
        "the UPDATE CTE must roll back with the failing outer INSERT"
    );
    assert_eq!(
        target
            .expect("read target after rejected outer INSERT")
            .rows(),
        &[vec![
            Value::Int64(2),
            Value::String("existing-target".to_owned()),
        ]]
    );
}
