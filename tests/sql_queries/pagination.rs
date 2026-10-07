use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_bind_pagination_parameters_before_planning_each_execution() {
    // Arrange
    let fixture = sql_fixture(
        "pagination_parameters",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1), (2), (3), (4)",
        ],
    );
    let sql = "SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2";

    // Act
    let first = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(2), Value::Int64(1)],
    );
    let second = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(1), Value::Int64(3)],
    );

    // Assert
    assert_eq!(
        first.expect("first bounds").rows,
        vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
    assert_eq!(
        second.expect("second bounds").rows,
        vec![vec![Value::Int64(4)]]
    );
}

#[test]
fn should_apply_admitted_integer_pagination() {
    // Arrange
    let fixture = sql_fixture(
        "pagination_expressions",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1), (2), (3), (4)",
        ],
    );

    // Act
    let computed = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT n FROM records ORDER BY n LIMIT CAST($1 AS BIGINT) + 1 OFFSET 2 * 1",
        vec![Value::Int64(1)],
    );
    let unbounded = fixture.execute("SELECT n FROM records ORDER BY n LIMIT NULL OFFSET NULL");
    let all = fixture.execute("SELECT n FROM records ORDER BY n LIMIT ALL");

    // Assert
    assert_eq!(
        computed.expect("computed bounds").rows,
        vec![vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
    assert_eq!(unbounded.expect("NULL bounds").rows.len(), 4);
    assert_eq!(all.expect("ALL bound").rows.len(), 4);
}

#[test]
fn should_preserve_nested_pagination_parameters() {
    // Arrange
    let fixture = sql_fixture(
        "pagination_nested",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1), (2), (3), (4)",
        ],
    );
    let queries = [
        "WITH chosen AS (SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2) SELECT n FROM chosen ORDER BY n",
        "SELECT n FROM (SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2) AS chosen ORDER BY n",
        "SELECT n FROM records UNION ALL SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2",
    ];

    // Act
    let results: Vec<_> = queries
        .iter()
        .map(|sql| {
            fixture.cassie.execute_sql(
                &fixture.session,
                sql,
                vec![Value::Int64(1), Value::Int64(2)],
            )
        })
        .collect();

    // Assert
    assert_eq!(
        results[0].as_ref().expect("CTE").rows,
        vec![vec![Value::Int64(3)]]
    );
    assert_eq!(
        results[1].as_ref().expect("derived").rows,
        vec![vec![Value::Int64(3)]]
    );
    assert_eq!(
        results[2].as_ref().expect("set").rows,
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_enforce_checked_pagination_bounds() {
    // Arrange
    let fixture = sql_fixture(
        "pagination_checked",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1)",
        ],
    );

    // Act
    let huge = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2",
        vec![Value::Int64(i64::MAX), Value::Int64(i64::MAX)],
    );
    let overflow = fixture.execute("SELECT n FROM records LIMIT 9223372036854775807 + 1");
    let negative = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT n FROM records LIMIT $1",
        vec![Value::Int64(-1)],
    );

    // Assert
    assert_eq!(huge.expect("bounded source").rows, Vec::<Vec<Value>>::new());
    assert!(overflow
        .expect_err("overflow")
        .to_string()
        .contains("out of range"));
    assert!(negative
        .expect_err("negative limit")
        .to_string()
        .contains("LIMIT"));
}

#[test]
fn should_preserve_pagination_ast_contract() {
    // Arrange
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1 OFFSET CAST($2 AS INT)")
        .expect("expression AST");
    let catalog = cassie::catalog::Catalog::new();

    // Act
    let count = cassie::sql::parameter_count(&parsed);
    let oids = cassie::sql::parameter_type_oids_with_catalog(&parsed, &[], &catalog);
    let cassie::sql::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let serialized = serde_json::to_value(&select.limit).expect("serialize bounds");

    // Assert
    assert_eq!(count, 2);
    assert_eq!(oids, vec![20, 23]);
    assert_eq!(serialized, serde_json::json!({"Param": 0}));
    assert!(
        serde_json::from_value::<Option<cassie::sql::ast::Expr>>(serde_json::json!(10)).is_err()
    );
}

