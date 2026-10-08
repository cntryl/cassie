//! Actual-main alias accounting controls.
use super::*;
use std::time::Instant;

fn with_fixture(run: impl FnOnce(&Cassie)) {
    let path = std::env::temp_dir().join(format!("cassie-857-alias-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    run(&cassie);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

fn context() -> CteContext {
    CteContext::unleased(HashMap::from([(
        "c".into(),
        super::super::cte::CteRelation {
            rows: vec![vec![
                ("n".into(), Value::Null),
                ("tail".into(), Value::Int64(2)),
            ]],
            fields: vec![
                crate::types::FieldSchema {
                    name: "n".into(),
                    data_type: crate::types::DataType::Array(Box::new(
                        crate::types::DataType::Text,
                    )),
                    nullable: true,
                },
                crate::types::FieldSchema {
                    name: "tail".into(),
                    data_type: crate::types::DataType::BigInt,
                    nullable: false,
                },
            ],
            _rows_memory: None,
            _fields_memory: None,
        },
    )]))
}
fn source(alias: String) -> QuerySource {
    QuerySource::Aliased {
        source: Box::new(QuerySource::Cte("c".into())),
        alias,
        column_aliases: vec![],
    }
}

#[test]
fn should_retain_aliased_cte_source_qualification_until_output_drop() {
    // Arrange
    with_fixture(|cassie| {
        let mut context = context();
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let alias = "q".repeat(4096);
        let qualifier = crate::sql::binder::alias_row_qualifier(&alias);
        // Act
        let (output, _) =
            execute_query_source(&env, &source(alias), &mut context, true, None, None)
                .expect("aliased CTE source");
        // Assert
        assert_eq!(
            output[0][0].get(&format!("{qualifier}.n")),
            Some(&Value::Null)
        );
        assert!(
            controls.current_query_memory_bytes() >= 2 * qualifier.len(),
            "fresh qualifier names and eager lookup need retained charge"
        );
        assert!(output[0][0].operator_memory().is_some());
        drop(context);
        assert!(controls.current_query_memory_bytes() >= 2 * qualifier.len());
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_aliased_cte_carrier_before_uncontrolled_qualification() {
    // Arrange
    with_fixture(|cassie| {
        let mut context = context();
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 32 * 1024,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let result = execute_query_source(
            &env,
            &source("q".repeat(4096)),
            &mut context,
            true,
            None,
            None,
        );
        // Assert
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(context["c"].rows.len(), 1);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_retain_aliased_cte_array_null_template_until_drop() {
    // Arrange
    with_fixture(|cassie| {
        let context = context();
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let alias = "q".repeat(4096);
        let qualifier = crate::sql::binder::alias_row_qualifier(&alias);
        // Act
        let template =
            source_shape::null_row(&env, &source(alias), &context).expect("aliased NULL template");
        // Assert
        assert_eq!(template.entries()[0].0, "n");
        assert_eq!(template.get(&format!("{qualifier}.n")), Some(&Value::Null));
        assert_eq!(
            template.data_types()[0],
            crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
        );
        assert!(controls.current_query_memory_bytes() >= qualifier.len());
        drop(context);
        assert!(controls.current_query_memory_bytes() >= qualifier.len());
        drop(template);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_aliased_cte_null_template_before_uncontrolled_growth() {
    // Arrange
    with_fixture(|cassie| {
        let context = context();
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 32 * 1024,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let result = source_shape::null_row(&env, &source("q".repeat(4096)), &context);
        // Assert
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_retain_long_positional_prefix_output_backing_until_drop() {
    // Arrange
    with_fixture(|cassie| {
        let prefix = "p".repeat(4096);
        let sql = format!("WITH c AS (SELECT CAST(NULL AS TEXT[]) AS n, CAST(2 AS BIGINT) AS tail) SELECT * FROM c AS a(\"{prefix}\")");
        let statement = crate::sql::parse_statement(&sql).expect("prefix SQL");
        let plan = super::super::build_logical_plan_in_session(cassie, None, &statement)
            .expect("D1 bound prefix");
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
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
        .expect("prefix output");
        // Assert
        assert_eq!(output[0].entries()[0].0, prefix);
        assert_eq!(output[0].entries()[0].1, Value::Null);
        assert_eq!(output[0].get("tail"), Some(&Value::Int64(2)));
        assert_eq!(
            output[0].data_types()[0],
            crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
        );
        drop(context);
        assert!(
            controls.current_query_memory_bytes() >= 2 * prefix.len(),
            "entry names and eager lookup need owned output backing"
        );
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_admit_long_output_names_before_projection_construction() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 2048,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let parent = std::sync::Arc::new(controls.reserve_query_memory(1024).expect("parent"));
    let row = BatchRow::new(vec![("n".into(), Value::Int64(2))])
        .with_query_memory(Some(std::sync::Arc::clone(&parent)))
        .retain_operator_memory(&controls, std::sync::Arc::clone(&parent))
        .expect("operator");
    let batches = vec![vec![row]];
    let items = [crate::sql::SelectItem::Column {
        name: "n".into(),
        alias: Some("p".repeat(4096)),
    }];
    let before = controls.current_query_memory_bytes();
    // Act
    let result = reserve_projection_output_before_building(&controls, &batches, &items);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(batches);
    drop(parent);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_public_positional_prefix_metadata() {
    // Arrange
    with_fixture(|cassie| {
        let session = cassie.create_session("tester", None);
        let prefix = "p".repeat(4096);
        for alias in ["a", "\"A\""] {
            let sql = format!("WITH c AS (SELECT CAST(NULL AS TEXT[]) AS n, CAST(2 AS BIGINT) AS tail) SELECT * FROM c AS {alias}(\"{prefix}\")");
            // Act
            let result = cassie
                .execute_sql(&session, &sql, vec![])
                .expect("public prefix SQL");
            // Assert
            assert_eq!(result.rows, vec![vec![Value::Null, Value::Int64(2)]]);
            assert_eq!(result.columns[0].name, prefix);
            assert_eq!(result.columns[0].type_oid, 34025);
            assert_eq!(result.columns[1].name, "tail");
            assert_eq!(result.columns[1].type_oid, 20);
        }
        let error = cassie
            .execute_sql(
                &session,
                "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT * FROM c AS a(x,y)",
                vec![],
            )
            .expect_err("public binder alias arity rejected before execution");
        assert_eq!(error.descriptor().sql_state, "42P10");
    });
}

#[test]
fn should_retain_wide_primitive_projection_backing_until_output_drop() {
    // Arrange
    with_fixture(|cassie| {
        let columns = (0..100)
            .map(|i| format!("n AS x{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let statement = crate::sql::parse_statement(&format!(
            "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT {columns} FROM c"
        ))
        .expect("wide SQL");
        let plan = super::super::build_logical_plan_in_session(cassie, None, &statement)
            .expect("bound wide projection");
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let parent = std::sync::Arc::new(controls.reserve_query_memory(1024).expect("parent"));
        let row = BatchRow::new(vec![("n".into(), Value::Int64(1))])
            .with_query_memory(Some(std::sync::Arc::clone(&parent)))
            .retain_operator_memory(&controls, std::sync::Arc::clone(&parent))
            .expect("operator");
        let before = controls.current_query_memory_bytes();
        // Act
        let output = apply_projection_phase(
            vec![vec![row]],
            &plan,
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("wide output");
        // Assert
        assert_eq!(output[0][0].entries().len(), 100);
        assert!(output[0][0]
            .entries()
            .iter()
            .all(|(_, value)| value == &Value::Int64(1)));
        let actual_plain = output[0][0]
            .plain_entries_bytes()
            .expect("actual output entries capacity");
        assert!(
            controls.current_query_memory_bytes() - before >= actual_plain,
            "new owned inline slots and names must be charged; delta={} actualplain={actual_plain}",
            controls.current_query_memory_bytes() - before
        );
        drop(parent);
        assert!(controls.current_query_memory_bytes() >= 1024 + actual_plain);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}
