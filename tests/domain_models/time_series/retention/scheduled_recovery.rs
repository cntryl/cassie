// Focused scheduled-retention lifecycle and recovery probes included by scheduled.rs.

#[test]
fn should_enforce_due_window_with_foreign_key_index_and_rollup_parity() {
    // Arrange
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_scheduled_effect_source(
            &cassie,
            &session,
            "scheduled_effect_events",
            "scheduled_effect_policy",
            true,
        );
        create_scheduled_effect_source(
            &cassie,
            &session,
            "manual_effect_events",
            "manual_effect_policy",
            false,
        );

        // Act
        let effects = observe_scheduled_effects(&cassie, &session);

        // Assert
        assert_scheduled_effects_match_manual(&effects);
    });
}

struct ScheduledEffectObservation {
    scheduled_command: String,
    manual_command: String,
    scheduled_source: Vec<Vec<Value>>,
    manual_source: Vec<Vec<Value>>,
    scheduled_restrict: Vec<Vec<Value>>,
    manual_restrict: Vec<Vec<Value>>,
    scheduled_cascade: Vec<Vec<Value>>,
    manual_cascade: Vec<Vec<Value>>,
    scheduled_index_range: Vec<Vec<Value>>,
    manual_index_range: Vec<Vec<Value>>,
    scheduled_rollup: Vec<Vec<Value>>,
    manual_rollup: Vec<Vec<Value>>,
    scheduled_rollup_oracle: Vec<Vec<Value>>,
    manual_rollup_oracle: Vec<Vec<Value>>,
    progress: Vec<Vec<Value>>,
}

fn observe_scheduled_effects(
    cassie: &Cassie,
    session: &CassieSession,
) -> ScheduledEffectObservation {
    let scheduled_command = cassie
        .execute_sql(
            session,
            "ENFORCE RETENTION POLICY scheduled_effect_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let manual_command = cassie
        .execute_sql(
            session,
            "ENFORCE RETENTION POLICY manual_effect_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let scheduled_source = retention_source_rows(cassie, session, "scheduled_effect_events");
    let manual_source = retention_source_rows(cassie, session, "manual_effect_events");
    let scheduled_restrict = sql_rows(
        cassie,
        session,
        "SELECT parent_id FROM scheduled_effect_events_restrict ORDER BY parent_id",
    );
    let manual_restrict = sql_rows(
        cassie,
        session,
        "SELECT parent_id FROM manual_effect_events_restrict ORDER BY parent_id",
    );
    let scheduled_cascade = sql_rows(
        cassie,
        session,
        "SELECT parent_id FROM scheduled_effect_events_cascade ORDER BY parent_id",
    );
    let manual_cascade = sql_rows(
        cassie,
        session,
        "SELECT parent_id FROM manual_effect_events_cascade ORDER BY parent_id",
    );
    let scheduled_index_range = sql_rows(cassie, session, scheduled_index_query("scheduled"));
    let manual_index_range = sql_rows(cassie, session, scheduled_index_query("manual"));
    let scheduled_rollup = retention_rollup_rows(cassie, session, "scheduled_effect_events");
    let manual_rollup = retention_rollup_rows(cassie, session, "manual_effect_events");
    let scheduled_rollup_oracle =
        retention_rollup_source_rows(cassie, session, "scheduled_effect_events");
    let manual_rollup_oracle =
        retention_rollup_source_rows(cassie, session, "manual_effect_events");
    let progress = scheduled_policy_progress(cassie, session, "scheduled_effect_policy");
    ScheduledEffectObservation {
        scheduled_command,
        manual_command,
        scheduled_source,
        manual_source,
        scheduled_restrict,
        manual_restrict,
        scheduled_cascade,
        manual_cascade,
        scheduled_index_range,
        manual_index_range,
        scheduled_rollup,
        manual_rollup,
        scheduled_rollup_oracle,
        manual_rollup_oracle,
        progress,
    }
}

fn scheduled_index_query(kind: &str) -> String {
    format!(
        "SELECT id FROM {kind}_effect_events WHERE device = 'a' AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-02T00:00:00Z' ORDER BY id"
    )
}

fn sql_rows(cassie: &Cassie, session: &CassieSession, sql: impl AsRef<str>) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql.as_ref(), vec![])
        .unwrap()
        .rows()
        .to_vec()
}

