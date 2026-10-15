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
fn should_compare_line_point_operator_values() {
    // Arrange
    let sql = "SELECT '{1,0,-2}'::line = '{2,0,-4}'::line, \
                      '{1,0,-2}'::line = '{0,1,-2}'::line, \
                      point '(1,2)' <> point '(1,2)', \
                      point '(1,2)' <> point '(2,2)'";

    // Act
    let frames = execute_geometry_query("issue-907-line-point-equality-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 4]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_classify_line_orientation_operators() {
    // Arrange
    let sql = "SELECT ?- '{0,1,-3}'::line, \
                      ?- '[(0,2),(4,2)]'::lseg, \
                      point '(1,2)' ?- point '(4,2)', \
                      point '(1,2)' ?- point '(4,3)', \
                      ?| '{1,0,-3}'::line, \
                      ?| '[(2,0),(2,4)]'::lseg, \
                      point '(2,1)' ?| point '(2,4)', \
                      point '(2,1)' ?| point '(3,4)', \
                      '{1,0,-2}'::line ?|| '{2,0,4}'::line, \
                      '{1,0,-2}'::line ?|| '{0,1,-3}'::line, \
                      '[(0,0),(4,4)]'::lseg ?|| '[(1,0),(3,2)]'::lseg, \
                      '{1,0,-2}'::line ?-| '{0,1,-3}'::line, \
                      '{1,0,-2}'::line ?-| '{2,0,-4}'::line, \
                      '[(0,0),(4,4)]'::lseg ?-| '[(0,4),(4,0)]'::lseg, \
                      '[(0,0),(4,4)]'::lseg ?-| '[(1,0),(3,2)]'::lseg";

    // Act
    let frames = execute_geometry_query("issue-907-orientation-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 15]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

include!("../../protocol/spatial_operator/nulls.rs");

include!("../../protocol/spatial_operator/semantics.rs");

include!("../../relations/spatial_operator/semantics.rs");

include!("../../sequences/spatial_operator/metadata.rs");

include!("../../types/geometry/spatial_operator/semantics.rs");
