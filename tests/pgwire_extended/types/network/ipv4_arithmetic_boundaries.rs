use super::*;

#[test]
fn should_preserve_ipv4_arithmetic_boundaries_over_pgwire() {
    // Arrange
    let statement = "ipv4_arithmetic_boundaries";
    let portal = "ipv4_arithmetic_boundaries_binary";
    let sql = "SELECT '255.255.255.254'::inet + 1::bigint AS incremented, '0.0.0.1'::inet - 1::bigint AS decremented, '255.255.255.255'::inet - '0.0.0.0'::inet AS difference";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-ipv4-arithmetic-boundaries", steps);

    // Assert
    let text_result = &batches[0];
    assert_eq!(wire::error_code(text_result), None);
    assert_eq!(
        row_columns(text_result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(869, -1, 0), (869, -1, 0), (20, 8, 0)]
    );
    assert_eq!(
        wire::data_rows(text_result),
        vec![vec![
            Some("255.255.255.255".to_owned()),
            Some("0.0.0.0".to_owned()),
            Some("4294967295".to_owned()),
        ]]
    );

    let binary_result = &batches[2];
    assert_eq!(wire::error_code(binary_result), None);
    assert_eq!(
        row_columns(binary_result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(869, -1, 1), (869, -1, 1), (20, 8, 1)]
    );
    assert_eq!(
        binary_row(binary_result),
        vec![
            vec![2, 32, 0, 4, 255, 255, 255, 255],
            vec![2, 32, 0, 4, 0, 0, 0, 0],
            vec![0, 0, 0, 0, 255, 255, 255, 255],
        ]
    );
}