fn assert_scheduled_effects_match_manual(effects: &ScheduledEffectObservation) {
    assert_eq!(effects.scheduled_command, "ENFORCE RETENTION 1");
    assert_eq!(effects.manual_command, "ENFORCE RETENTION 1");
    assert_eq!(effects.scheduled_source, effects.manual_source);
    assert_eq!(effects.scheduled_source.len(), 3);
    assert_eq!(effects.scheduled_source[0][0], Value::Int64(2));
    assert_eq!(effects.scheduled_source[1][0], Value::Int64(3));
    assert_eq!(effects.scheduled_restrict, effects.manual_restrict);
    assert_eq!(effects.scheduled_restrict, vec![vec![Value::Int64(2)]]);
    assert_eq!(effects.scheduled_cascade, effects.manual_cascade);
    assert!(effects.scheduled_cascade.is_empty());
    assert_eq!(effects.scheduled_index_range, effects.manual_index_range);
    assert!(effects.scheduled_index_range.is_empty());
    assert_eq!(effects.scheduled_rollup, effects.manual_rollup);
    assert_eq!(effects.scheduled_rollup, effects.scheduled_rollup_oracle);
    assert_eq!(effects.manual_rollup, effects.manual_rollup_oracle);
    assert_scheduled_effect_progress(&effects.progress);
}

fn assert_scheduled_effect_progress(progress: &[Vec<Value>]) {
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0][0], Value::String("active".to_owned()));
    assert_explicit_ready_policy(&progress[0]);
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-04T00:00:00Z");
    assert_eq!(progress[0][2], Value::Null);
    assert_eq!(progress[0][3], Value::Int64(0));
    assert_eq!(progress[0][4], Value::Int64(0));
    assert_eq!(progress[0][5], Value::Int64(0));
    assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T00:00:00Z");
    assert_eq!(progress[0][7], Value::String(String::new()));
    assert_eq!(progress[0][8], Value::Null);
    assert_eq!(progress[0][14], Value::Int64(1));
    assert_eq!(progress[0][15], Value::Int64(1));
}

#[test]
fn should_delete_only_rows_strictly_before_due_window_cutoff() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE scheduled_cutoff_events (id INT PRIMARY KEY, event_at TIMESTAMP NOT NULL)",
            vec![],
        )
        .unwrap();
    for sql in [
        "INSERT INTO scheduled_cutoff_events VALUES (1, '2026-01-02T11:59:59.999999Z')",
        "INSERT INTO scheduled_cutoff_events VALUES (2, '2026-01-02T12:00:00Z')",
        "INSERT INTO scheduled_cutoff_events VALUES (3, '2026-01-02T12:00:00.000001Z')",
        "CREATE RETENTION POLICY scheduled_cutoff_policy ON scheduled_cutoff_events USING event_at RETAIN FOR '1 day'",
        "ALTER RETENTION POLICY scheduled_cutoff_policy SCHEDULE DAILY START AT '2026-01-03T00:00:00Z'",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }

    // Act
    let enforced = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_cutoff_policy DUE AT '2026-01-03T12:00:00Z'",
            vec![],
        )
        .unwrap();
    let remaining = cassie
        .execute_sql(
            &session,
            "SELECT id FROM scheduled_cutoff_events ORDER BY id",
            vec![],
        )
        .unwrap();
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_cutoff_policy");

    // Assert
    assert_eq!(enforced.command(), "ENFORCE RETENTION 1");
    assert_eq!(
        remaining.rows(),
        vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
    assert_eq!(progress[0][14], Value::Int64(1));
    assert_eq!(progress[0][15], Value::Int64(0));
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-04T00:00:00Z");
    assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T12:00:00Z");
}

