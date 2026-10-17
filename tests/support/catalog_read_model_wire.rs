//! Empty catalog exports retain column-name shape across statement/portal formats.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_empty(batches: &[fixture::Frames], names: &[&str]) {
    assert_eq!(batches.len(), 10);
    for (format, cycle) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_eq!(cycle[2][0].0, b'2', "{:?}", cycle[2]);
        fixture::assert_success(&cycle[3]);
        for (phase, format) in [(1, 0), (3, i16::try_from(format).expect("format"))] {
            let descriptor = cycle[phase]
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("row description");
            let columns = wire::parse_row_description(&descriptor.1);
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                names
            );
            assert!(columns.iter().all(|column| column.format_code == format));
        }
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}
