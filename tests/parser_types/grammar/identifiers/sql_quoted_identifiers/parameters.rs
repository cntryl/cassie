#[test]
fn should_bind_a_parameter_against_a_quoted_indexed_column() {
    with_users_table("quoted_ident_indexed_param", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE INDEX users_id_idx ON users (id)");
        let sql = "SELECT \"name\" FROM users WHERE \"id\" = $1";

        // Act
        let selected = match cassie.execute_sql(session, sql, vec![Value::Int64(2)]) {
            Ok(result) => result,
            Err(error) => panic!("statement failed: {sql}: {error}"),
        };

        // Assert
        assert_eq!(selected.rows(), string_rows(&["bob"]));
    });
}
