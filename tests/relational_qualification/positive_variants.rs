//! Execute the independently reviewed positive variants with literal bags/order.
use crate::support_relational_qualification::{
    cycle, fixture, matches_row, records, run_wire, Column,
};
use cassie::types::Value;
use serde::Deserialize;

#[derive(Deserialize)]
struct Variant {
    invariant: String,
    sql: String,
    rows: Vec<Vec<serde_json::Value>>,
    comparison: String,
    ordered_descriptor_columns: Vec<Column>,
}

fn variants() -> Vec<Variant> {
    records()
        .variants
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("reviewed positive variant record"))
        .collect()
}

#[test]
fn should_match_positive_variant_literal_rows() {
    // Arrange
    let fixture = fixture();
    let variants = variants();
    assert_eq!(variants.len(), 20);

    // Act
    let results = variants
        .iter()
        .map(|case| {
            fixture
                .cassie
                .execute_sql(&fixture.session, &case.sql, vec![])
        })
        .collect::<Vec<_>>();

    for (case, result) in variants.iter().zip(&results) {
        println!(
            "positive observation {} {}: {result:?}",
            case.invariant, case.sql
        );
    }

    // Assert
    for (case, result) in variants.iter().zip(results) {
        let result =
            result.unwrap_or_else(|error| panic!("{} {}: {error}", case.invariant, case.sql));
        println!(
            "positive {} {}: {:?} {:?}",
            case.invariant, case.sql, result.rows, result.columns
        );
        assert_eq!(result.rows.len(), case.rows.len(), "{}", case.sql);
        assert_eq!(
            result.columns.len(),
            case.ordered_descriptor_columns.len(),
            "{}",
            case.sql
        );
        for (actual, expected) in result.columns.iter().zip(&case.ordered_descriptor_columns) {
            assert_eq!(
                (
                    actual.type_oid,
                    actual.typlen,
                    actual.atttypmod,
                    actual.format_code
                ),
                (i64::from(expected.oid), expected.typlen, expected.typmod, 0),
                "{}",
                case.sql
            );
        }
        if case.comparison == "bag" {
            let mut remaining = result.rows;
            for row in &case.rows {
                let index = remaining
                    .iter()
                    .position(|actual| matches_row(actual, row))
                    .unwrap_or_else(|| panic!("{} missing literal bag member {row:?}", case.sql));
                remaining.remove(index);
            }
            assert_eq!(remaining, Vec::<Vec<Value>>::new());
        } else {
            assert_eq!(case.comparison, "ordered unless singleton/empty");
            assert!(
                result
                    .rows
                    .iter()
                    .zip(&case.rows)
                    .all(|(actual, expected)| matches_row(actual, expected)),
                "{}: ordered literal rows",
                case.sql
            );
        }
    }
}

#[test]
fn should_preserve_positive_variant_wire_descriptors() {
    // Arrange
    let fixture = fixture();
    let variants = variants();
    let cycles = variants
        .iter()
        .flat_map(|case| [cycle(&case.sql, &[], 0), cycle(&case.sql, &[], 1)])
        .collect();

    // Act
    let results = run_wire(&fixture, cycles);

    for (case, pair) in variants.iter().zip(results.as_chunks::<2>().0.iter()) {
        println!(
            "positive wire observation {} {}: {:?}",
            case.invariant,
            case.sql,
            pair.iter()
                .map(|frames| crate::support_pgwire::error_code(frames))
                .collect::<Vec<_>>()
        );
    }

    // Assert
    assert_eq!(results.len(), 40);
    for (case, pair) in variants.iter().zip(results.as_chunks::<2>().0.iter()) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(
                crate::support_pgwire::error_code(frames),
                None,
                "{}",
                case.sql
            );
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert!(descriptions.len() >= 2, "{} Statement/Portal", case.sql);
            for (index, description) in descriptions.iter().enumerate() {
                assert_eq!(
                    description.len(),
                    case.ordered_descriptor_columns.len(),
                    "{}",
                    case.sql
                );
                for (actual, expected) in description.iter().zip(&case.ordered_descriptor_columns) {
                    assert_eq!(
                        (
                            &actual.name,
                            actual.type_oid,
                            actual.type_size,
                            actual.type_mod,
                            actual.format_code
                        ),
                        (
                            &expected.name,
                            expected.oid,
                            expected.typlen,
                            expected.typmod,
                            if index == 0 { 0 } else { format }
                        ),
                        "{}",
                        case.sql
                    );
                }
            }
        }
    }
}

#[test]
fn should_preserve_quoted_cte_prefix_descriptors_after_empty_output() {
    // Arrange
    let fixture = fixture();
    let statements = [
        "WITH c AS (SELECT n,a FROM r WHERE id=3) SELECT q.\"X\",q.a FROM c AS q(\"X\")",
        "WITH c AS (SELECT n,a FROM r WHERE id<0) SELECT q.\"X\",q.a FROM c AS q(\"X\")",
    ];

    // Act
    let results = statements
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    let packets = run_wire(
        &fixture,
        statements
            .iter()
            .flat_map(|sql| [cycle(sql, &[], 0), cycle(sql, &[], 1)])
            .collect(),
    );

    // Assert
    for (index, result) in results.into_iter().enumerate() {
        let result = result.expect("quoted CTE prefix");
        assert_eq!(result.rows.len(), usize::from(index == 0));
        if index == 0 {
            assert_eq!(result.rows, vec![vec![Value::Null, Value::Null]]);
        }
        assert_eq!(
            result
                .columns
                .iter()
                .map(|c| (c.name.as_str(), c.type_oid))
                .collect::<Vec<_>>(),
            vec![("X", 20), ("a", 34025)]
        );
    }
    for pair in packets.as_chunks::<2>().0 {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(crate::support_pgwire::error_code(frames), None);
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert_eq!(descriptions.len(), 2);
            for (index, description) in descriptions.iter().enumerate() {
                assert_eq!(description.len(), 2);
                for (column, (name, oid, length)) in
                    description.iter().zip([("X", 20, 8), ("a", 34025, -1)])
                {
                    assert_eq!(
                        (
                            column.name.as_str(),
                            column.type_oid,
                            column.type_size,
                            column.type_mod,
                            column.format_code
                        ),
                        (name, oid, length, -1, if index == 0 { 0 } else { format })
                    );
                }
            }
        }
    }
}