#[test]
fn should_resolve_direct_executor_bounds() {
    // Arrange
    let fixture = sql_fixture("pagination_direct_executor", &[]);
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("logical plan");
    let physical = cassie::planner::physical::build(logical);

    // Act
    let result = cassie::executor::run(&fixture.cassie, physical.clone(), vec![Value::Int64(0)]);
    let breakdown = cassie::executor::run_with_execution_breakdown(
        &fixture.cassie,
        physical,
        vec![Value::Int64(0)],
    );

    // Assert
    assert_eq!(result.expect("direct run").rows, Vec::<Vec<Value>>::new());
    assert_eq!(
        breakdown.expect("breakdown").result.rows,
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_admit_direct_bound_plan_state() {
    // Arrange
    let fixture = sql_fixture("pagination_memory", &[]);
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    let physical = std::sync::Arc::new(cassie::planner::physical::build(logical));
    let mut limits = cassie::config::CassieRuntimeConfig::from_env()
        .expect("limits")
        .limits;
    limits.query_memory_budget_bytes = 1;
    let controls =
        cassie::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());

    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &physical,
        vec![Value::Int64(0)],
        &controls,
    );

    // Assert
    assert!(result
        .expect_err("pre-clone admission")
        .to_string()
        .contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_resolve_pagination_before_mutation() {
    // Arrange
    for command in [
        "DELETE FROM records WHERE EXISTS (SELECT n FROM records LIMIT $1)",
        "UPDATE records SET n = n + 10 WHERE EXISTS (SELECT n FROM records LIMIT $1)",
    ] {
        let fixture = sql_fixture(
            "pagination_dml_exists",
            &[
                "CREATE TABLE records (n BIGINT)",
                "INSERT INTO records VALUES (1), (2)",
            ],
        );

        // Act
        let zero = fixture
            .cassie
            .execute_sql(&fixture.session, command, vec![Value::Int64(0)]);
        let after_zero = fixture.rows("SELECT n FROM records ORDER BY n");
        let negative =
            fixture
                .cassie
                .execute_sql(&fixture.session, command, vec![Value::Int64(-1)]);
        let after_negative = fixture.rows("SELECT n FROM records ORDER BY n");

        // Assert
        zero.expect("zero bound is admitted");
        assert_eq!(
            after_zero,
            vec![vec![Value::Int64(1)], vec![Value::Int64(2)]],
            "{command}"
        );
        assert!(negative
            .expect_err("negative bound rejects before mutation")
            .to_string()
            .contains("LIMIT"));
        assert_eq!(after_negative, after_zero, "{command}");
        fixture
            .cassie
            .execute_sql(&fixture.session, command, vec![Value::Int64(1)])
            .expect("positive bound admits existing mutation");
        let expected = if command.starts_with("DELETE") {
            Vec::new()
        } else {
            vec![vec![Value::Int64(11)], vec![Value::Int64(12)]]
        };
        assert_eq!(
            fixture.rows("SELECT n FROM records ORDER BY n"),
            expected,
            "{command}"
        );
    }
}

#[test]
fn should_bound_pagination_arithmetic_complexity_before_recursive_parsing() {
    // Arrange
    let bound = std::iter::repeat_n("1", 130)
        .collect::<Vec<_>>()
        .join(" + ");
    let sql = format!("SELECT 1 LIMIT {bound}");

    // Act
    let error = cassie::sql::parse_statement(&sql).expect_err("finite bound expression budget");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::ResourceLimit);
}

#[test]
fn should_preserve_stored_view_pagination_domains() {
    // Arrange
    let fixture = sql_fixture(
        "pagination_views",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1), (2), (3)",
            "CREATE VIEW chosen AS SELECT n FROM records ORDER BY n LIMIT 1 + 1 OFFSET 1",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT n FROM chosen ORDER BY n");
    let small = fixture
        .execute("SELECT n FROM records LIMIT CAST(32767 AS SMALLINT) + CAST(1 AS SMALLINT)");
    let integer =
        fixture.execute("SELECT n FROM records LIMIT CAST(2147483647 AS INT) + CAST(1 AS INT)");
    let outer_cast = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT n FROM records LIMIT CAST($1 + $2 AS BIGINT)",
        vec![Value::Int64(1), Value::Int64(1)],
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]);
    for result in [small, integer] {
        assert!(result
            .expect_err("integer domain overflow")
            .to_string()
            .contains("out of range"));
    }
    assert!(outer_cast
        .expect_err("outer cast cannot type inner arithmetic")
        .to_string()
        .contains("ambiguous"));
}

