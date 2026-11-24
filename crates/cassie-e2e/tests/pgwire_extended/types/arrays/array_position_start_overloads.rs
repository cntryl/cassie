//! Three-argument `array_position` contracts for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

const ARRAY_POSITION_START_SQL: &str = "SELECT array_position($1, $2, $3)";
const ARRAY_WITH_CUSTOM_LOWER_BOUND: &[u8] = b"[0:4]={7,NULL,7,NULL,7}";
const SEARCH_VALUE: &[u8] = b"7";

fn array_position_start_cycle(start: Option<&[u8]>) -> fixture::Cycle {
    let parameters = [
        Some(ARRAY_WITH_CUSTOM_LOWER_BOUND),
        Some(SEARCH_VALUE),
        start,
    ];
    vec![
        wire::parse_frame_with_types("", ARRAY_POSITION_START_SQL, &[1_007, 23, 23]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats("", "", &[0], &parameters, &[0]),
        wire::describe_portal_frame(""),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

#[test]
fn should_apply_array_position_start_subscript_to_custom_lower_bound_arrays() {
    // Arrange
    let cycle = array_position_start_cycle(Some(b"3"));

    // Act
    let batches = fixture::run_cycles("issue-807-array-position-start-subscript", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let parameter_description = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("array_position start overload ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        [1_007, 23, 23]
    );
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("4".to_owned())]]);
}
