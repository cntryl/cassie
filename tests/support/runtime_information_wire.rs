//! Complete primitive descriptors for the empty loaded-module table function.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_modules(batches: &[fixture::Frames]) {
    assert_eq!(batches.len(), 10);
    for (format, cycle) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(wire::error_code(&cycle[0]), None);
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_eq!(cycle[2][0].0, b'2');
        fixture::assert_success(&cycle[3]);
        for (phase, format) in [(1, 0), (3, i16::try_from(format).expect("format"))] {
            let descriptor = cycle[phase]
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("descriptor");
            let columns = wire::parse_row_description(&descriptor.1);
            assert_eq!(columns.len(), 3);
            for (column, name) in columns.iter().zip(["module_name", "version", "file_name"]) {
                assert_eq!(
                    (
                        column.name.as_str(),
                        column.type_oid,
                        column.type_size,
                        column.type_mod,
                        column.format_code
                    ),
                    (name, 25, -1, -1, format)
                );
            }
        }
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}
