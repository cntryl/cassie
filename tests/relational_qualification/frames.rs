//! Independent finite peer-group and exclusion truth tables.
use crate::support_relational_qualification::{cycle, fixture, matches_row, run_wire, Fixture};
use serde::Deserialize;

#[derive(Deserialize)]
struct FrameRecords {
    cases: Vec<FrameCase>,
}

#[derive(Deserialize)]
struct FrameCase {
    name: String,
    sql: String,
    rows: Vec<Vec<serde_json::Value>>,
    columns: Vec<String>,
    oids: Vec<i32>,
}

fn cases() -> Vec<FrameCase> {
    serde_json::from_str::<FrameRecords>(include_str!(
        "../support/relational_qualification/frame_records.json"
    ))
    .expect("independently derived frame records")
    .cases
}

fn seed(fixture: &Fixture) {
    for sql in [
        "CREATE TABLE qualification_frames (id BIGINT,n BIGINT)",
        "INSERT INTO qualification_frames (id,n) VALUES (1,10),(2,20),(3,20),(4,30)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("frame seed");
    }
}

#[test]
fn should_match_literal_frame_endpoints() {
    // Arrange
    let fixture = fixture();
    seed(&fixture);
    let cases = cases();
    assert_eq!(cases.len(), 2);

    // Act
    let results = cases
        .iter()
        .map(|case| {
            fixture
                .cassie
                .execute_sql(&fixture.session, &case.sql, vec![])
        })
        .collect::<Vec<_>>();

    // Assert
    for (case, result) in cases.iter().zip(results) {
        let result = result.unwrap_or_else(|error| panic!("{}: {error}", case.name));
        println!(
            "frame {}: row_count={} column_count={}",
            case.name,
            result.rows.len(),
            result.columns.len()
        );
        assert_eq!(result.rows.len(), case.rows.len(), "{}", case.name);
        assert!(
            result
                .rows
                .iter()
                .zip(&case.rows)
                .all(|(row, literal)| matches_row(row, literal)),
            "{}",
            case.name
        );
        assert_eq!(
            result
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            case.columns.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(
            result
                .columns
                .iter()
                .map(|c| c.type_oid)
                .collect::<Vec<_>>(),
            case.oids
                .iter()
                .map(|oid| i64::from(*oid))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn should_describe_frame_results_in_both_formats() {
    // Arrange
    let fixture = fixture();
    seed(&fixture);
    let cases = cases();
    let cycles = cases
        .iter()
        .flat_map(|case| [cycle(&case.sql, &[], 0), cycle(&case.sql, &[], 1)])
        .collect();

    // Act
    let results = run_wire(&fixture, cycles);

    // Assert
    assert_eq!(results.len(), 4);
    for (case, pair) in cases.iter().zip(results.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(
                crate::support_pgwire::error_code(frames),
                None,
                "{}",
                case.name
            );
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert!(descriptions.len() >= 2);
            for (index, description) in descriptions.iter().enumerate() {
                assert_eq!(description.len(), case.columns.len());
                for ((actual, name), oid) in description.iter().zip(&case.columns).zip(&case.oids) {
                    assert_eq!(
                        (
                            &actual.name,
                            actual.type_oid,
                            actual.type_size,
                            actual.type_mod,
                            actual.format_code
                        ),
                        (name, *oid, 8, -1, if index == 0 { 0 } else { format }),
                        "{}",
                        case.name
                    );
                }
            }
        }
    }
}
