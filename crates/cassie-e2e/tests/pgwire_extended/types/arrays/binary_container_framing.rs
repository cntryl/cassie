//! PostgreSQL binary array outer-container framing over extended Bind.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn int4_array_binary_cycle(body: &[u8]) -> fixture::Cycle {
    fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 1, Some(body))),
        0,
        false,
    )
}

fn assert_pgwire_error_and_recovery(
    rejected: &fixture::Frames,
    recovery: &fixture::Frames,
    state: &str,
    message: &str,
) {
    assert_eq!(wire::error_code(rejected).as_deref(), Some(state));
    assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    let fields = rejected
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("malformed binary array ErrorResponse");
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some(message)
    );
    assert_eq!(fixture::data_row_payloads(rejected), Vec::<Vec<u8>>::new());
    assert!(!rejected.iter().any(|(tag, _)| matches!(*tag, b'T' | b'C')));
    assert_eq!(
        rejected.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );

    fixture::assert_success(recovery);
    assert_eq!(
        fixture::data_row_payloads(recovery),
        vec![fixture::one_field_row(b"45")]
    );
}
