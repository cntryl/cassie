#[test]
fn should_resolve_expression_index_conflicts_without_scanning_rows_per_input_row() {
    // Arrange
    let (cassie, session, path) = start("expression-index-conflict-scan");
    seed_expression_index(&cassie, &session, ROW_COUNT);
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    let sql = format!(
        "INSERT INTO {TABLE} (id, email) VALUES \
         (1001, 'USER-0001@example.com'), \
         (1002, 'USER-0002@example.com'), \
         (1003, 'USER-0003@example.com'), \
         (1004, 'USER-0004@example.com') ON CONFLICT DO NOTHING"
    );

    // Act
    run(&cassie, &session, &sql);
    let scanned_entries = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(
        scanned_entries < ROW_COUNT as u64,
        "four exact reservation lookups must not scan all {ROW_COUNT} table rows; observed {scanned_entries} scanned entries"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            &format!("SELECT id FROM {TABLE} WHERE id >= 1000")
        ),
        [] as [Vec<Value>; 0]
    );
    cleanup(cassie, path);
}

#[test]
fn should_match_a_unique_conflict_index_with_its_explicit_operator_class() {
    // Arrange
    let (cassie, session, path) = start("explicit-conflict-operator-class");
    run(
        &cassie,
        &session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_email ON {TABLE} (email)"),
    );
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (1, 'alpha@example.com')"),
    );

    // Act
    let updated = cassie
        .execute_sql(
            &session,
            &format!(
                "INSERT INTO {TABLE} (id, email) VALUES (2, 'alpha@example.com') \
                 ON CONFLICT (email text_ops) DO UPDATE SET id = excluded.id \
                 RETURNING id, email"
            ),
            vec![],
        )
        .expect("infer the unique index using its explicit operator class");
    let stored = rows(&cassie, &session, &format!("SELECT id, email FROM {TABLE}"));

    // Assert
    assert_eq!(
        updated.rows(),
        &[vec![
            Value::Int64(2),
            Value::String("alpha@example.com".to_owned()),
        ]]
    );
    assert_eq!(
        stored,
        vec![vec![
            Value::Int64(2),
            Value::String("alpha@example.com".to_owned()),
        ]]
    );
    cleanup(cassie, path);
}
