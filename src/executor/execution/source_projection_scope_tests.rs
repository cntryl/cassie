//! COPY compiler classification and preserved scalar source-retention policy.
use super::*;
use std::sync::Arc;
use std::time::Instant;

fn run_projection(select: &str, input_name: &str, expected: &str, scalar: bool) {
    let path =
        std::env::temp_dir().join(format!("cassie-projection-scope-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    let statement = crate::sql::parse_statement(&format!(
        "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT {select} FROM c"
    ))
    .expect("private compiler-seam SQL");
    let plan =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("plan");
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let row = BatchRow::new(vec![(input_name.into(), Value::String("s".repeat(65536)))]);
    let origin = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("body"))
            .expect("origin"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&origin)))
        .retain_operator_memory(&controls, Arc::clone(&origin))
        .expect("prior root");
    let before = controls.current_query_memory_bytes();
    let expected_guard = reserve_projection_output_before_building(
        &controls,
        &[vec![row.clone()]],
        &plan.projection,
    )
    .expect("legacy bound");
    let legacy_bytes = expected_guard.bytes();
    drop(expected_guard);
    let mut output = apply_projection_phase(
        vec![vec![row]],
        &plan,
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("source projection");
    assert_eq!(output[0][0].get("x"), Some(&Value::String(expected.into())));
    assert!(output[0][0].operator_memory().is_some());
    if scalar {
        assert!(
            controls.current_query_memory_bytes() - before >= legacy_bytes,
            "source scalar boundary must preserve its old broad output owner"
        );
    } else {
        let actual = output[0][0].owned_body_bytes().expect("actual copied body");
        assert!(controls.current_query_memory_bytes() - before >= actual);
    }
    drop(origin);
    let row = output[0].pop().expect("copied row");
    assert_eq!(row.get("x"), Some(&Value::String(expected.into())));
    drop(row);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}

#[test]
fn should_retain_precomputed_aggregate_alias_copy_backing() {
    // Arrange
    let select = "min(n) AS x";
    // Act
    // Assert
    run_projection(select, "x", &"s".repeat(65536), false);
}

#[test]
fn should_retain_precomputed_window_alias_copy_backing() {
    // Arrange
    let select = "first_value(n) OVER () AS x";
    // Act
    // Assert
    run_projection(select, "x", &"s".repeat(65536), false);
}

#[test]
fn should_preserve_legacy_scalar_source_projection_contract() {
    // Arrange
    let literal = "t".repeat(524_288);
    let select = format!("concat(n, '{literal}') AS x");
    let expected = format!("{}{literal}", "s".repeat(65536));
    // Act
    // Assert
    run_projection(&select, "n", &expected, true);
}

#[test]
fn should_admit_mixed_direct_copy_capacity_growth() {
    // Arrange
    for count in [64, 256] {
        let path =
            std::env::temp_dir().join(format!("cassie-wildcard-growth-{}", uuid::Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
        let statement = crate::sql::parse_statement(
            "WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT n FROM c",
        )
        .expect("private seam SQL");
        let mut plan =
            super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("plan");
        plan.projection = std::iter::once(crate::sql::SelectItem::Wildcard)
            .chain((0..30).map(|index| crate::sql::SelectItem::Column {
                name: "n".into(),
                alias: Some(format!("x{index}")),
            }))
            .collect();
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 16 * 1024 * 1024,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let rows = (0..count)
            .map(|_| {
                BatchRow::new(vec![
                    ("n".into(), Value::Int64(1)),
                    ("m".into(), Value::Int64(2)),
                ])
            })
            .collect::<Vec<_>>();
        let body = rows
            .iter()
            .map(|row| row.unleased_body_bytes().expect("body"))
            .sum();
        let origin = Arc::new(controls.reserve_query_memory(body).expect("source"));
        let rows = rows
            .into_iter()
            .map(|row| {
                row.with_query_memory(Some(Arc::clone(&origin)))
                    .retain_operator_memory(&controls, Arc::clone(&origin))
                    .expect("prior root")
            })
            .collect();
        // Act
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apply_projection_phase(
                vec![rows],
                &plan,
                &[],
                None,
                &HashMap::new(),
                None,
                &controls,
            )
        }));
        // Assert
        let mut output = result
            .expect("selected COPY preadmission must cover wildcard Vec growth")
            .expect("projection");
        assert_eq!(output[0].len(), count);
        assert!(output[0].iter().all(|row| row.entries().len() == 32));
        let actual = output[0]
            .iter()
            .map(|row| row.owned_body_bytes().expect("body"))
            .sum::<usize>();
        assert!(controls.current_query_memory_bytes() >= actual);
        let row = output[0].pop().expect("row");
        let query = row.query_memory();
        let operator = row.operator_memory();
        let entries = row.into_entries();
        println!(
            "wildcard rows={count} width={} entry_capacity={} actual_body={actual}",
            entries.len(),
            entries.capacity()
        );
        assert_eq!(entries.capacity(), 62);
        drop(entries);
        drop(query);
        drop(operator);
        drop(output);
        drop(origin);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop(cassie);
        std::fs::remove_dir_all(path).expect("strict cleanup");
    }
}