#[test]
fn should_resume_one_bounded_due_batch_before_later_cutoffs_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("retention_scheduled_resume");

    // Act
    let first = record_first_resume_batch(&path);
    let replay = resume_after_restart(&path);

    // Assert
    assert_first_resume_batch(&first);
    assert_resumed_windows(&replay);
}

struct FirstResumeBatch {
    command: String,
    row_count: Vec<Vec<Value>>,
    progress: Vec<Vec<Value>>,
    suspended_command: String,
    suspended_progress: Vec<Vec<Value>>,
}

fn record_first_resume_batch(path: &std::path::Path) -> FirstResumeBatch {
    let cassie = Cassie::new_with_data_dir(path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_resume_events",
        "scheduled_resume_policy",
        513,
        2,
    );
    let command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let row_count = sql_rows(
        &cassie,
        &session,
        "SELECT COUNT(*) FROM scheduled_resume_events",
    );
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
    cassie
        .execute_sql(
            &session,
            "ALTER RETENTION POLICY scheduled_resume_policy SUSPEND",
            vec![],
        )
        .unwrap();
    let suspended_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let suspended_progress =
        scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
    drop(cassie);
    FirstResumeBatch {
        command,
        row_count,
        progress,
        suspended_command,
        suspended_progress,
    }
}

fn assert_first_resume_batch(first: &FirstResumeBatch) {
    assert_eq!(first.command, "ENFORCE RETENTION 256");
    assert_eq!(first.row_count, vec![vec![Value::Int64(259)]]);
    assert_explicit_ready_policy(&first.progress[0]);
    assert_timestamptz_value(&first.progress[0], 2, 10, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(&first.progress[0], 1, 9, "2026-01-03T00:00:00Z");
    assert_eq!(first.progress[0][3], Value::Int64(256));
    assert_eq!(first.progress[0][4], Value::Int64(256));
    assert_eq!(first.progress[0][5], Value::Int64(0));
    assert_eq!(first.progress[0][6], Value::Null);
    assert_eq!(first.progress[0][11], Value::Null);
    assert_eq!(first.progress[0][7], Value::String(String::new()));
    assert_eq!(first.progress[0][8], Value::Null);
    assert_eq!(first.suspended_command, "ENFORCE RETENTION 0");
    assert_explicit_ready_policy(&first.suspended_progress[0]);
    assert_eq!(first.suspended_progress[0][1], first.progress[0][1]);
    assert_eq!(first.suspended_progress[0][2], first.progress[0][2]);
    assert_eq!(first.suspended_progress[0][3], first.progress[0][3]);
    assert_eq!(first.suspended_progress[0][4], first.progress[0][4]);
    assert_eq!(first.suspended_progress[0][6], Value::Null);
    assert_eq!(first.suspended_progress[0][11], Value::Null);
}

struct ResumeReplay {
    same_time_command: String,
    same_time_rows: Vec<Vec<Value>>,
    same_time_progress: Vec<Vec<Value>>,
    different_time_command: String,
    old_window_rows: Vec<Vec<Value>>,
    old_window_progress: Vec<Vec<Value>>,
    second_window_command: String,
    second_window_rows: Vec<Vec<Value>>,
    second_window_progress: Vec<Vec<Value>>,
}

fn resume_after_restart(path: &std::path::Path) -> ResumeReplay {
    let cassie = Cassie::new_with_data_dir(path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    let same_time_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_resume_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let same_time_rows = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_resume_events ORDER BY id",
    );
    let same_time_progress =
        scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
    let different_time_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_resume_policy AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let old_window_rows = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_resume_events ORDER BY id",
    );
    let old_window_progress =
        scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
    cassie
        .execute_sql(
            &session,
            "ALTER RETENTION POLICY scheduled_resume_policy RESUME",
            vec![],
        )
        .unwrap();
    let second_window_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let second_window_rows = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_resume_events ORDER BY id",
    );
    let second_window_progress =
        scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
    drop(session);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
    ResumeReplay {
        same_time_command,
        same_time_rows,
        same_time_progress,
        different_time_command,
        old_window_rows,
        old_window_progress,
        second_window_command,
        second_window_rows,
        second_window_progress,
    }
}

