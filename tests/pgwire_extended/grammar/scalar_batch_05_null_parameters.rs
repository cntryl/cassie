//! Declared Boolean NULL parameters remain typed and truth predicates never emit NULL.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_truth_table_given_null_boolean_parameters_when_formats_are_crossed() {
    // Arrange
    let cases = [
        ("TRUE", false),
        ("NOT TRUE", true),
        ("FALSE", false),
        ("NOT FALSE", true),
        ("UNKNOWN", true),
        ("NOT UNKNOWN", false),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(target, _)| {
            [0, 1].into_iter().flat_map(move |input| {
                [0, 1].map(move |output| {
                    fixture::execute_cycle(
                        &format!("SELECT $1 IS {target} AS output"),
                        Some((16, input, None)),
                        output,
                        true,
                    )
                })
            })
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("truth-null-parameters", &[], cycles);
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (16, 1, -1),
            i16::try_from(index % 2).expect("format"),
            true,
        );
        let expected = if index % 2 == 0 {
            if cases[index / 4].1 {
                b"t".as_slice()
            } else {
                b"f".as_slice()
            }
        } else if cases[index / 4].1 {
            &[1]
        } else {
            &[0]
        };
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(expected)]
        );
        let body = &frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description")
            .1;
        assert_eq!(wire::parse_parameter_description(body), vec![16]);
    }
}
