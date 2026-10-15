use super::support_desktop_trace::{
    parse_pinned_trace, parse_trace, replay_trace, validate_trace_for, DesktopClient,
};

const PGADMIN_TRACE: &str = include_str!("../../../fixtures/desktop_traces/pgadmin-9.16.json");
const DBEAVER_TRACE: &str = include_str!("../../../fixtures/desktop_traces/dbeaver-26.1.3.json");

#[test]
fn should_reject_malformed_desktop_trace_json() {
    // Arrange
    let malformed = "[]";

    // Act
    let malformed_error = parse_trace(malformed).expect_err("malformed trace must fail");

    // Assert
    assert!(malformed_error.contains("valid JSON"));
}

#[test]
fn should_reject_desktop_trace_without_workflow_steps() {
    // Arrange
    let incomplete = r#"{
            "schema_version": 1,
            "client": {
                "name": "pgadmin",
                "version": "9.16",
                "driver_version": null,
                "upstream_revision": "888a053231923e6704b56165cf248db056252144"
            },
            "steps": []
        }"#;

    // Act
    let incomplete_error = parse_trace(incomplete).expect_err("empty trace must fail");

    // Assert
    assert!(incomplete_error.contains("workflow steps"));
}

#[test]
fn should_reject_mismatched_desktop_trace_identity_or_digest() {
    // Arrange
    let pgadmin = parse_trace(PGADMIN_TRACE).expect("valid pgAdmin fixture");
    let mismatched_version = parse_trace(&PGADMIN_TRACE.replace("9.16", "9.15"))
        .expect("structurally valid mismatched-version trace");
    let mismatched_revision = parse_trace(&PGADMIN_TRACE.replace(
        "888a053231923e6704b56165cf248db056252144",
        "0000000000000000000000000000000000000000",
    ))
    .expect("structurally valid mismatched-revision trace");

    // Act
    let mismatch_error = validate_trace_for(&pgadmin, DesktopClient::Dbeaver)
        .expect_err("client mismatch must fail");
    let version_error = validate_trace_for(&mismatched_version, DesktopClient::PgAdmin)
        .expect_err("client version mismatch must fail");
    let revision_error = validate_trace_for(&mismatched_revision, DesktopClient::PgAdmin)
        .expect_err("upstream revision mismatch must fail");
    let stale_digest_error = parse_pinned_trace(
        &PGADMIN_TRACE.replace("desktop_pgadmin_docs", "desktop_pgadmin_stale"),
        DesktopClient::PgAdmin,
    )
    .expect_err("modified trace digest must fail");

    // Assert
    assert!(mismatch_error.contains("client identity"));
    assert!(version_error.contains("client identity"));
    assert!(revision_error.contains("client identity"));
    assert!(stale_digest_error.contains("fixture digest"));
}

#[test]
fn should_reject_unsupported_workflow_or_malformed_sqlstate() {
    // Arrange
    let unknown_workflow =
        PGADMIN_TRACE.replace("\"workflow\": \"plan\"", "\"workflow\": \"dashboard\"");
    let malformed_sqlstate = DBEAVER_TRACE.replace(
        "\"expected_sqlstate\": \"42P01\"",
        "\"expected_sqlstate\": \"nope?\"",
    );

    // Act
    let workflow_error =
        parse_trace(&unknown_workflow).expect_err("unknown workflow category must fail");
    let sqlstate_error =
        parse_trace(&malformed_sqlstate).expect_err("malformed SQLSTATE must fail");

    // Assert
    assert!(workflow_error.contains("unsupported workflow"));
    assert!(sqlstate_error.contains("invalid SQLSTATE"));
}

#[test]
fn should_replay_pinned_pgadmin_trace() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let pgadmin = parse_pinned_trace(PGADMIN_TRACE, DesktopClient::PgAdmin)
        .expect("valid pinned pgAdmin fixture");

    // Act
    let result = runtime.block_on(replay_trace(&pgadmin));

    // Assert
    result.expect("pgAdmin trace should replay");
}

#[test]
fn should_replay_pinned_dbeaver_trace() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let dbeaver = parse_pinned_trace(DBEAVER_TRACE, DesktopClient::Dbeaver)
        .expect("valid pinned DBeaver fixture");

    // Act
    let result = runtime.block_on(replay_trace(&dbeaver));

    // Assert
    result.expect("DBeaver trace should replay");
}

#[test]
#[ignore = "selected via CASSIE_DESKTOP_TRACE_CLIENT"]
fn should_replay_selected_desktop_trace() {
    // Arrange
    let selected = std::env::var("CASSIE_DESKTOP_TRACE_CLIENT")
        .expect("CASSIE_DESKTOP_TRACE_CLIENT must select a pinned client");
    let (source, client) = match selected.as_str() {
        "pgadmin-9.16" => (PGADMIN_TRACE, DesktopClient::PgAdmin),
        "dbeaver-26.1.3" => (DBEAVER_TRACE, DesktopClient::Dbeaver),
        other => panic!("unsupported desktop trace client '{other}'"),
    };
    let trace = parse_pinned_trace(source, client).expect("valid selected pinned fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let result = runtime.block_on(replay_trace(&trace));

    // Assert
    result.expect("selected desktop trace should replay");
}
