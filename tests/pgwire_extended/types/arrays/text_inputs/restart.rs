#[test]
fn should_preserve_unlimited_varchar_values_across_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE finite_unlimited_varchar (k INT, v VARCHAR)",
            Vec::new(),
        ),
        (
            "CREATE TABLE finite_unlimited_varchar_array (k INT, v VARCHAR[])",
            Vec::new(),
        ),
    ];
    let scalar_inputs = [Some(&b"unlimited"[..]), Some(&b""[..]), None];
    let array_inputs = [Some(&b"[\"unlimited\"]"[..]), Some(&b"[]"[..]), None];
    let array_text_outputs = [Some(&b"{unlimited}"[..]), Some(&b"{}"[..]), None];
    let array_nonempty_binary =
        &b"\0\0\0\x01\0\0\0\0\0\0\x04\x13\0\0\0\x01\0\0\0\x01\0\0\0\x09unlimited"[..];
    let array_empty_binary = &b"\0\0\0\0\0\0\0\0\0\0\x04\x13"[..];
    let mut writes = Vec::new();
    let mut write_expectations = Vec::new();
    for format in [0, 1] {
        for (index, (scalar, array)) in scalar_inputs.into_iter().zip(array_inputs).enumerate() {
            let key = usize::try_from(format).expect("result format") * 3 + index + 1;
            writes.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO finite_unlimited_varchar (k, v) VALUES ({key}, $1) RETURNING v"
                ),
                Some((1043, 0, scalar)),
                format,
                true,
            ));
            write_expectations.push((1043, format, scalar));
            writes.push(fixture::execute_cycle(
                &format!("INSERT INTO finite_unlimited_varchar_array (k, v) VALUES ({key}, $1) RETURNING v"),
                Some((1015, 0, array)),
                format,
                true,
            ));
            let expected = if format == 1 {
                match index {
                    0 => Some(array_nonempty_binary),
                    1 => Some(array_empty_binary),
                    _ => None,
                }
            } else {
                array_text_outputs[index]
            };
            write_expectations.push((1015, format, expected));
        }
    }
    let reads = unlimited_varchar_reads();
    let write_count = writes.len();
    writes.extend(reads.clone());

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-unlimited-varchar",
        &setup,
        vec![writes, reads],
    );

    // Assert
    for ((oid, format, expected), frames) in write_expectations.into_iter().zip(&stages[0]) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![expected.map_or_else(|| vec![0, 1, 255, 255, 255, 255], fixture::one_field_row,)]
        );
    }
    for frames in [&stages[0][write_count..], stages[1].as_slice()] {
        for (index, frame) in frames.iter().enumerate() {
            fixture::assert_success(frame);
            let format = i16::try_from(index / 2).expect("result format");
            let scalar = index % 2 == 0;
            fixture::assert_single_column_descriptors(
                frame,
                (if scalar { 1043 } else { 1015 }, -1, -1),
                format,
                true,
            );
            let values: [Option<&[u8]>; 3] = if scalar {
                scalar_inputs
            } else if format == 1 {
                [Some(array_nonempty_binary), Some(array_empty_binary), None]
            } else {
                array_text_outputs
            };
            let expected = values.map(|value| {
                value.map_or_else(|| vec![0, 1, 255, 255, 255, 255], fixture::one_field_row)
            });
            assert_eq!(fixture::data_row_payloads(frame), expected.to_vec());
        }
    }
}
