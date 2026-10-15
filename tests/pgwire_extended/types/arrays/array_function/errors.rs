//! Residual PostgreSQL array search function behavior for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_reject_element_search_in_multidimensional_arrays_with_0a000() {
    // Arrange
    let position = fixture::execute_cycle(
        "SELECT array_position(ARRAY[[1,2],[3,4]], 2)",
        None,
        0,
        false,
    );
    let positions = fixture::execute_cycle(
        "SELECT array_positions(ARRAY[[1,2],[3,4]], 2)",
        None,
        0,
        false,
    );
    let recovery = fixture::execute_cycle("SELECT 47 AS recovery", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-807-multidimensional-array-search-recovery",
        &[],
        vec![position, positions, recovery],
    );

    // Assert
    for frames in &batches[..2] {
        assert_eq!(wire::error_code(frames).as_deref(), Some("0A000"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        let fields = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .expect("multidimensional array search ErrorResponse");
        assert!(fields.iter().any(|(kind, value)| {
            *kind == 'M'
                && value == "searching for elements in multidimensional arrays is not supported"
        }));
        assert_eq!(fixture::data_row_payloads(frames), Vec::<Vec<u8>>::new());
    }
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("47".to_owned())]]
    );
}

include!("null_search.rs");

include!("search_positions.rs");
