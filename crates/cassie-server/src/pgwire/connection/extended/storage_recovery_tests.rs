//! Admission precedes every retained portal or descriptor fast path.

use super::*;
use crate::config::CassieRuntimeConfig;

fn fixture(sql: &str) -> (Arc<Cassie>, CassieSession, SessionState) {
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let mut state = SessionState::new(&cassie.runtime.limits());
    let parsed = crate::sql::parse_statement(sql).expect("statement");
    state.prepared_statements.insert(
        "source".to_owned(),
        PreparedStatement {
            id: 1,
            name: "source".to_owned(),
            query: sql.to_owned(),
            sql_fingerprint: crate::runtime::sql_fingerprint(&parsed),
            parsed: Some(parsed),
            parameter_count: 0,
            parameter_types: Vec::new(),
            described: false,
        },
    );
    state.portals.insert(
        "retained".to_owned(),
        Portal {
            name: "retained".to_owned(),
            statement_name: "source".to_owned(),
            prepared_id: 1,
            params: Vec::new(),
            parameter_memory: Vec::new(),
            result_formats: Vec::new(),
            described: false,
            suspended: None,
            completed_command: None,
        },
    );
    (cassie, session, state)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

#[test]
fn should_reject_retained_portal_output_after_storage_requires_recovery() {
    // Arrange
    for suspended in [false, true] {
        let (cassie, session, mut state) = fixture("SELECT 1");
        let portal = state.portals.get_mut("retained").expect("portal");
        if suspended {
            let mut builder =
                QueryResult::try_builder(&state.portal_memory_controls).expect("result");
            builder
                .try_push_column(&ColumnMeta::text("value"))
                .expect("column");
            builder
                .try_push_row(&[Value::String("retained".to_owned())])
                .expect("row");
            portal.suspended = Some(PortalSuspended {
                result: builder.finish().expect("owned result"),
                row_offset: 0,
                row_description_sent: true,
                streaming: false,
                rows_emitted: 0,
                cancellation: None,
                portal_memory: None,
            });
        } else {
            portal.completed_command = Some("SELECT 1".to_owned());
        }
        cassie.midge.latch_storage_recovery_required_for_test();
        let mut writer = Vec::new();

        // Act
        let result = runtime().block_on(handle_execute(
            Arc::clone(&cassie),
            &cassie.runtime,
            &mut writer,
            &mut state,
            &session,
            "retained".to_owned(),
            1,
        ));

        // Assert
        assert_eq!(
            result.expect_err("new Execute rejected").pg_error.code,
            "58030"
        );
        assert_eq!(writer.as_slice(), &[] as &[u8]);
        let portal = state
            .portals
            .get("retained")
            .expect("admission did not consume portal");
        assert_eq!(portal.suspended.is_some(), suspended);
        assert_eq!(portal.completed_command.is_some(), !suspended);
    }
}

#[test]
fn should_reject_bind_before_retaining_a_new_portal_after_quarantine() {
    // Arrange
    let (cassie, session, mut state) = fixture("SELECT 1");
    cassie.midge.latch_storage_recovery_required_for_test();
    let mut writer = Vec::new();

    // Act
    let result = runtime().block_on(handle_bind(
        Arc::clone(&cassie),
        &cassie.runtime,
        &mut writer,
        &mut state,
        &session,
        BindRequest {
            portal: "new".to_owned(),
            statement: "source".to_owned(),
            parameter_formats: Vec::new(),
            parameters: Vec::new(),
            result_formats: Vec::new(),
        },
    ));

    // Assert
    assert_eq!(result.expect_err("Bind rejected").pg_error.code, "58030");
    assert!(!state.portals.contains_key("new"));
    assert!(state.prepared_result_descriptors.is_empty());
    assert_eq!(writer.as_slice(), &[] as &[u8]);
}

#[test]
fn should_reject_empty_prepared_describe_after_quarantine() {
    // Arrange
    let (cassie, session, mut state) = fixture("SELECT 1");
    let mut prepared = state
        .prepared_statements
        .remove("source")
        .expect("prepared");
    prepared.parsed = None;
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let result = runtime().block_on(describe_prepared(cassie, session, prepared));

    // Assert
    assert_eq!(
        result.expect_err("Describe rejected").pg_error.code,
        "58030"
    );
}

#[test]
fn should_reject_empty_parse_after_quarantine() {
    // Arrange
    let (cassie, session, mut state) = fixture("SELECT 1");
    cassie.midge.latch_storage_recovery_required_for_test();
    let mut writer = Vec::new();

    // Act
    let result = runtime().block_on(handle_parse(
        Arc::clone(&cassie),
        &cassie.runtime,
        &mut writer,
        &mut state,
        &session,
        ParseRequest {
            name: "empty".to_owned(),
            query: String::new(),
            parameter_type_oids: Vec::new(),
        },
    ));

    // Assert
    assert_eq!(result.expect_err("Parse rejected").pg_error.code, "58030");
    assert!(!state.prepared_statements.contains_key("empty"));
    assert_eq!(writer.as_slice(), &[] as &[u8]);
}
