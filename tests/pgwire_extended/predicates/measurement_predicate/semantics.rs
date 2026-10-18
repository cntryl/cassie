use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn execute_geometry_query(label: &str, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let batches = run_script(
        label,
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    batches
        .into_iter()
        .next()
        .expect("one geometry query batch")
}

fn geometry_result_types(frames: &[(u8, Vec<u8>)]) -> Vec<(i32, i16, i32, i16)> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("geometry RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect()
}

#[test]
fn should_classify_horizontal_vertical_shape_predicates() {
    // Arrange
    let sql = "SELECT isvertical(point '(2,0)', point '(2,4)'), \
                      isvertical(point '(0,0)', point '(4,3)'), \
                      ishorizontal(point '(0,2)', point '(4,2)'), \
                      ishorizontal(point '(0,0)', point '(4,3)'), \
                      isvertical('{1,0,-2}'::line), \
                      ishorizontal('{0,1,-3}'::line), \
                      isvertical('[(2,0),(2,4)]'::lseg), \
                      ishorizontal('[(0,2),(4,2)]'::lseg)";

    // Act
    let frames = execute_geometry_query("issue-907-horizontal-vertical-predicates", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 8]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

include!("../../protocol/measurement_predicate/semantics.rs");

include!("../../routines/measurement_predicate/semantics.rs");

include!("../../types/geometry/measurement_predicate/semantics.rs");
