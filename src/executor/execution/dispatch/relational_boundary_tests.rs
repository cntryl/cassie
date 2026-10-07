use super::*;

#[test]
fn should_keep_union_rhs_cte_source_inside_declared_scalar_boundary() {
    // Arrange
    let statement = crate::sql::parse_statement(
        "WITH c AS (SELECT 1 AS x) SELECT DISTINCT x FROM (SELECT 2 AS x UNION SELECT x FROM c) q",
    )
    .expect("CTE set");
    let catalog = crate::catalog::Catalog::new();
    let bound = crate::sql::binder::bind(statement, &catalog).expect("actual CTE binding");
    let plan = crate::planner::logical::plan(&bound).expect("actual bound CTE plan");
    // Act
    let boundary = source_has_cte_boundary(&plan.source);
    // Assert
    assert!(
        boundary,
        "CTE source in derived UNION RHS must propagate to outer operators"
    );
}

#[test]
fn should_preserve_cte_body_scalar_boundary_and_public_null_descriptors() {
    // Arrange
    with_fixture(|cassie, limits| {
        let controls = QueryExecutionControls::from_limits(limits, Instant::now());
        let sql = "WITH c AS (SELECT DISTINCT CAST(NULL AS BIGINT) AS n) SELECT DISTINCT n FROM c";
        let statement = crate::sql::parse_statement(sql).expect("CTE SQL");
        let plan = build_logical_plan_in_session(cassie, None, &statement).expect("bound CTE plan");
        let mut context = CteContext::new();
        // Act
        let output = execute_plan(
            cassie,
            None,
            &plan,
            &mut context,
            &HashMap::new(),
            &[],
            &controls,
        )
        .expect("scalar CTE boundary");
        // Assert
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].entries()[0].1, Value::Null);
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "scalar_cte_materialization"))
        );
        assert!(output[0].operator_memory().is_none());
        // Record existing context ownership without promoting this scalar boundary as bounded.
        println!(
            "baseline_cte_context_live_rows={} current_accounted_bytes={}",
            context["c"].rows.len(),
            controls.current_query_memory_bytes()
        );
        drop(output);
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        let session = cassie.create_session("root", None);
        let cte = cassie
            .execute_sql(&session, sql, vec![])
            .expect("public CTE result");
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "scalar_cte_materialization"))
        );
        let direct = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT CAST(NULL AS BIGINT) AS n",
                vec![],
            )
            .expect("public native NULL result");
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "native_primitive_keys"))
        );
        assert_eq!(cte.rows, direct.rows);
        assert_eq!(cte.columns, direct.columns);
        assert_eq!(direct.columns[0].type_oid, 20);
    });
}

#[test]
fn should_preserve_cte_scalar_boundary_budget_failure_cleanup() {
    // Arrange
    with_fixture(|cassie, limits| {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..limits.clone()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let statement = crate::sql::parse_statement(
            "WITH c AS (SELECT DISTINCT 1 AS n) SELECT DISTINCT n FROM c",
        )
        .expect("CTE SQL");
        let plan = build_logical_plan_in_session(cassie, None, &statement).expect("bound CTE plan");
        let mut context = CteContext::new();
        let completed = crate::executor::typed_batch::relational_diagnostics::last_path();
        // Act
        let result = execute_plan(
            cassie,
            None,
            &plan,
            &mut context,
            &HashMap::new(),
            &[],
            &controls,
        );
        // Assert
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            completed
        );
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

fn with_fixture(run: impl FnOnce(&Cassie, &crate::config::CassieRuntimeLimits)) {
    let path = std::env::temp_dir().join(format!("cassie-760-cte-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 128 * 1024;
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("Cassie fixture");
    run(&cassie, &config.limits);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
