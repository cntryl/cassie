#[test]
fn should_match_full_scan_for_non_finite_range_parameters_on_scalar_index() {
    // Arrange
    let path = data_dir("scalar_index_non_finite_parameters");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["non_finite_param_indexed", "non_finite_param_plain"],
            "x FLOAT",
            "x",
            &[Value::Float64(5.5)],
        );

        // Act
        let mut mismatches = Vec::new();
        for op in [">", ">=", "<", "<="] {
            for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let run = |table: &str| {
                    cassie
                        .execute_sql(
                            &session,
                            &format!("SELECT x FROM {table} WHERE x {op} $1"),
                            vec![Value::Float64(parameter)],
                        )
                        .expect("range query with non-finite parameter")
                        .rows()
                        .to_vec()
                };
                let full_scan = run("non_finite_param_plain");
                let indexed = run("non_finite_param_indexed");
                if indexed != full_scan {
                    mismatches.push(format!(
                        "x {op} {parameter}: full_scan={full_scan:?} indexed={indexed:?}"
                    ));
                }
            }
        }

        // Assert
        assert!(
            mismatches.is_empty(),
            "indexed results diverge from full scan: {mismatches:#?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_the_scalar_index_for_a_non_finite_parameter_outside_the_filter() {
    // Arrange
    let path = data_dir("scalar_index_unused_non_finite_parameter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["unused_non_finite_indexed", "unused_non_finite_plain"],
            "x FLOAT",
            "x",
            &[Value::Float64(5.5)],
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT x FROM unused_non_finite_indexed WHERE x > 4.5",
                vec![Value::Float64(f64::NAN)],
            )
            .expect("range query with an unbound non-finite parameter");
        let rows = result.rows();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(rows, vec![vec![Value::Float64(5.5)]]);
        assert!(
            metrics["read_paths"]["last_index_scan_index"]
                .as_str()
                .is_some_and(|index| index.ends_with("unused_non_finite_indexed_idx")),
            "a non-finite parameter outside the filter disabled the scalar index: {:?}",
            metrics["read_paths"]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_no_rows_for_null_range_parameter_on_scalar_index() {
    // Arrange
    let path = data_dir("scalar_index_null_range_parameter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        let session = cassie.create_session("tester", None);
        create_indexed_and_plain_tables(
            &cassie,
            &session,
            ["null_range_param_indexed", "null_range_param_plain"],
            "x FLOAT",
            "x",
            &[Value::Float64(5.5)],
        );

        // Act
        let mut results = Vec::new();
        for op in [">", ">=", "<", "<=", "="] {
            for table in ["null_range_param_plain", "null_range_param_indexed"] {
                let rows = cassie
                    .execute_sql(
                        &session,
                        &format!("SELECT x FROM {table} WHERE x {op} $1"),
                        vec![Value::Null],
                    )
                    .expect("range query with NULL parameter")
                    .rows()
                    .to_vec();
                results.push((format!("{table}: x {op} NULL"), rows));
            }
        }

        // Assert
        let non_empty = results
            .into_iter()
            .filter(|(_, rows)| !rows.is_empty())
            .collect::<Vec<_>>();
        assert!(
            non_empty.is_empty(),
            "comparisons with a NULL parameter must match no rows: {non_empty:#?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
