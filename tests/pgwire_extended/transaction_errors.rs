//! Wire `PostgreSQL` 18 transaction-error SQLSTATEs and `ReadyForQuery` status bytes.

use super::support_parameter_occurrence_wire::{Step, run_script};
use super::support_pgwire as wire;
use super::support_type_metadata_contract::Frames;

#[derive(Clone, Copy)]
enum Mode {
    Simple,
    ExtendedText,
    ExtendedBinary,
}

const MODES: [Mode; 3] = [Mode::Simple, Mode::ExtendedText, Mode::ExtendedBinary];

const SETUP: &str = "CREATE TABLE wire_tx_errors (id INT PRIMARY KEY)";

fn step(mode: Mode, sql: &str) -> Step {
    let cycle = match mode {
        Mode::Simple => vec![wire::simple_query_frame(sql)],
        Mode::ExtendedText | Mode::ExtendedBinary => {
            let format = i16::from(matches!(mode, Mode::ExtendedBinary));
            vec![
                wire::parse_frame("", sql),
                wire::bind_frame_with_formats("", "", &[], &[], &[format]),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]
        }
    };
    Step::ThroughReady(cycle)
}

/// SQLSTATE, completed command tags and the `ReadyForQuery` status byte.
type Outcome = (Option<String>, Vec<String>, u8);

fn outcome(frames: &Frames) -> Outcome {
    let commands = frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(b"\0").unwrap_or(payload)).into_owned()
        })
        .collect();
    let status = frames
        .last()
        .filter(|(tag, _)| *tag == b'Z')
        .and_then(|(_, payload)| payload.first().copied())
        .unwrap_or(0);
    (wire::error_code(frames), commands, status)
}

fn ok(command: &str, status: u8) -> Outcome {
    (None, vec![command.to_string()], status)
}

fn failed(code: &str, status: u8) -> Outcome {
    (Some(code.to_string()), Vec::new(), status)
}

fn run(label: &str, mode: Mode, statements: &[&str], observer_sql: &str) -> (Vec<Outcome>, Frames) {
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(SETUP)])];
    steps.extend(statements.iter().map(|sql| step(mode, sql)));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        observer_sql,
    )]));
    let mut batches = run_script(label, steps);
    let observation = batches.pop().expect("observer batch");
    let outcomes = batches.iter().skip(1).map(outcome).collect();
    (outcomes, observation)
}

fn run_mutation_returning_order(
    label: &str,
    mode: Mode,
    seed_first_candidate: bool,
    values: &str,
) -> (Vec<Outcome>, Frames) {
    let mut statements = vec![
        "CREATE TABLE wire_tx_ordering_target (id BIGINT, unique_value BIGINT UNIQUE)".to_owned(),
        "CREATE TABLE wire_tx_ordering_error (n BIGINT)".to_owned(),
    ];
    if seed_first_candidate {
        statements.push("INSERT INTO wire_tx_ordering_target VALUES (1, 7)".to_owned());
    }
    statements.extend([
        "BEGIN".to_owned(),
        "INSERT INTO wire_tx_ordering_error VALUES (0)".to_owned(),
        format!(
            "INSERT INTO wire_tx_ordering_target VALUES {values} \
             RETURNING EXISTS (SELECT 1 FROM wire_tx_ordering_error e WHERE e.n / 0 = 0)"
        ),
        "ROLLBACK".to_owned(),
    ]);
    let statement_refs = statements.iter().map(String::as_str).collect::<Vec<_>>();
    run(
        label,
        mode,
        &statement_refs,
        "SELECT (SELECT COUNT(*) FROM wire_tx_ordering_target), \
                (SELECT COUNT(*) FROM wire_tx_ordering_error)",
    )
}

#[test]
fn should_ignore_failed_wire_block_work_until_commit_rolls_back() {
    // Arrange
    let statements = [
        "BEGIN",
        "INSERT INTO wire_tx_errors (id) VALUES (1)",
        "INSERT INTO wire_tx_errors (id) VALUES (1)",
        "SELECT id FROM wire_tx_errors",
        "SAVEPOINT late",
        "COMMIT",
    ];

    // Act
    let runs = MODES.map(|mode| {
        run(
            "wire-tx-failed-commit",
            mode,
            &statements,
            "SELECT COUNT(*) FROM wire_tx_errors",
        )
    });

    // Assert
    for (outcomes, observed) in runs {
        assert_eq!(
            outcomes,
            vec![
                ok("BEGIN", b'T'),
                ok("INSERT 0 1", b'T'),
                failed("23505", b'E'),
                failed("25P02", b'E'),
                failed("25P02", b'E'),
                ok("ROLLBACK", b'I'),
            ]
        );
        assert_eq!(wire::error_code(&observed), None);
        assert_eq!(
            wire::data_rows(&observed),
            vec![vec![Some("0".to_string())]]
        );
    }
}

#[test]
fn should_report_savepoint_errors_across_wire_protocols() {
    // Arrange
    let statements = [
        "SAVEPOINT idle_point",
        "ROLLBACK TO SAVEPOINT idle_point",
        "RELEASE SAVEPOINT idle_point",
        "BEGIN",
        "ROLLBACK TO SAVEPOINT missing",
        "ROLLBACK",
        "BEGIN",
        "RELEASE SAVEPOINT missing",
        "ROLLBACK",
    ];

    // Act
    let runs = MODES.map(|mode| {
        run(
            "wire-tx-savepoint-errors",
            mode,
            &statements,
            "SELECT COUNT(*) FROM wire_tx_errors",
        )
    });

    // Assert
    for (outcomes, _) in runs {
        assert_eq!(
            outcomes,
            vec![
                failed("25P01", b'I'),
                failed("25P01", b'I'),
                failed("25P01", b'I'),
                ok("BEGIN", b'T'),
                failed("3B001", b'E'),
                ok("ROLLBACK", b'I'),
                ok("BEGIN", b'T'),
                failed("3B001", b'E'),
                ok("ROLLBACK", b'I'),
            ]
        );
    }
}

