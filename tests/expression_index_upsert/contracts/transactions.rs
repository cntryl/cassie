#[test]
fn should_ignore_expression_reservation_owners_deleted_or_rekeyed_in_the_current_transaction() {
    // Arrange
    let (cassie, session, path) = start("expression-index-rekey-owner");
    run(
        &cassie,
        &session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_lower ON {TABLE} ((lower(email)))"),
    );
    run(
        &cassie,
        &session,
        &format!(
            "INSERT INTO {TABLE} (id, email) VALUES \
             (1, 'owner@example.com'), (2, 'deleted@example.com')"
        ),
    );
    run(&cassie, &session, "BEGIN");

    // Act
    run(
        &cassie,
        &session,
        &format!("UPDATE {TABLE} SET email = 'new@example.com' WHERE id = 1"),
    );
    run(
        &cassie,
        &session,
        &format!("DELETE FROM {TABLE} WHERE id = 2"),
    );
    run(
        &cassie,
        &session,
        &format!(
            "INSERT INTO {TABLE} (id, email) VALUES (2, 'OWNER@example.com') ON CONFLICT DO NOTHING"
        ),
    );
    run(
        &cassie,
        &session,
        &format!(
            "INSERT INTO {TABLE} (id, email) VALUES (3, 'DELETED@example.com') ON CONFLICT DO NOTHING"
        ),
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        rows(
            &cassie,
            &session,
            &format!("SELECT id, email FROM {TABLE} ORDER BY id")
        ),
        vec![
            vec![
                Value::Int64(1),
                Value::String("new@example.com".to_string())
            ],
            vec![
                Value::Int64(2),
                Value::String("OWNER@example.com".to_string())
            ],
            vec![
                Value::Int64(3),
                Value::String("DELETED@example.com".to_string())
            ],
        ]
    );
    cleanup(cassie, path);
}

#[test]
fn should_check_staged_expression_reservation_owner_in_the_current_transaction() {
    // Arrange
    let (cassie, session, path) = start("expression-index-staged-owner");
    run(
        &cassie,
        &session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_lower ON {TABLE} ((lower(email)))"),
    );
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (1, 'owner@example.com')"),
    );

    // Act
    run(
        &cassie,
        &session,
        &format!(
            "INSERT INTO {TABLE} (id, email) VALUES (2, 'OWNER@example.com') ON CONFLICT DO NOTHING"
        ),
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        rows(&cassie, &session, &format!("SELECT id FROM {TABLE}")),
        vec![vec![Value::Int64(1)]]
    );
    cleanup(cassie, path);
}