fn assert_resumed_windows(replay: &ResumeReplay) {
    assert_eq!(replay.same_time_command, "ENFORCE RETENTION 256");
    assert_eq!(
        replay.same_time_rows,
        vec![
            vec![Value::Int64(513)],
            vec![Value::Int64(514)],
            vec![Value::Int64(515)]
        ]
    );
    assert_timestamptz_value(&replay.same_time_progress[0], 2, 10, "2026-01-03T00:00:00Z");
    assert_explicit_ready_policy(&replay.same_time_progress[0]);
    assert_eq!(replay.same_time_progress[0][3], Value::Int64(512));
    assert_eq!(replay.same_time_progress[0][4], Value::Int64(512));
    assert_eq!(replay.same_time_progress[0][6], Value::Null);
    assert_eq!(replay.same_time_progress[0][11], Value::Null);
    assert_eq!(replay.different_time_command, "ENFORCE RETENTION 1");
    assert_eq!(
        replay.old_window_rows,
        vec![vec![Value::Int64(514)], vec![Value::Int64(515)]]
    );
    assert_eq!(replay.old_window_progress[0][2], Value::Null);
    assert_explicit_ready_policy(&replay.old_window_progress[0]);
    assert_timestamptz_value(&replay.old_window_progress[0], 1, 9, "2026-01-04T00:00:00Z");
    assert_timestamptz_value(
        &replay.old_window_progress[0],
        6,
        11,
        "2026-01-03T00:00:00Z",
    );
    assert_eq!(replay.second_window_command, "ENFORCE RETENTION 1");
    assert_explicit_ready_policy(&replay.second_window_progress[0]);
    assert_eq!(replay.second_window_rows, vec![vec![Value::Int64(515)]]);
    assert_timestamptz_value(
        &replay.second_window_progress[0],
        6,
        11,
        "2026-01-04T00:00:00Z",
    );
    assert_timestamptz_value(
        &replay.second_window_progress[0],
        1,
        9,
        "2026-01-05T00:00:00Z",
    );
}

#[test]
fn should_advance_scheduled_cursor_when_a_bounded_batch_deletes_no_rows() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_cursor_events",
        "scheduled_cursor_policy",
        0,
        257,
    );

    // Act
    let evidence = advance_cursor_with_late_correction(&cassie, &session);

    // Assert
    assert_late_correction_waits_for_next_window(&evidence);
}

struct CursorCorrectionEvidence {
    first_command: String,
    first_progress: Vec<Vec<Value>>,
    finish_command: String,
    completed_first_progress: Vec<Vec<Value>>,
    count_before_next_window: Vec<Vec<Value>>,
    next_command: String,
    completed_second_progress: Vec<Vec<Value>>,
    final_count: Vec<Vec<Value>>,
    corrected_row: Vec<Vec<Value>>,
}