#[test]
fn should_recover_failed_wire_blocks_through_existing_savepoints() {
    // Arrange
    let statements = [
        "BEGIN",
        "INSERT INTO wire_tx_errors (id) VALUES (1)",
        "SAVEPOINT before_dup",
        "INSERT INTO wire_tx_errors (id) VALUES (1)",
        "ROLLBACK TO SAVEPOINT before_dup",
        "INSERT INTO wire_tx_errors (id) VALUES (2)",
        "COMMIT",
    ];

    // Act
    let runs = MODES.map(|mode| {
        run(
            "wire-tx-savepoint-recovery",
            mode,
            &statements,
            "SELECT id FROM wire_tx_errors ORDER BY id",
        )
    });

    // Assert
    for (outcomes, observed) in runs {
        assert_eq!(
            outcomes,
            vec![
                ok("BEGIN", b'T'),
                ok("INSERT 0 1", b'T'),
                ok("SAVEPOINT", b'T'),
                failed("23505", b'E'),
                ok("ROLLBACK", b'T'),
                ok("INSERT 0 1", b'T'),
                ok("COMMIT", b'I'),
            ]
        );
        assert_eq!(
            wire::data_rows(&observed),
            vec![vec![Some("1".to_string())], vec![Some("2".to_string())]]
        );
    }
}

#[test]
fn should_skip_statements_after_a_failure_inside_one_simple_query_block() {
    // Arrange
    let statements = [
        "BEGIN; INSERT INTO wire_tx_errors (id) VALUES (1); SELECT 1 / 0; COMMIT",
        "SELECT 1",
        "COMMIT",
    ];

    // Act
    let (outcomes, observed) = run(
        "wire-tx-batch-failure",
        Mode::Simple,
        &statements,
        "SELECT COUNT(*) FROM wire_tx_errors",
    );

    // Assert
    assert_eq!(
        outcomes,
        vec![
            (
                Some("22012".to_string()),
                vec!["BEGIN".to_string(), "INSERT 0 1".to_string()],
                b'E'
            ),
            failed("25P02", b'E'),
            ok("ROLLBACK", b'I'),
        ]
    );
    assert_eq!(
        wire::data_rows(&observed),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_evaluate_each_insert_returning_row_before_a_later_unique_conflict() {
    // Arrange
    let cases = [
        ("later-conflict", false, "(1, 7), (2, 7)", "22012", 0),
        ("preexisting-conflict", true, "(1, 7), (2, 8)", "23505", 1),
        (
            "distinct-runtime-error",
            false,
            "(1, 7), (2, 8)",
            "22012",
            0,
        ),
    ];
    let mut evidence = Vec::new();

    // Act
    for (case_name, seed_first_candidate, values, error_code, target_rows) in cases {
        for (mode_index, mode) in MODES.into_iter().enumerate() {
            let label = format!("issue823-returning-order-{case_name}-{mode_index}");
            evidence.push((
                label.clone(),
                error_code,
                target_rows,
                run_mutation_returning_order(&label, mode, seed_first_candidate, values),
                seed_first_candidate,
            ));
        }
    }

    // Assert
    for (label, error_code, target_rows, (outcomes, observed), seeded) in evidence {
        let mut expected = vec![ok("CREATE TABLE", b'I'), ok("CREATE TABLE", b'I')];
        if seeded {
            expected.push(ok("INSERT 0 1", b'I'));
        }
        expected.extend([
            ok("BEGIN", b'T'),
            ok("INSERT 0 1", b'T'),
            failed(error_code, b'E'),
            ok("ROLLBACK", b'I'),
        ]);
        assert_eq!(outcomes, expected, "{label}");
        assert_eq!(
            wire::data_rows(&observed),
            vec![vec![Some(target_rows.to_string()), Some("0".to_owned())]],
            "{label}: the failed INSERT statement and its transaction-local RETURNING input must roll back"
        );
    }
}

#[test]
fn should_roll_back_implicit_blocks_that_attempt_savepoints() {
    // Arrange
    let simple = vec![wire::simple_query_frame(
        "INSERT INTO wire_tx_errors (id) VALUES (1); SAVEPOINT implicit_point",
    )];
    let extended = vec![
        wire::parse_frame("", "INSERT INTO wire_tx_errors (id) VALUES (2)"),
        wire::bind_frame("", "", &[]),
        wire::execute_frame(""),
        wire::parse_frame("", "RELEASE SAVEPOINT implicit_point"),
        wire::bind_frame("", "", &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let runs = [simple, extended].map(|cycle| {
        let steps = vec![
            Step::ThroughReady(vec![wire::simple_query_frame(SETUP)]),
            Step::ThroughReady(cycle),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT COUNT(*) FROM wire_tx_errors",
            )]),
        ];
        run_script("wire-tx-implicit-savepoint", steps)
    });

    // Assert
    for batches in runs {
        let (code, _, status) = outcome(&batches[1]);
        assert_eq!((code.as_deref(), status), (Some("25P01"), b'I'));
        assert_eq!(
            wire::data_rows(&batches[2]),
            vec![vec![Some("0".to_string())]]
        );
    }
}