#[test]
fn should_reject_direct_constructed_bound_depth_before_temporary_plan_clone() {
    // Arrange
    let fixture = sql_fixture("pagination_direct_depth", &[]);
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let mut logical = cassie::planner::logical::plan(&bound).expect("plan");
    let mut expr = cassie::sql::ast::Expr::Param(0);
    for _ in 0..130 {
        expr = cassie::sql::ast::Expr::Cast {
            expr: Box::new(expr),
            data_type: cassie::types::DataType::BigInt,
        };
    }
    logical.limit = Some(expr);
    let physical = std::sync::Arc::new(cassie::planner::physical::build(logical));
    let limits = cassie::config::CassieRuntimeConfig::from_env()
        .expect("limits")
        .limits;
    let controls =
        cassie::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());

    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &physical,
        vec![Value::Int64(0)],
        &controls,
    );

    // Assert
    assert!(result
        .expect_err("finite direct bound depth")
        .to_string()
        .contains("pagination plan nesting"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_deep_nested_direct_bound_before_temporary_plan_clone() {
    // Arrange
    let fixture = sql_fixture("pagination_nested_direct_depth", &[]);
    let parsed = cassie::sql::parse_statement("SELECT n FROM (SELECT 1 AS n LIMIT $1) AS chosen")
        .expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let mut logical = cassie::planner::logical::plan(&bound).expect("plan");
    let cassie::sql::ast::QuerySource::Subquery { select, .. } = &mut logical.source else {
        panic!("derived source");
    };
    let mut expr = cassie::sql::ast::Expr::Param(0);
    for _ in 0..300 {
        expr = cassie::sql::ast::Expr::Cast {
            expr: Box::new(expr),
            data_type: cassie::types::DataType::BigInt,
        };
    }
    select.limit = Some(expr);
    let physical = std::sync::Arc::new(cassie::planner::physical::build(logical));
    let limits = cassie::config::CassieRuntimeConfig::from_env()
        .expect("limits")
        .limits;
    let controls =
        cassie::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());

    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &physical,
        vec![Value::Int64(0)],
        &controls,
    );

    // Assert
    assert!(result
        .expect_err("finite serialized temporary plan depth")
        .to_string()
        .contains("pagination plan nesting"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_direct_bounds_at_the_existing_nesting_envelope() {
    // Arrange
    let fixture = sql_fixture("pagination_direct_admitted_depth", &[]);
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let mut logical = cassie::planner::logical::plan(&bound).expect("plan");
    let mut expr = cassie::sql::ast::Expr::Param(0);
    for _ in 0..128 {
        expr = cassie::sql::ast::Expr::Cast {
            expr: Box::new(expr),
            data_type: cassie::types::DataType::BigInt,
        };
    }
    logical.limit = Some(expr);
    let physical = cassie::planner::physical::build(logical);

    // Act
    let result = cassie::executor::run(&fixture.cassie, physical, vec![Value::Int64(0)]);

    // Assert
    assert_eq!(
        result.expect("existing finite depth remains admitted").rows,
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_guard_deep_direct_expression_before_recursive_bound_detection() {
    // Arrange
    let fixture = sql_fixture("pagination_deep_expression", &[]);
    let parsed = cassie::sql::parse_statement("SELECT 1 LIMIT $1").expect("parse");
    let bound = cassie::sql::binder::bind(parsed.clone(), &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    let mut physical = cassie::planner::physical::build(logical);
    let mut expr = cassie::sql::ast::Expr::Exists(Box::new(parsed));
    for _ in 0..256 {
        expr = cassie::sql::ast::Expr::Case {
            operand: Some(Box::new(expr)),
            branches: Vec::new(),
            else_expr: None,
        };
    }
    physical.logical.limit = None;
    let cassie::sql::ast::SelectItem::Expr {
        expr: projection, ..
    } = &mut physical.logical.projection[0]
    else {
        panic!("expression projection");
    };
    *projection = expr;
    let physical = std::sync::Arc::new(physical);
    let limits = cassie::config::CassieRuntimeConfig::from_env()
        .expect("limits")
        .limits;
    let controls =
        cassie::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());

    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &physical,
        vec![Value::Int64(0)],
        &controls,
    );

    // Assert
    assert!(result
        .expect_err("direct expression depth admission")
        .to_string()
        .contains("pagination plan nesting"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
