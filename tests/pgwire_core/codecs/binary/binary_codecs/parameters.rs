#[test]
fn should_decode_text_array_parameters() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("text-array-parameters");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE text_array_parameters (values INT[])",
                Vec::new(),
            )
            .expect("create array parameter table");

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame_with_types(
                "text_array_parameter_stmt",
                "INSERT INTO text_array_parameters (values) VALUES ($1) RETURNING values",
                &[34_023],
            ),
            support::bind_frame_with_formats(
                "text_array_parameter_portal",
                "text_array_parameter_stmt",
                &[0],
                &[Some(b"{7,8,9}")],
                &[0],
            ),
            support::execute_frame("text_array_parameter_portal"),
        )
        .await;

        // Assert
        let row = frames
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("text array parameter should return a row");
        assert_eq!(
            support::parse_data_row(&row.1),
            vec![Some("[7,8,9]".into())]
        );
        assert!(frames.iter().any(|frame| frame.0 == b'C'));

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
