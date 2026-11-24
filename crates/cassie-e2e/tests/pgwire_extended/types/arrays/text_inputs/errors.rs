#[test]
fn should_reject_scalar_array_text_elements_outside_the_declared_domain() {
    // Arrange
    let cases: [(i32, &[u8]); 23] = [
        (1005, b"{32768}"),
        (1005, b"[32768]"),
        (1007, b"{-2147483649}"),
        (1007, b"[-2147483649]"),
        (1016, b"{9223372036854775808}"),
        (1016, b"[9223372036854775808]"),
        (1016, b"[1.5]"),
        (1005, b"[true]"),
        (1000, b"{not-a-boolean}"),
        (1000, b"[1]"),
        (1022, b"{NaN}"),
        (1022, b"[true]"),
        (1009, b"[1]"),
        (2951, b"{not-a-uuid}"),
        (2951, b"[\"not-a-uuid\"]"),
        (1001, b"{bad-bytes}"),
        (1001, b"[\"\\\\xABC\"]"),
        (1182, b"{2026-02-30}"),
        (1182, b"[\"2026-02-30\"]"),
        (1115, b"{not-a-timestamp}"),
        (1115, b"[\"not-a-timestamp\"]"),
        (1014, b"[true]"),
        (1015, b"[{}]"),
    ];
    let mut cycles = Vec::new();
    for (oid, payload) in cases {
        for result_format in [0, 1] {
            // Describe before Bind can legitimately emit a type-only
            // descriptor. Omit it for the decoder-before-metadata oracle.
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((oid, 0, Some(payload))),
                result_format,
                false,
            ));
            cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-text-declared-domain", &[], cycles);

    // Assert
    for (pair_index, pair) in batches.as_chunks::<2>().0.iter().enumerate() {
        let (oid, payload) = cases[pair_index / 2];
        assert_eq!(
            wire::error_code(&pair[0]).as_deref(),
            Some("08P01"),
            "OID {oid}, payload {payload:?}"
        );
        assert_eq!(
            pair[0]
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0
        );
        assert_eq!(
            pair[0].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}
