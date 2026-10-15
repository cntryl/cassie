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

#[test]
fn should_reject_null_array_position_start_with_postgresql_sqlstate() {
    // Arrange
    let baseline = array_position_start_cycle(Some(b"3"));
    let invalid = array_position_start_cycle(None);
    let recovery = fixture::execute_cycle("SELECT 47 AS recovery", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-807-null-array-position-start-recovery",
        &[],
        vec![baseline, invalid, recovery],
    );

    // Assert
    let baseline_frames = &batches[0];
    fixture::assert_success(baseline_frames);
    fixture::assert_single_column_descriptors(baseline_frames, (23, 4, -1), 0, true);
    assert_eq!(
        wire::data_rows(baseline_frames),
        vec![vec![Some("4".to_owned())]]
    );

    let frames = &batches[1];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22004"));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    let fields = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("null array_position start ErrorResponse");
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'M' && value == "initial position must not be null"));
    assert_eq!(fixture::data_row_payloads(frames), Vec::<Vec<u8>>::new());

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("47".to_owned())]]
    );
}