fn advance_cursor_with_late_correction(
    cassie: &Cassie,
    session: &CassieSession,
) -> CursorCorrectionEvidence {
    let first_command = cassie
        .execute_sql(
            session,
            "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let first_progress = scheduled_policy_progress(cassie, session, "scheduled_cursor_policy");
    cassie
        .execute_sql(
            session,
            "UPDATE scheduled_cursor_events SET event_at = '2026-01-01T00:00:00Z' WHERE id = 1",
            vec![],
        )
        .unwrap();
    let finish_command = cassie
        .execute_sql(
            session,
            "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let completed_first_progress =
        scheduled_policy_progress(cassie, session, "scheduled_cursor_policy");
    let count_before_next_window = sql_rows(
        cassie,
        session,
        "SELECT COUNT(*) FROM scheduled_cursor_events",
    );
    let next_command = cassie
        .execute_sql(
            session,
            "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let completed_second_progress =
        scheduled_policy_progress(cassie, session, "scheduled_cursor_policy");
    let final_count = sql_rows(
        cassie,
        session,
        "SELECT COUNT(*) FROM scheduled_cursor_events",
    );
    let corrected_row = sql_rows(
        cassie,
        session,
        "SELECT id FROM scheduled_cursor_events WHERE id = 1",
    );
    CursorCorrectionEvidence {
        first_command,
        first_progress,
        finish_command,
        completed_first_progress,
        count_before_next_window,
        next_command,
        completed_second_progress,
        final_count,
        corrected_row,
    }
}

fn assert_late_correction_waits_for_next_window(evidence: &CursorCorrectionEvidence) {
    assert_eq!(evidence.first_command, "ENFORCE RETENTION 0");
    assert_explicit_ready_policy(&evidence.first_progress[0]);
    assert_timestamptz_value(&evidence.first_progress[0], 2, 10, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(&evidence.first_progress[0], 1, 9, "2026-01-03T00:00:00Z");
    assert_eq!(evidence.first_progress[0][3], Value::Int64(256));
    assert_eq!(evidence.first_progress[0][4], Value::Int64(0));
    assert_eq!(evidence.first_progress[0][5], Value::Int64(0));
    assert_eq!(evidence.first_progress[0][6], Value::Null);
    assert_eq!(evidence.first_progress[0][11], Value::Null);
    assert_eq!(evidence.finish_command, "ENFORCE RETENTION 0");
    assert_explicit_ready_policy(&evidence.completed_first_progress[0]);
    assert_timestamptz_value(
        &evidence.completed_first_progress[0],
        1,
        9,
        "2026-01-04T00:00:00Z",
    );
    assert_eq!(evidence.completed_first_progress[0][2], Value::Null);
    assert_timestamptz_value(
        &evidence.completed_first_progress[0],
        6,
        11,
        "2026-01-03T00:00:00Z",
    );
    assert_eq!(
        evidence.count_before_next_window,
        vec![vec![Value::Int64(257)]]
    );
    assert_eq!(evidence.next_command, "ENFORCE RETENTION 1");
    assert_explicit_ready_policy(&evidence.completed_second_progress[0]);
    assert_timestamptz_value(
        &evidence.completed_second_progress[0],
        1,
        9,
        "2026-01-05T00:00:00Z",
    );
    assert_timestamptz_value(
        &evidence.completed_second_progress[0],
        6,
        11,
        "2026-01-04T00:00:00Z",
    );
    assert_eq!(evidence.final_count, vec![vec![Value::Int64(256)]]);
    assert!(evidence.corrected_row.is_empty());
}

#[test]
fn should_complete_an_exactly_full_batch_on_a_metadata_only_call() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_exact_batch_events",
        "scheduled_exact_batch_policy",
        256,
        0,
    );

    // Act
    let first_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_exact_batch_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let pending = scheduled_policy_progress(&cassie, &session, "scheduled_exact_batch_policy");
    let second_command = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_exact_batch_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap()
        .command()
        .to_owned();
    let completed = scheduled_policy_progress(&cassie, &session, "scheduled_exact_batch_policy");
    let remaining = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_exact_batch_events ORDER BY id",
    );

    // Assert
    assert_eq!(first_command, "ENFORCE RETENTION 256");
    assert_exact_batch_remains_pending(&pending[0]);
    assert_eq!(second_command, "ENFORCE RETENTION 0");
    assert_exact_batch_completes_metadata_only(&completed[0]);
    assert!(remaining.is_empty());
}

fn assert_exact_batch_remains_pending(progress: &[Value]) {
    assert_explicit_ready_policy(progress);
    assert_timestamptz_value(progress, 2, 10, "2026-01-03T00:00:00Z");
    assert_eq!(progress[3], Value::Int64(256));
    assert_eq!(progress[4], Value::Int64(256));
    assert_eq!(progress[6], Value::Null);
}

fn assert_exact_batch_completes_metadata_only(progress: &[Value]) {
    assert_eq!(progress[2], Value::Null);
    assert_eq!(progress[3], Value::Int64(0));
    assert_eq!(progress[4], Value::Int64(0));
    assert_timestamptz_value(progress, 6, 11, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(progress, 1, 9, "2026-01-04T00:00:00Z");
    assert_eq!(progress[14], Value::Int64(256));
    assert_eq!(progress[15], Value::Int64(0));
}

#[test]
fn should_keep_older_pending_cutoff_when_newer_due_tick_races_replay() {
    // Arrange
    let cassie = std::sync::Arc::new(crate::support_sql::new_in_memory_cassie().unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_overlap_window_events",
        "scheduled_overlap_window_policy",
        513,
        2,
    );
    let first = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_overlap_window_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    assert_eq!(first.command(), "ENFORCE RETENTION 256");

    // Act
    let commands = run_pending_replay_race(&cassie);
    let remaining = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_overlap_window_events ORDER BY id",
    );
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_overlap_window_policy");

    // Assert
    assert_eq!(
        commands,
        vec!["ENFORCE RETENTION 1", "ENFORCE RETENTION 256"]
    );
    assert_eq!(
        remaining,
        vec![vec![Value::Int64(514)], vec![Value::Int64(515)]]
    );
    assert_eq!(progress[0][2], Value::Null);
    assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-04T00:00:00Z");
    assert_eq!(progress[0][14], Value::Int64(513));
}

fn run_pending_replay_race(cassie: &std::sync::Arc<Cassie>) -> Vec<String> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let replay = spawn_due_worker(
        std::sync::Arc::clone(cassie),
        std::sync::Arc::clone(&barrier),
        "ENFORCE RETENTION POLICY scheduled_overlap_window_policy AT '2026-01-03T00:00:00Z'",
    );
    let newer_tick = spawn_due_worker(
        std::sync::Arc::clone(cassie),
        std::sync::Arc::clone(&barrier),
        "ENFORCE RETENTION POLICY scheduled_overlap_window_policy DUE AT '2026-01-04T00:00:00Z'",
    );
    barrier.wait();
    let mut commands = vec![
        replay.join().expect("same-cutoff replay completed"),
        newer_tick.join().expect("newer due tick completed"),
    ];
    commands.sort();
    commands
}

