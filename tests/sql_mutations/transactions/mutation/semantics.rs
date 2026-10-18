#[test]
fn should_expose_updated_cte_rows_beside_the_original_statement_snapshot() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-update-cte-snapshot-wave4",
        &[
            "CREATE TABLE issue_834_update_snapshot_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_snapshot_wave4 VALUES (1, 'old'), (2, 'keep')",
        ],
    );

    // Act
    let observed = fixture.execute(
        "WITH changed AS ( \
             UPDATE issue_834_update_snapshot_wave4 SET payload = 'new' WHERE id = 1 \
             RETURNING id, payload \
         ), snapshot AS ( \
             SELECT id, payload FROM issue_834_update_snapshot_wave4 \
         ) \
         SELECT 'changed' AS source, id, payload FROM changed \
         UNION ALL \
         SELECT 'snapshot', id, payload FROM snapshot \
         ORDER BY source, id",
    );
    let stored =
        fixture.execute("SELECT id, payload FROM issue_834_update_snapshot_wave4 ORDER BY id");

    // Assert
    assert_eq!(
        observed
            .expect("read UPDATE RETURNING and sibling statement snapshot")
            .rows(),
        &[
            vec![
                Value::String("changed".to_owned()),
                Value::Int64(1),
                Value::String("new".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(1),
                Value::String("old".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(2),
                Value::String("keep".to_owned()),
            ],
        ]
    );
    assert_eq!(
        stored.expect("read table after UPDATE CTE").rows(),
        &[
            vec![Value::Int64(1), Value::String("new".to_owned())],
            vec![Value::Int64(2), Value::String("keep".to_owned())],
        ]
    );
}
