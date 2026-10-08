//! Selected rejection records, including exact PostgreSQL wire SQLSTATEs.
use crate::support_relational_qualification::{cycle, fixture, run_wire, Parameter};
use serde::Deserialize;

#[derive(Deserialize)]
struct Records {
    cases: Vec<Case>,
    d2: Vec<BoundCase>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    sql: String,
    sqlstate: String,
    message_fragment: Option<String>,
}
#[derive(Deserialize)]
struct BoundCase {
    name: String,
    sql: String,
    sqlstate: String,
    oids: Vec<i32>,
    utf8: Vec<String>,
}
fn records() -> Records {
    serde_json::from_str(include_str!(
        "../support/relational_qualification/negative_records.json"
    ))
    .expect("pre-execution negative literals")
}

#[test]
fn should_reject_selected_invalid_relational_inputs() {
    // Arrange
    let fixture = fixture();
    let cases = records().cases;

    // Act
    let results = cases
        .iter()
        .map(|case| {
            fixture
                .cassie
                .execute_sql(&fixture.session, &case.sql, vec![])
        })
        .collect::<Vec<_>>();
    let packets = run_wire(
        &fixture,
        cases.iter().map(|case| cycle(&case.sql, &[], 0)).collect(),
    );
    for ((case, result), frames) in cases.iter().zip(&results).zip(&packets) {
        println!(
            "negative {}: rejected={} wire {:?}",
            case.name,
            result.is_err(),
            crate::support_pgwire::error_code(frames)
        );
    }

    // Assert
    assert_eq!(cases.len(), 6);
    for ((case, result), frames) in cases.iter().zip(results).zip(packets) {
        let error = result.expect_err(&case.name).to_string();
        if let Some(fragment) = &case.message_fragment {
            assert!(error.contains(fragment), "{}: {error}", case.name);
        }
        assert_eq!(
            crate::support_pgwire::error_code(&frames).as_deref(),
            Some(case.sqlstate.as_str()),
            "{}",
            case.name
        );
        assert!(
            !frames.iter().any(|(tag, _)| *tag == b'D' || *tag == b'C'),
            "{} partial output",
            case.name
        );
    }
    assert_eq!(
        fixture
            .cassie
            .execute_sql(&fixture.session, "SELECT id FROM r WHERE id=1", vec![])
            .expect("statement cleanup canary")
            .rows
            .len(),
        1
    );
}

#[test]
fn should_reject_invalid_dynamic_pagination_bounds() {
    // Arrange
    let fixture = fixture();
    let cases = records().d2;
    let cycles = cases
        .iter()
        .map(|case| {
            let params = case
                .oids
                .iter()
                .zip(&case.utf8)
                .map(|(oid, text)| Parameter {
                    oid: *oid,
                    format: 0,
                    hex: None,
                    utf8: Some(text.clone()),
                })
                .collect::<Vec<_>>();
            cycle(&case.sql, &params, 0)
        })
        .collect();

    // Act
    let results = run_wire(&fixture, cycles);
    for (case, frames) in cases.iter().zip(&results) {
        println!(
            "bound negative {}: {:?}",
            case.name,
            crate::support_pgwire::error_code(frames)
        );
    }

    // Assert
    assert_eq!(cases.len(), 4);
    for (case, frames) in cases.iter().zip(results) {
        assert_eq!(
            crate::support_pgwire::error_code(&frames).as_deref(),
            Some(case.sqlstate.as_str()),
            "{}",
            case.name
        );
        assert!(
            !frames.iter().any(|(tag, _)| *tag == b'D' || *tag == b'C'),
            "{} partial output",
            case.name
        );
    }
}
