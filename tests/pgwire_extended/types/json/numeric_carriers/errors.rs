#[test]
fn should_reject_non_numeric_aggregate_carriers_with_exact_wire_errors() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_aggregate_errors (doc JSON, label TEXT, flag BOOLEAN)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_aggregate_errors (doc, label, flag) VALUES ($1, 'first', true), ($2, 'second', false), ($3, 'third', NULL)",
            vec![
                Value::Json(serde_json::json!(2)),
                Value::Json(serde_json::json!("not a number")),
                Value::Json(serde_json::json!(3)),
            ],
        ),
    ];
    let cases = [
        ("SUM(doc)", "function sum(json) does not exist"),
        ("AVG(doc)", "function avg(json) does not exist"),
        ("SUM(label)", "function sum(text) does not exist"),
        ("AVG(label)", "function avg(text) does not exist"),
        ("SUM(flag)", "function sum(boolean) does not exist"),
        ("AVG(flag)", "function avg(boolean) does not exist"),
        (
            "SUM(CAST(doc AS TEXT))",
            "function sum(text) does not exist",
        ),
        (
            "AVG(CAST(doc AS TEXT))",
            "function avg(text) does not exist",
        ),
        (
            "SUM(CAST(doc AS JSON))",
            "function sum(json) does not exist",
        ),
        (
            "AVG(CAST(doc AS JSON))",
            "function avg(json) does not exist",
        ),
    ];
    let mut cycles = Vec::new();
    for (expression, _) in cases {
        cycles.push(fixture::execute_cycle(
            &format!("SELECT {expression} FROM json_aggregate_errors"),
            None,
            0,
            false,
        ));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-json-aggregate-errors", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((expression, expected), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let frames = &pair[0];
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("42883"),
            "{expression}"
        );
        let errors = frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{expression}");
        let message = wire::parse_error_fields(&errors[0].1)
            .into_iter()
            .find_map(|(key, value)| (key == 'M').then_some(value))
            .expect("aggregate error message");
        assert_eq!(message, expected, "{expression}");
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0,
            "failed Execute must not publish a result for {expression}"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}
