#[test]
fn should_preserve_unsigned_json_scalar_documents_across_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE unsigned_json_documents (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "CREATE TABLE unsigned_json_bigints (k INT, wide BIGINT)",
            Vec::new(),
        ),
    ];
    let mut first = Vec::new();
    for input_format in [0, 1] {
        for result_format in [0, 1] {
            let key = input_format * 2 + result_format + 1;
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_documents (k, doc) VALUES ({key}, $1) RETURNING doc"
                ),
                Some((114, input_format, Some(MAX_DOCUMENT))),
                result_format,
                true,
            ));
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_bigints (k, wide) VALUES ({key}, $1) RETURNING wide"
                ),
                Some((
                    20,
                    input_format,
                    Some(if input_format == 0 {
                        WIDE_INTEGER_TEXT
                    } else {
                        WIDE_INTEGER_BINARY
                    }),
                )),
                result_format,
                true,
            ));
        }
    }
    let reads = [0, 1]
        .into_iter()
        .flat_map(|format| {
            [
                fixture::execute_cycle(
                    "SELECT doc FROM unsigned_json_documents WHERE k = 1",
                    None,
                    format,
                    true,
                ),
                fixture::execute_cycle(
                    "SELECT wide FROM unsigned_json_bigints WHERE k = 1",
                    None,
                    format,
                    true,
                ),
            ]
        })
        .collect::<Vec<_>>();
    first.extend(reads.clone());

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-json-scalar-u64-restart",
        &setup,
        vec![first, reads],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].len(), 12);
    assert_eq!(stages[1].len(), 4);
    for (index, frames) in stages[0].iter().enumerate() {
        let format = if index < 8 {
            (index / 2) % 2
        } else {
            (index - 8) / 2
        };
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(format).expect("format"),
        );
    }
    for (index, frames) in stages[1].iter().enumerate() {
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(index / 2).expect("format"),
        );
    }
}
