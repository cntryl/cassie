//! Same prepared statement, repeated dynamic bounds and exact finite result rows.
use crate::support_relational_qualification::{cycle, fixture, run_wire, Parameter};

fn parameter(value: Option<i64>, format: i16) -> Parameter {
    Parameter {
        oid: 20,
        format,
        hex: if format == 1 {
            value.map(|value| format!("{value:016x}"))
        } else {
            None
        },
        utf8: if format == 0 {
            value.map(|value| value.to_string())
        } else {
            None
        },
    }
}

#[test]
fn should_rebind_dynamic_bounds_without_reusing_previous_values() {
    // Arrange
    let fixture = fixture();
    let sql = "SELECT id FROM r WHERE n>=0 ORDER BY n,id LIMIT $1 OFFSET $2";
    let cases = [
        (Some(2_i64), Some(1_i64), vec![1_i64, 2]),
        (Some(1), Some(2), vec![2]),
        (Some(0), Some(0), vec![]),
        (None, Some(0), vec![5, 1, 2, 6]),
        (Some(2), None, vec![5, 1]),
    ];
    let mut cycles = Vec::new();
    for format in [0, 1] {
        for (index, (limit, offset, _)) in cases.iter().enumerate() {
            let parameters = [limit, offset]
                .into_iter()
                .map(|value| parameter(*value, format))
                .collect::<Vec<_>>();
            let mut packets = cycle(sql, &parameters, format);
            if index > 0 {
                packets.drain(..2);
            }
            cycles.push(packets);
        }
    }

    // Act
    let results = run_wire(&fixture, cycles);

    // Assert
    assert_eq!(results.len(), 10);
    for (format, group) in [0, 1].into_iter().zip(results.as_chunks::<5>().0) {
        for (index, ((limit, offset, expected), frames)) in cases.iter().zip(group).enumerate() {
            assert_eq!(crate::support_pgwire::error_code(frames), None);
            let values = if format == 0 {
                crate::support_pgwire::data_rows(frames)
                    .into_iter()
                    .map(|row| {
                        assert_eq!(row.len(), 1);
                        row[0]
                            .as_ref()
                            .expect("id not NULL")
                            .parse::<i64>()
                            .expect("text BIGINT")
                    })
                    .collect::<Vec<_>>()
            } else {
                frames
                    .iter()
                    .filter(|(tag, _)| *tag == b'D')
                    .map(|(_, payload)| {
                        assert_eq!(&payload[..6], &[0, 1, 0, 0, 0, 8]);
                        assert_eq!(payload.len(), 14);
                        i64::from_be_bytes(payload[6..14].try_into().expect("binary BIGINT"))
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(&values, expected, "format{format} {limit:?}/{offset:?}");
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert_eq!(descriptions.len(), if index == 0 { 2 } else { 1 });
            for (position, description) in descriptions.iter().enumerate() {
                assert_eq!(description.len(), 1);
                let actual = &description[0];
                let expected_format = if index == 0 && position == 0 {
                    0
                } else {
                    format
                };
                assert_eq!(
                    (
                        actual.name.as_str(),
                        actual.type_oid,
                        actual.type_size,
                        actual.type_mod,
                        actual.format_code
                    ),
                    ("id", 20, 8, -1, expected_format)
                );
            }
            if index == 0 {
                let types = frames
                    .iter()
                    .find(|(tag, _)| *tag == b't')
                    .expect("initial prepared parameter description");
                assert_eq!(
                    crate::support_pgwire::parse_parameter_description(&types.1),
                    vec![20, 20]
                );
            } else {
                assert!(!frames.iter().any(|(tag, _)| *tag == b't'));
            }
            println!("rebind format{format} {limit:?}/{offset:?}: {values:?}");
        }
    }
}
