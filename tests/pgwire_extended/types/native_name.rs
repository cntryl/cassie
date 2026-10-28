//! Native NAME and NAME[] codecs preserve their PostgreSQL wire identities.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

const NAME_ARRAY_BINARY: &[u8] = &[
    0, 0, 0, 1, // one dimension
    0, 0, 0, 1, // contains a SQL NULL element
    0, 0, 0, 19, // name element OID
    0, 0, 0, 2, // two elements
    0, 0, 0, 1, // lower bound
    0, 0, 0, 5, b'a', b'l', b'p', b'h', b'a', // first name
    255, 255, 255, 255, // SQL NULL element
];

const NAME_TEXT: &[u8] = b"alpha";
const NAME_ARRAY_TEXT: &[u8] = b"{alpha,NULL}";
const SELECT_PARAMETERS: &str = "SELECT $1 AS scalar_name, $2 AS name_array";

fn parameter_cycle(input_format: i16, result_format: i16) -> fixture::Cycle {
    vec![
        wire::parse_frame_with_types("", SELECT_PARAMETERS, &[19, 1003]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats(
            "name-portal",
            "",
            &[input_format, input_format],
            &[
                Some(NAME_TEXT),
                Some(if input_format == 0 {
                    NAME_ARRAY_TEXT
                } else {
                    NAME_ARRAY_BINARY
                }),
            ],
            &[result_format],
        ),
        wire::describe_portal_frame("name-portal"),
        wire::execute_frame("name-portal"),
        wire::sync_frame(),
    ]
}

fn assert_native_descriptors(frames: &fixture::Frames, format: i16) {
    fixture::assert_success(frames);
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for (index, columns) in descriptions.iter().enumerate() {
        let expected_format = if index == 0 { 0 } else { format };
        assert_eq!(
            columns
                .iter()
                .map(|column| (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                ))
                .collect::<Vec<_>>(),
            vec![
                (19, 64, -1, expected_format),
                (1003, -1, -1, expected_format)
            ]
        );
    }
}

#[test]
fn should_bind_native_name_carriers_in_all_wire_formats() {
    // Arrange
    let cycles = [
        parameter_cycle(0, 0),
        parameter_cycle(0, 1),
        parameter_cycle(1, 0),
        parameter_cycle(1, 1),
    ];

    // Act
    let batches = fixture::run_cycles("native-name-parameter-formats", &[], cycles.to_vec());

    // Assert
    for (format, frames) in [0, 1, 0, 1].into_iter().zip(&batches) {
        assert_native_descriptors(frames, format);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("alpha".to_owned()),
            Some("{alpha,NULL}".to_owned())
        ]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![data_row(&[Some(NAME_TEXT), Some(NAME_ARRAY_BINARY)])]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("alpha".to_owned()),
            Some("{alpha,NULL}".to_owned())
        ]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![data_row(&[Some(NAME_TEXT), Some(NAME_ARRAY_BINARY)])]
    );
}

#[test]
fn should_preserve_native_name_cast_assignment_descriptors() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE native_name_assignment (scalar_name NAME, name_array NAME[])",
            Vec::new(),
        ),
        (
            "INSERT INTO native_name_assignment VALUES \
             ('alpha'::pg_catalog.name, ARRAY['alpha'::name, NULL::name])",
            Vec::new(),
        ),
    ];
    let cycles = [
        fixture::execute_cycle(
            "SELECT 'alpha'::name AS scalar_name, ARRAY['alpha'::pg_catalog.name, NULL::name] AS name_array",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT scalar_name, name_array FROM native_name_assignment",
            None,
            1,
            true,
        ),
        fixture::execute_cycle(
            "SELECT ARRAY['alpha'::name, NULL::name]::text AS name_array_text",
            None,
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles("native-name-casts-assignment", &setup, cycles.to_vec());

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        assert_native_descriptors(frames, format);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("alpha".to_owned()),
            Some("{alpha,NULL}".to_owned())
        ]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![data_row(&[Some(NAME_TEXT), Some(NAME_ARRAY_BINARY)])]
    );
    let cast_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .expect("text cast description");
    assert_eq!(cast_description[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("{alpha,NULL}".to_owned())]]
    );
}

fn data_row(fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut bytes = u16::try_from(fields.len())
        .expect("name result field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        match field {
            Some(value) => {
                bytes.extend_from_slice(
                    &i32::try_from(value.len())
                        .expect("name result field length")
                        .to_be_bytes(),
                );
                bytes.extend_from_slice(value);
            }
            None => bytes.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    bytes
}
