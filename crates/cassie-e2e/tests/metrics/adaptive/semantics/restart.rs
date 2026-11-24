#[test]
fn should_restore_fixed_plan_after_restart_without_deleting_persisted_feedback() {
    // Arrange
    use_local_storage();
    let path = data_dir("adaptive_profile_rollback");
    let mut evaluation = adaptive_execution_confidence_config(true, 500, 900);
    evaluation.limits.operator_switching_enabled = OperatorSwitchingEnabled::enabled();
    evaluation.limits.operator_switch_join_row_threshold = 4_096;
    let collection = "metrics_adaptive_profile_rollback";
    let base_index = "metrics_adaptive_profile_rollback_body_idx_a";
    let preferred_index = "metrics_adaptive_profile_rollback_title_idx_b";
    let shape_sql = "SELECT title FROM metrics_adaptive_profile_rollback WHERE title = 'alpha' AND body = 'one'";
    let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_profile_rollback WHERE title = 'alpha' AND body = 'one'";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, evaluation).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_persisted_adaptive_fixture(
            &cassie,
            &session,
            collection,
            base_index,
            preferred_index,
        );
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let evaluation_plan = cassie
            .execute_sql(&session, explain_sql, vec![])
            .unwrap()
            .rows()[0][0]
            .as_str()
            .unwrap()
            .to_string();
        let persisted_before = cassie
            .midge
            .list_runtime_feedback_records()
            .unwrap()
            .into_iter()
            .map(|(key, record)| (key, record.executions, record.stable_samples))
            .collect::<Vec<_>>();
        drop(session);
        drop(cassie);

        let mut disabled = cassie::config::CassieRuntimeConfig::from_env().unwrap();
        disabled.limits.operator_feedback_enabled = false;
        disabled.limits.adaptive_execution_enabled = false;
        disabled.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, disabled).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let selected = cassie.execute_sql(&session, shape_sql, vec![]).unwrap();
        let disabled_plan = cassie
            .execute_sql(&session, explain_sql, vec![])
            .unwrap()
            .rows()[0][0]
            .as_str()
            .unwrap()
            .to_string();
        let persisted_after = cassie
            .midge
            .list_runtime_feedback_records()
            .unwrap()
            .into_iter()
            .map(|(key, record)| (key, record.executions, record.stable_samples))
            .collect::<Vec<_>>();

        // Assert
        assert!(
            evaluation_plan.contains(&format!(
                "adaptive_selected_alternative=index:postgres.public.{preferred_index}"
            )),
            "plan={evaluation_plan}"
        );
        assert_eq!(selected.rows().len(), 8);
        assert!(selected
            .rows()
            .iter()
            .all(|row| { row == &vec![cassie::types::Value::String("alpha".into())] }));
        assert!(disabled_plan.contains("operator_feedback_reason=disabled"));
        assert!(disabled_plan.contains("adaptive_plan_enabled=false"));
        assert!(disabled_plan.contains(&format!(
            "adaptive_selected_alternative=index:postgres.public.{base_index}"
        )));
        assert_eq!(persisted_after.len(), persisted_before.len());
        for (key, executions, stable_samples) in &persisted_before {
            let (_, after_executions, after_stable_samples) = persisted_after
                .iter()
                .find(|(after_key, _, _)| after_key == key)
                .expect("rollback must preserve every persisted feedback key");
            assert!(*after_executions >= *executions);
            assert!(*after_stable_samples >= *stable_samples);
        }

        let _ = std::fs::remove_dir_all(path);
    });
}