fn spawn_due_worker(
    cassie: std::sync::Arc<Cassie>,
    barrier: std::sync::Arc<std::sync::Barrier>,
    sql: &'static str,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let session = cassie.create_session("scheduled-worker", None);
        barrier.wait();
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("pending window batch")
            .command()
            .to_owned()
    })
}

#[test]
fn should_apply_explicit_at_to_scheduled_policy_without_pending_window() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_one_off_events",
        "scheduled_one_off_policy",
        1,
        0,
    );

    // Act
    let enforced = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_one_off_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    let remaining = sql_rows(
        &cassie,
        &session,
        "SELECT id FROM scheduled_one_off_events ORDER BY id",
    );
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_one_off_policy");

    // Assert
    assert_eq!(enforced.command(), "ENFORCE RETENTION 1");
    assert!(remaining.is_empty());
    assert_eq!(progress[0][0], Value::String("active".to_owned()));
    assert_explicit_ready_policy(&progress[0]);
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-03T00:00:00Z");
    assert_eq!(progress[0][2], Value::Null);
    assert_eq!(progress[0][6], Value::Null);
}

#[test]
fn should_preserve_scheduled_failure_after_failed_one_off_at() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_one_off_failure_events",
        "scheduled_one_off_failure_policy",
        1,
        0,
    );
    replace_retention_duration(&cassie, "scheduled_one_off_failure_policy", "1 fortnight");
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&canonical_name("scheduled_one_off_failure_policy"))
        .expect("scheduled policy metadata");
    metadata.last_failure = Some("previous scheduled failure".to_owned());
    cassie.midge.put_retention_policy(&metadata).unwrap();
    cassie.catalog.register_retention_policy(metadata);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_one_off_failure_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap_err()
        .to_string();
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_one_off_failure_policy");

    // Assert
    assert!(!error.is_empty());
    assert_eq!(progress[0][0], Value::String("active".to_owned()));
    assert_eq!(progress[0][2], Value::Null);
    assert_eq!(progress[0][13], Value::String("error".to_owned()));
    assert!(matches!(&progress[0][7], Value::String(value) if value == &error));
    assert_eq!(
        progress[0][8],
        Value::String("previous scheduled failure".to_owned())
    );
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-03T00:00:00Z");
    assert_eq!(progress[0][6], Value::Null);
}

