//! Explicit physical server file policy, with bounded harmless inputs.

use crate::support_backend_sessions::assert_rows;
use crate::support_catalog_policy as policy;

const FILE_CALLS: &[(&str, &str)] = &[
    (
        "pg_ls_dir(text,boolean,boolean)",
        "SELECT * FROM pg_ls_dir('cassie-harmless-fixture', true, false)",
    ),
    ("pg_ls_logdir()", "SELECT * FROM pg_ls_logdir()"),
    ("pg_ls_waldir()", "SELECT * FROM pg_ls_waldir()"),
    ("pg_ls_tmpdir()", "SELECT * FROM pg_ls_tmpdir()"),
    (
        "pg_ls_archive_statusdir()",
        "SELECT * FROM pg_ls_archive_statusdir()",
    ),
    (
        "pg_ls_logicalmapdir()",
        "SELECT * FROM pg_ls_logicalmapdir()",
    ),
    (
        "pg_ls_logicalsnapdir()",
        "SELECT * FROM pg_ls_logicalsnapdir()",
    ),
    (
        "pg_ls_replslotdir(text)",
        "SELECT * FROM pg_ls_replslotdir('cassie_harmless_slot')",
    ),
    ("pg_ls_summariesdir()", "SELECT * FROM pg_ls_summariesdir()"),
    (
        "pg_read_file(text,bigint,bigint,boolean)",
        "SELECT pg_read_file('cassie-harmless-fixture.txt', 0, 1, true)",
    ),
    (
        "pg_read_binary_file(text,bigint,bigint,boolean)",
        "SELECT pg_read_binary_file('cassie-harmless-fixture.txt', 0, 1, true)",
    ),
    (
        "pg_stat_file(text,boolean)",
        "SELECT * FROM pg_stat_file('cassie-harmless-fixture.txt', true)",
    ),
];

#[test]
fn should_enforce_file_access_policy_given_bounded_harmless_inputs_when_server_file_helpers_are_called(
) {
    // Arrange
    // A missing symbol is not the selected policy: prove every signature
    // exists before requiring its deliberate 0A000 rejection.
    let controls = FILE_CALLS
        .iter()
        .map(|(signature, _)| {
            format!("SELECT to_regprocedure('pg_catalog.{signature}') IS NOT NULL")
        })
        .collect::<Vec<_>>();
    let mut statements = controls.iter().map(String::as_str).collect::<Vec<_>>();
    statements.extend(FILE_CALLS.iter().map(|(_, sql)| *sql));
    statements.push("SELECT 42");

    // Act
    let batches = policy::run("physical-server-file-policy", &statements);

    // Assert
    for frames in &batches[..FILE_CALLS.len()] {
        policy::assert_registered(frames);
    }
    for frames in &batches[FILE_CALLS.len()..2 * FILE_CALLS.len()] {
        policy::assert_error(frames, "0A000");
    }
    assert_rows(batches.last().expect("recovery"), &[&["42"]]);
}
