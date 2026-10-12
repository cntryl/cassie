//! Shared descriptor/row assertions for selected existing scalar input signatures.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub struct Case {
    pub sql: &'static str,
    pub parameter_oid: i32,
    pub column: (i32, i16, i32),
    pub input: &'static [u8],
    pub text: &'static [u8],
    pub binary: &'static [u8],
}

pub fn assert_case(frames: &fixture::Frames, case: &Case, output_format: i16, null: bool) {
    fixture::assert_success(frames);
    let parameters = frames
        .iter()
        .filter(|(tag, _)| *tag == b't')
        .map(|(_, payload)| wire::parse_parameter_description(payload))
        .collect::<Vec<_>>();
    assert_eq!(parameters, vec![vec![case.parameter_oid]], "{}", case.sql);
    fixture::assert_single_column_descriptors(frames, case.column, output_format, true);
    for (_, payload) in frames.iter().filter(|(tag, _)| *tag == b'T') {
        let columns = wire::parse_row_description(payload);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].name, "value", "{}", case.sql);
        assert_eq!((columns[0].table_oid, columns[0].attr_num), (0, 0));
    }
    let expected = if null {
        vec![0, 1, 255, 255, 255, 255]
    } else {
        fixture::one_field_row(if output_format == 0 {
            case.text
        } else {
            case.binary
        })
    };
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![expected],
        "{}",
        case.sql
    );
    let tags = frames.iter().map(|(tag, _)| *tag).collect::<Vec<_>>();
    assert_eq!(tags, b"1tT2TDCZ", "{}", case.sql);
    let command = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .expect("command");
    assert_eq!(command.1.as_slice(), b"SELECT 1\0");
}