#[test]
fn should_preserve_last_scheduled_failure_across_successful_replay_and_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("retention_scheduled_sticky_failure");

    // Act
    let first_failure = record_failure_then_successful_replay(&path);
    let recovered = reopen_after_successful_replay(&path, &first_failure);
    let second_failure = record_new_failure_after_restart(&recovered, &first_failure);

    // Assert
    assert_ne!(first_failure, second_failure);
    let RecoveredRetentionPolicy { cassie, session } = recovered;
    drop(session);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

struct RecoveredRetentionPolicy {
    cassie: Cassie,
    session: CassieSession,
}

fn record_failure_then_successful_replay(path: &std::path::Path) -> String {
    let cassie = Cassie::new_with_data_dir(path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_failure_events",
        "scheduled_failure_policy",
        257,
        0,
    );
    let first_batch = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    assert_eq!(first_batch.command(), "ENFORCE RETENTION 256");
    replace_retention_duration(&cassie, "scheduled_failure_policy", "1 fortnight");
    let first_failure = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap_err()
        .to_string();
    let failed_progress = scheduled_policy_progress(&cassie, &session, "scheduled_failure_policy");
    assert_eq!(failed_progress[0][7], Value::String(first_failure.clone()));
    assert_eq!(failed_progress[0][8], Value::String(first_failure.clone()));
    assert_eq!(failed_progress[0][13], Value::String("error".to_owned()));
    cassie
        .execute_sql(
            &session,
            "ALTER RETENTION POLICY scheduled_failure_policy RETAIN FOR '1 day'",
            vec![],
        )
        .unwrap();
    let replay = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    assert_eq!(replay.command(), "ENFORCE RETENTION 1");
    drop(session);
    drop(cassie);
    first_failure
}

fn reopen_after_successful_replay(
    path: &std::path::Path,
    expected_failure: &str,
) -> RecoveredRetentionPolicy {
    let cassie = Cassie::new_with_data_dir(path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    let progress = scheduled_policy_progress(&cassie, &session, "scheduled_failure_policy");
    assert_eq!(progress[0][7], Value::String(String::new()));
    assert_eq!(progress[0][8], Value::String(expected_failure.to_owned()));
    assert_eq!(progress[0][13], Value::String("ready".to_owned()));
    assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T00:00:00Z");
    RecoveredRetentionPolicy { cassie, session }
}

fn record_new_failure_after_restart(
    recovered: &RecoveredRetentionPolicy,
    previous_failure: &str,
) -> String {
    replace_retention_duration(
        &recovered.cassie,
        "scheduled_failure_policy",
        "invalid duration",
    );
    let second_failure = recovered
        .cassie
        .execute_sql(
            &recovered.session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy DUE AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap_err()
        .to_string();
    let progress = scheduled_policy_progress(
        &recovered.cassie,
        &recovered.session,
        "scheduled_failure_policy",
    );
    assert_ne!(second_failure, previous_failure);
    assert_eq!(progress[0][7], Value::String(second_failure.clone()));
    assert_eq!(progress[0][8], Value::String(second_failure.clone()));
    assert_eq!(progress[0][13], Value::String("error".to_owned()));
    assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(&progress[0], 1, 9, "2026-01-04T00:00:00Z");
    second_failure
}
