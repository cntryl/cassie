//! Actual completed-path witnesses for selected and explicitly scalar operators.
use crate::support_relational_paths::capture;
use crate::support_relational_qualification::fixture;

#[test]
fn should_witness_each_selected_operator_completion() {
    // Arrange
    let fixture = fixture();
    let cases = [
        ("SELECT id FROM r ORDER BY n,id", "sort", "native_primitive_keys"),
        ("SELECT id FROM r ORDER BY n,id LIMIT 2", "top_k", "native_primitive_keys"),
        ("SELECT DISTINCT n FROM r", "distinct", "native_primitive_keys"),
        ("SELECT n FROM sa UNION ALL SELECT n FROM sb", "set", "native_primitive_keys"),
        ("SELECT DISTINCT ON(n) n,id FROM r ORDER BY n,id", "distinct_on", "native_primitive_keys"),
        ("SELECT id,ROW_NUMBER() OVER (ORDER BY n,id) AS rn FROM r", "window", "native_primitive_keys"),
        ("SELECT id FROM r ORDER BY n+1,id", "sort", "scalar_expression_order"),
        ("WITH c AS (SELECT n FROM r) SELECT DISTINCT n FROM c", "distinct", "scalar_cte_materialization"),
        ("SELECT id,FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW) AS v FROM r", "window", "scalar_window_frame"),
    ];

    // Act
    let observations = cases
        .iter()
        .map(|(sql, _, _)| capture(|| fixture.cassie.execute_sql(&fixture.session, sql, vec![])))
        .collect::<Vec<_>>();
    for ((sql, _, _), (result, events)) in cases.iter().zip(&observations) {
        println!("path {sql}: {result:?} {events:?}");
    }

    // Assert
    for ((sql, operator, path), (result, events)) in cases.iter().zip(observations) {
        result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let state = if path.starts_with("scalar_") {
            "documented_scalar_boundary"
        } else {
            "accounted_blocking"
        };
        assert!(
            events.iter().any(|event| event.operator == *operator
                && event.path == *path
                && event.state == state),
            "{sql}: {events:?}"
        );
    }
}

#[test]
fn should_retain_distinct_sort_sibling_events() {
    // Arrange
    let fixture = fixture();

    // Act
    let (result, events) = capture(|| {
        fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT DISTINCT n FROM r ORDER BY n",
            vec![],
        )
    });

    // Assert
    assert_eq!(result.expect("distinct sort").rows.len(), 5);
    for operator in ["distinct", "sort"] {
        assert!(
            events.iter().any(|event| event.operator == operator
                && event.path == "native_primitive_keys"
                && event.state == "accounted_blocking"),
            "{events:?}"
        );
    }
    println!("sibling operator events: {events:?}");
}

#[test]
fn should_isolate_parallel_caller_operator_events() {
    // Arrange
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let cases = [
        ("SELECT DISTINCT n FROM r", "distinct", "native_primitive_keys", 5),
        ("SELECT id,FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW) AS v FROM r", "window", "scalar_window_frame", 6),
    ];

    // Act
    let handles = cases
        .into_iter()
        .map(|(sql, operator, path, count)| {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let fixture = fixture();
                barrier.wait();
                let (result, events) =
                    capture(|| fixture.cassie.execute_sql(&fixture.session, sql, vec![]));
                (
                    result.expect("independent scoped query").rows.len(),
                    events,
                    operator,
                    path,
                    count,
                )
            })
        })
        .collect::<Vec<_>>();
    let observations = handles
        .into_iter()
        .map(|handle| handle.join().expect("scoped query thread"))
        .collect::<Vec<_>>();

    // Assert
    for (rows, events, operator, path, count) in observations {
        assert_eq!(rows, count);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(
            (events[0].operator.as_str(), events[0].path.as_str()),
            (operator, path)
        );
        assert_eq!(
            events[0].state,
            if path.starts_with("scalar_") {
                "documented_scalar_boundary"
            } else {
                "accounted_blocking"
            }
        );
        println!("parallel scoped owner {operator}: {events:?}");
    }
}
