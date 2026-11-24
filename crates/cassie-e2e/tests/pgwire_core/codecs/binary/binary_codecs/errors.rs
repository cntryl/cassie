#[test]
fn should_reject_incompatible_coalesce_before_sending_rows_over_pgwire() {
    // Arrange
    let path = support::data_dir("coalesce-wire-types");
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE wire_coalesce (flag BOOLEAN, name TEXT, score INT)",
                vec![],
            )
            .expect("table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO wire_coalesce VALUES (true, 'x', 5), (NULL, 'y', 0)",
                vec![],
            )
            .expect("seed");

        // Act
        for format in [0, 1] {
            for sql in [
                "SELECT COALESCE(flag, name) FROM wire_coalesce",
                "SELECT COALESCE(flag, score) FROM wire_coalesce",
            ] {
                let (frames, server) = start_extended_query(
                    cassie.clone(),
                    support::parse_frame("coalesce_stmt", sql),
                    support::bind_frame_with_formats(
                        "coalesce_portal",
                        "coalesce_stmt",
                        &[],
                        &[],
                        &[format],
                    ),
                    support::execute_frame("coalesce_portal"),
                )
                .await;

                // Assert
                assert!(
                    frames.iter().any(|frame| frame.0 == b'E'),
                    "expected planning error"
                );
                assert!(
                    !frames.iter().any(|frame| matches!(frame.0, b'T' | b'D')),
                    "no metadata or partial result before error"
                );
                server.stop().await;
            }
        }
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame_with_types(
                "coalesce_parameter_stmt",
                "SELECT COALESCE(flag, $1) FROM wire_coalesce",
                &[25],
            ),
            support::bind_frame_with_formats(
                "coalesce_parameter_portal",
                "coalesce_parameter_stmt",
                &[0],
                &[Some(b"text")],
                &[1],
            ),
            support::execute_frame("coalesce_parameter_portal"),
        )
        .await;
        assert!(frames.iter().any(|frame| frame.0 == b'E'));
        assert!(!frames.iter().any(|frame| matches!(frame.0, b'T' | b'D')));
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_binary_array_lengths_that_exceed_the_payload() {
    // Arrange
    let path = support::data_dir("binary-array-length");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let array = [
            1_i32.to_be_bytes().as_slice(),
            0_i32.to_be_bytes().as_slice(),
            23_i32.to_be_bytes().as_slice(),
            i32::MAX.to_be_bytes().as_slice(),
            1_i32.to_be_bytes().as_slice(),
        ]
        .concat();

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame_with_types("invalid_array_length_stmt", "SELECT $1", &[1_007]),
            support::bind_frame_with_formats(
                "invalid_array_length_portal",
                "invalid_array_length_stmt",
                &[1],
                &[Some(&array)],
                &[],
            ),
            support::execute_frame("invalid_array_length_portal"),
        )
        .await;

        // Assert
        assert!(frames.iter().any(|frame| frame.0 == b'E'));
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
