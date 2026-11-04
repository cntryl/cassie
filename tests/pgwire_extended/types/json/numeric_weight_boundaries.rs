use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_recover_from_true_jsonb_numeric_range_errors_after_accepting_the_boundary() {
    // Arrange
    let cycles = [
        "SELECT length(('0.01e131073'::JSONB)::TEXT)",
        "SELECT '0.01e131074'::JSONB",
        "SELECT '0e1073741824'::JSONB",
        "SELECT '1e131072x'::JSONB",
        "SELECT 44::INT AS recovered",
    ]
    .into_iter()
    .map(|sql| vec![wire::simple_query_frame(sql)])
    .collect();
    // Act
    let batches = fixture::run_cycles("jsonb-numeric-format-boundaries", &[], cycles);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("131072".into())]]
    );
    for (index, code) in [(1, "22003"), (2, "22003"), (3, "22P02")] {
        assert_eq!(wire::error_code(&batches[index]).as_deref(), Some(code));
        assert!(batches[index]
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'D' | b'C')));
        assert_eq!(
            batches[index].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    fixture::assert_success(&batches[4]);
    assert_eq!(wire::data_rows(&batches[4]), vec![vec![Some("44".into())]]);
}
