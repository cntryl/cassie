//! Assert native one-row and empty pagination results in both output formats.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

pub fn assert_rows(batches: &[fixture::Frames], rows: usize) {
    if rows == 1 {
        crate::support_unary_numeric::assert_result(
            batches,
            23,
            [&[0, 1, 0, 0, 0, 1, b'7'], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        );
        return;
    }
    assert_eq!(rows, 0);
    assert_eq!(batches.len(), 10);
    for (format, batch) in batches.as_chunks::<5>().0.iter().enumerate() {
        assert_eq!(batch[0][0].0, b'1');
        fixture::assert_success(&batch[1]);
        assert_eq!(batch[2][0].0, b'2');
        fixture::assert_success(&batch[3]);
        let description = batch[3]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("descriptor");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].type_oid, 23);
        assert_eq!(
            columns[0].format_code,
            i16::try_from(format).expect("format")
        );
        assert_eq!(fixture::data_row_payloads(&batch[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&batch[4]);
    }
}

pub fn assert_execute_failure(batches: &[fixture::Frames], state: &str) {
    assert_eq!(batches.len(), 10);
    for batch in batches.as_chunks::<5>().0 {
        assert_eq!(batch[0][0].0, b'1');
        fixture::assert_success(&batch[1]);
        assert_eq!(batch[2][0].0, b'2');
        assert_eq!(wire::error_code(&batch[3]).as_deref(), Some(state));
        assert_eq!(fixture::data_row_payloads(&batch[3]), Vec::<Vec<u8>>::new());
        assert_eq!(batch[3].last().map(|frame| frame.0), Some(b'Z'));
        fixture::assert_success(&batch[4]);
    }
}
