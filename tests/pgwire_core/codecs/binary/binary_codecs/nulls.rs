#[test]
fn should_encode_stored_empty_array_with_zero_dimensions() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("binary-empty-array-result");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE empty_array_rows (id TEXT, vals INT[])",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO empty_array_rows (id, vals) VALUES ('e', $1)",
                vec![Value::Json(serde_json::json!([]))],
            )
            .expect("insert empty array");

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame("empty_array_select", "SELECT vals FROM empty_array_rows"),
            support::bind_frame_with_formats(
                "empty_array_select_portal",
                "empty_array_select",
                &[],
                &[],
                &[1],
            ),
            support::execute_frame("empty_array_select_portal"),
        )
        .await;

        // Assert
        let rows = frames
            .iter()
            .filter(|frame| frame.0 == b'D')
            .map(|frame| read_binary_row(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(rows, vec![vec![Some(empty_int_array_body())]]);

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_mixed_result_formats_with_null_values() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("binary-mixed-formats");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_result_codecs(&cassie);

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame(
                "mixed_binary_stmt",
                "SELECT nullable, regular FROM binary_codec_docs",
            ),
            support::bind_frame_with_formats(
                "mixed_binary_portal",
                "mixed_binary_stmt",
                &[],
                &[],
                &[0, 1],
            ),
            support::execute_frame("mixed_binary_portal"),
        )
        .await;

        // Assert
        let row_description = frames
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("row description");
        let fields = support::parse_row_description(&row_description.1);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.format_code)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        let row = frames
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("data row");
        assert_eq!(
            read_binary_row(&row.1),
            vec![None, Some(123_456_789_i32.to_be_bytes().to_vec())]
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(b"I".as_slice())
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
