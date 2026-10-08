//! Fresh owned projection backing across the specialized finalizer handoff.
use super::*;
use std::sync::Arc;

mod controls;

#[test]
fn should_retain_wide_rich_projection_backing_after_specialized_handoff() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-projected-handoff-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    let statement = crate::sql::parse_statement(
        "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT n FROM c ORDER BY n",
    )
    .expect("SQL");
    let mut plan =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("bound plan");
    plan.projection = (0..17)
        .map(|index| SelectItem::Column {
            name: "n".into(),
            alias: Some(format!("x{index}")),
        })
        .collect();
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let row = BatchRow::new(vec![("n".into(), Value::String("s".repeat(65_536)))]);
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("body"))
            .expect("parent"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(&controls, Arc::clone(&parent))
        .expect("prior operator");
    let functions = HashMap::new();
    let mut batches = vec![vec![row]];

    // Act
    let mut output = finalize_projected_filtered_read(
        ProjectedReadFinalization {
            cassie: &cassie,
            session: None,
            plan: &plan,
            user_functions: &functions,
            params: &[],
            controls: &controls,
            apply_filter: false,
            apply_sort: true,
            index_usage: None,
        },
        &mut batches,
    )
    .expect("selected finalization");
    drop(parent);
    let row = output.pop().expect("output");
    let query = row.query_memory();
    let operator = row.operator_memory();
    let row = row.with_query_memory(None).with_operator_memory(None);
    let actual = row.unleased_body_bytes().expect("actual returned backing");
    let row = row.with_query_memory(query).with_operator_memory(operator);
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert_eq!(row.entries().len(), 17);
    assert!(row
        .entries()
        .iter()
        .all(|(_, value)| value == &Value::String("s".repeat(65_536))));
    assert!(
        retained >= actual,
        "retained {retained} < actual returned backing {actual}"
    );
    drop(row);
    drop(output);
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}

fn collection_handoff(use_breakdown: bool) {
    let path = std::env::temp_dir().join(format!("cassie-projected-text-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE projected_handoff (n TEXT)", vec![])
        .expect("table");
    cassie
        .midge
        .put_fresh_documents(
            "projected_handoff",
            vec![(
                Some("one".into()),
                serde_json::json!({"n": "s".repeat(65_536)}),
            )],
        )
        .expect("seed");
    let columns = (0..17)
        .map(|index| format!("n AS x{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {columns} FROM projected_handoff ORDER BY n");
    let public = cassie
        .execute_sql(&session, &sql, vec![])
        .expect("public ordinary TEXT query");
    assert_eq!(public.rows.len(), 1);
    assert_eq!(public.rows[0].len(), 17);
    assert!(public.columns.iter().all(|column| column.type_oid == 25));
    drop(public);
    let statement = crate::sql::parse_statement(&sql).expect("SQL");
    let plan = super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
        .expect("ordinary TEXT plan");
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let functions = HashMap::new();
    let mut output = if use_breakdown {
        execute_projected_filtered_read_with_breakdown(
            &cassie,
            Some(&session),
            &plan,
            &functions,
            &[],
            &controls,
        )
        .expect("breakdown read")
        .expect("selected path")
        .0
    } else {
        execute_projected_filtered_read(&cassie, Some(&session), &plan, &functions, &[], &controls)
            .expect("ordinary read")
            .expect("selected path")
    };
    let row = output.pop().expect("output");
    let query = row.query_memory();
    let operator = row.operator_memory();
    let row = row.with_query_memory(None).with_operator_memory(None);
    let actual = row.unleased_body_bytes().expect("actual returned body");
    let row = row.with_query_memory(query).with_operator_memory(operator);
    assert_eq!(row.entries().len(), 17);
    assert!(row
        .entries()
        .iter()
        .all(|(_, value)| value == &Value::String("s".repeat(65_536))));
    // Existing ScalarBacked TEXT projection omits the ARRAY-only descriptor carrier.
    assert!(row.data_types().is_empty());
    let retained = controls.current_query_memory_bytes();
    assert!(
        retained >= actual,
        "breakdown={use_breakdown} retained {retained} < actual returned backing {actual}"
    );
    drop(row);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}

#[test]
fn should_retain_ordinary_text_projection_backing_after_selected_read() {
    // Arrange
    let use_breakdown = false;
    // Act
    // Assert
    collection_handoff(use_breakdown);
}

#[test]
fn should_retain_ordinary_text_projection_backing_after_breakdown_read() {
    // Arrange
    let use_breakdown = true;
    // Act
    // Assert
    collection_handoff(use_breakdown);
}

#[test]
fn should_preserve_unpromoted_scalar_expression_projection_values() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-scalar-projection-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE scalar_projection (n TEXT)", vec![])
        .expect("table");
    cassie
        .midge
        .put_fresh_documents(
            "scalar_projection",
            vec![(
                Some("one".into()),
                serde_json::json!({"n": "s".repeat(65_536)}),
            )],
        )
        .expect("seed");
    let literal = "t".repeat(524_288);
    let sql = format!("SELECT concat(n, '{literal}') AS x FROM scalar_projection ORDER BY n");
    // Act
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cassie.execute_sql(&session, &sql, vec![])
    }));
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
    // Assert
    let result = result
        .expect("unpromoted scalar expression must preserve SQL errors instead of panic")
        .expect("existing scalar CONCAT query");
    assert_eq!(
        result.rows,
        vec![vec![Value::String(format!(
            "{}{literal}",
            "s".repeat(65_536)
        ))]]
    );
    assert_eq!(result.columns[0].type_oid, 25);
}
