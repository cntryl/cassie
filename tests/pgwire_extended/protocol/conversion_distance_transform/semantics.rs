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
fn should_return_postgresql_shape_to_shape_distance_overloads() {
    // Arrange
    let sql = "SELECT '((0,0),(2,2))'::box <-> '((5,0),(7,2))'::box, \
                      '[(0,0),(2,0)]'::lseg <-> '[(5,0),(7,0)]'::lseg, \
                      '{1,0,0}'::line <-> '{1,0,-3}'::line, \
                      '[(0,0),(2,0)]'::path <-> '[(5,0),(7,0)]'::path, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon \
                          <-> '((5,0),(7,0),(7,2),(5,2))'::polygon, \
                      '<(0,0),1>'::circle <-> '<(5,0),2>'::circle, \
                      '((0,0),(2,2))'::box <-> '[(5,0),(5,2)]'::lseg, \
                      '[(0,0),(2,0)]'::lseg <-> '{1,0,-5}'::line, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon \
                          <-> '<(5,1),1>'::circle, \
                      '[(5,0),(5,2)]'::lseg <-> '((0,0),(2,2))'::box, \
                      '{1,0,-5}'::line <-> '[(0,0),(2,0)]'::lseg, \
                      '<(5,1),1>'::circle \
                          <-> '((0,0),(2,0),(2,2),(0,2))'::polygon, \
                      '((0,0),(2,2))'::box <-> point '(3,0)', \
                      '[(0,0),(2,0)]'::lseg <-> point '(3,0)', \
                      '{0,1,0}'::line <-> point '(3,4)', \
                      '[(0,0),(2,0)]'::path <-> point '(3,0)', \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon <-> point '(3,0)', \
                      '<(0,0),1>'::circle <-> point '(3,0)'";

    // Act
    let frames = execute_geometry_query("issue-907-shape-to-shape-distances", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(701, 8, -1, 0); 18]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("5".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("2".to_owned()),
            Some("1".to_owned()),
            Some("1".to_owned()),
            Some("4".to_owned()),
            Some("1".to_owned()),
            Some("1".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

include!("../../types/geometry/conversion_distance_transform/semantics.rs");
