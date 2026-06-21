#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_explain_cost_model_diagnostics_for_index_choice() {
    // Arrange
    with_fallback();
    let path = data_dir("planner_cost_diagnostics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE planner_cost_docs (tenant TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_planner_cost_tenant ON planner_cost_docs USING btree (tenant)",
                vec![],
            )
            .unwrap();

        // Act
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM planner_cost_docs WHERE tenant = 'acme'",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = match &explained.rows[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains("cost_model=v1"));
        assert!(plan.contains("selected_cost="));
        assert!(plan.contains("cost_source="));
        assert!(plan.contains("rejected_alternatives="));

        let _ = std::fs::remove_dir_all(path);
    });
}
