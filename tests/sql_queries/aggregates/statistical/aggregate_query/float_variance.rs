#[test]
fn should_preserve_explicit_float_variance_mode_through_ordered_window() {
    // Arrange
    let fixture = sql_fixture(
        "variance-explicit-float-window",
        &[
            "CREATE TABLE vf (id INT PRIMARY KEY, v FLOAT8)",
            "INSERT INTO vf VALUES (1,1),(2,2),(3,4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, VAR_POP(v::FLOAT8) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM vf ORDER BY id",
    ).expect("ordered variance");

    // Assert
    assert_eq!(
        result.rows()[2],
        vec![Value::Int64(3), Value::Float64(1.555_555_555_555_555_6)]
    );
    assert_eq!(result.columns()[1].type_oid, 701);
}

#[test]
fn should_report_explicit_float_variance_overflow_after_filter_selection() {
    // Arrange
    let fixture = sql_fixture(
        "variance-explicit-float-overflow",
        &[
            "CREATE TABLE vo (id INT PRIMARY KEY, v FLOAT8)",
            "INSERT INTO vo VALUES (1,1e308),(2,1e308)",
        ],
    );

    // Act
    let grouped = fixture.error("SELECT VAR_POP(v::FLOAT8) FROM vo");
    let window = fixture.error("SELECT VAR_POP(v::FLOAT8) OVER (ORDER BY id) FROM vo");
    let filtered = fixture
        .execute("SELECT VAR_POP(v::FLOAT8) FILTER (WHERE id = 1) FROM vo")
        .expect("filtered variance");

    // Assert
    for error in [grouped, window] {
        assert!(
            matches!(error, cassie::app::CassieError::TypeInput(ref error) if error.sqlstate == "22003" && error.message == "value out of range: overflow")
        );
    }
    assert_eq!(filtered.rows(), &[vec![Value::Float64(0.0)]]);
    assert_eq!(filtered.columns()[0].type_oid, 701);
}
