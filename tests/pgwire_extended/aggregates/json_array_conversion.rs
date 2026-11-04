//! Native numeric array parameters preserve document conversion and descriptors.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_json_array_conversion_across_native_parameter_formats() {
    // Arrange
    let value = "9007199254740993.125500";
    let numeric = cassie::types::extended::numeric::parse_text(value)
        .expect("numeric")
        .send_binary();
    let mut binary = Vec::new();
    for item in [1_i32, 1, 1700, 2, -2] {
        binary.extend_from_slice(&item.to_be_bytes());
    }
    binary.extend_from_slice(
        &i32::try_from(numeric.len())
            .expect("small native number")
            .to_be_bytes(),
    );
    binary.extend_from_slice(&numeric);
    binary.extend_from_slice(&(-1_i32).to_be_bytes());
    let text = format!("[-2:-1]={{{value},NULL}}");
    let sql = "SELECT to_json($1::NUMERIC[]),to_jsonb($1::NUMERIC[]),json_agg($1::NUMERIC[]),jsonb_agg($1::NUMERIC[])";
    let mut cycles = Vec::new();
    for parameter_format in [0, 1] {
        for result_format in [0, 1] {
            let statement = format!("json_array_{parameter_format}_{result_format}");
            let input = if parameter_format == 0 {
                text.as_bytes()
            } else {
                binary.as_slice()
            };
            cycles.push(vec![
                wire::parse_frame(&statement, sql),
                wire::describe_statement_frame(&statement),
                wire::bind_frame_with_formats(
                    "",
                    &statement,
                    &[parameter_format],
                    &[Some(input)],
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]);
        }
    }

    // Act
    let batches = fixture::run_cycles("issue-811-json-array-native-formats", &[], cycles);

    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let parameter = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter descriptor");
        assert_eq!(wire::parse_parameter_description(&parameter.1), [1231]);
        let description = frames
            .iter()
            .rev()
            .find(|(tag, _)| *tag == b'T')
            .expect("result descriptor");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(
            columns
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [114, 3802, 114, 3802]
        );
        let result_format = i16::from(index % 2 != 0);
        assert!(columns
            .iter()
            .all(|column| column.format_code == result_format));
        let version = if result_format == 1 { "\u{1}" } else { "" };
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![
                Some(format!("[{value},null]")),
                Some(format!("{version}[{value}, null]")),
                Some(format!("[[{value},null]]")),
                Some(format!("{version}[[{value}, null]]"))
            ]]
        );
    }
}
