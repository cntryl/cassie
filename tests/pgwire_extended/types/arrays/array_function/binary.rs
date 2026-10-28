//! Direct PostgreSQL array function contracts not covered by the scalar-array matrix.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[path = "../array_order_functions/nulls.rs"]
mod array_order_functions;

#[test]
fn should_round_trip_multidimensional_nondefault_bounds_in_binary_format() {
    // Arrange
    let input = b"\0\0\0\x02\0\0\0\x01\0\0\0\x17\0\0\0\x02\0\0\0\0\0\0\0\x02\xff\xff\xff\xff\0\0\0\x04\0\0\0\x01\xff\xff\xff\xff\0\0\0\x04\0\0\0\x03\0\0\0\x04\0\0\0\x04";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| {
            fixture::execute_cycle(
                "SELECT $1::INT4[] AS value",
                Some((1_007, 1, Some(input))),
                format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-multidimensional-binary-bounds",
        &[],
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (1_007, -1, -1), format, true);
        if format == 0 {
            assert_eq!(
                wire::data_rows(&frames),
                vec![vec![Some("[0:1][-1:0]={{1,NULL},{3,4}}".to_owned())]]
            );
        } else {
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![fixture::one_field_row(input)]
            );
        }
    }
}

include!("../../../set_returning/array_function/resources.rs");

include!("../../../set_returning/array_function/semantics.rs");

include!("semantics.rs");
