#[test]
fn should_classify_spatial_direction_overlap_relations() {
    // Arrange
    let sql = "SELECT '((0,0),(2,2))'::box << '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box >> '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box <<| '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box |>> '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box &< '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box &> '((4,0),(6,2))'::box, \
                      '((0,0),(2,2))'::box &<| '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box |&> '((0,4),(2,6))'::box, \
                      '((0,0),(3,3))'::box && '((2,2),(4,4))'::box, \
                      '((0,0),(2,2))'::box && '((4,4),(6,6))'::box, \
                      '<(0,0),1>'::circle << '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle >> '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle <<| '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle |>> '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle &< '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle &> '<(4,0),1>'::circle, \
                      '<(0,0),1>'::circle &<| '<(0,4),1>'::circle, \
                      '<(0,0),1>'::circle |&> '<(0,4),1>'::circle, \
                      '<(0,0),2>'::circle && '<(3,0),2>'::circle, \
                      '<(0,0),2>'::circle && '<(5,0),2>'::circle, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon << \
                          '((4,0),(6,0),(6,2),(4,2))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon >> \
                          '((4,0),(6,0),(6,2),(4,2))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon <<| \
                          '((0,4),(2,4),(2,6),(0,6))'::polygon, \
                      '((0,0),(2,0),(2,2),(0,2))'::polygon |>> \
                          '((0,4),(2,4),(2,6),(0,6))'::polygon, \
                      '((0,0),(3,0),(3,3),(0,3))'::polygon && \
                          '((2,2),(4,2),(4,4),(2,4))'::polygon, \
                      '((0,0),(3,0),(3,3),(0,3))'::polygon && \
                          '((5,5),(7,5),(7,7),(5,7))'::polygon, \
                      point '(0,0)' << point '(1,0)', \
                      point '(0,0)' >> point '(1,0)', \
                      point '(0,0)' <<| point '(0,1)', \
                      point '(0,0)' |>> point '(0,1)', \
                      '((0,0),(2,2))'::box <^ '((0,4),(2,6))'::box, \
                      '((0,0),(2,2))'::box >^ '((0,4),(2,6))'::box, \
                      point '(0,0)' <^ point '(0,1)', \
                      point '(0,0)' >^ point '(0,1)'";

    // Act
    let frames = execute_geometry_query("issue-907-directional-overlap-operators", sql);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(geometry_result_types(&frames), vec![(16, 1, -1, 0); 34]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}
