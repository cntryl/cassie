#[test]
fn should_emit_outer_join_rows_matching_pgwire_metadata() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("outer-join-wire-shape");
    runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in ["CREATE TABLE wire_users (user_key INT, name TEXT)", "CREATE TABLE wire_orders (order_key INT, total INT)", "INSERT INTO wire_users VALUES (1,'ada'),(2,'grace')"] {
                cassie.execute_sql(&session,sql,vec![]).expect("setup");
            }
            // Act
            for format in [0,1] {
                for sql in ["SELECT * FROM wire_users LEFT JOIN wire_orders ON wire_users.user_key=wire_orders.order_key", "SELECT * FROM wire_orders RIGHT JOIN wire_users ON wire_users.user_key=wire_orders.order_key", "SELECT * FROM wire_users FULL JOIN wire_orders ON wire_users.user_key=wire_orders.order_key"] {
                    let (frames,server)=start_extended_query(cassie.clone(),support::parse_frame("join_stmt",sql),support::bind_frame_with_formats("join_portal","join_stmt",&[],&[],&[format]),support::execute_frame("join_portal")).await;
                    // Assert
                    assert!(!frames.iter().any(|frame| frame.0==b'E'),"{sql}: {frames:?}");
                    let description=frames.iter().find(|frame| frame.0==b'T').expect("metadata");
                    let width=support::parse_row_description(&description.1).len();
                    assert_eq!(width,6);
                    let rows=frames.iter().filter(|frame| frame.0==b'D').map(|frame| read_binary_row(&frame.1)).collect::<Vec<_>>();
                    assert_eq!(rows.len(),2);
                    for row in rows { assert_eq!(row.len(),width,"{sql}"); assert_eq!(row.iter().filter(|value| value.is_none()).count(),3); }
                    server.stop().await;
                }
            }
            drop(cassie);
        });
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_describe_case_result_types_over_pgwire() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("case-result-types");
    runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame("case_stmt", "SELECT CASE WHEN true THEN CAST(2 AS INT) ELSE CAST(3 AS BIGINT) END AS n, CASE WHEN false THEN 'x' ELSE 'y' END AS t, CASE WHEN true THEN CAST(2 AS INT) ELSE 3.5 END AS widened"),
                support::bind_frame_with_formats("case_portal", "case_stmt", &[], &[], &[1]),
                support::execute_frame("case_portal"),
            ).await;

            // Assert
            let description = frames.iter().find(|frame| frame.0 == b'T').expect("row description");
            let fields = support::parse_row_description(&description.1);
            assert_eq!(fields.iter().map(|field| field.type_oid).collect::<Vec<_>>(), vec![20, 25, 1700]);
            let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
            assert_eq!(read_binary_row(&row.1), vec![Some(2_i64.to_be_bytes().to_vec()), Some(b"y".to_vec()), Some(vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 2])]);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
}
