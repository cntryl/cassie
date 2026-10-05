//! Selected OID705 descriptor authority; NULL value framing is independent.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_match_unknown_carrier_lengths_across_metadata_surfaces() {
    // Arrange
    let setup = [("CREATE TABLE metadata_empty (k INT)", Vec::new())];
    let cases = [
        ("SELECT NULL AS v", true),
        ("SELECT NULL AS v FROM metadata_empty", false),
    ];
    let mut cycles = vec![fixture::execute_cycle(
        "SELECT typlen FROM pg_catalog.pg_type WHERE oid = 705",
        None,
        0,
        false,
    )];
    for format in [0, 1] {
        cycles.extend(cases.map(|(sql, _)| fixture::execute_cycle(sql, None, format, true)));
    }

    // Act
    let batches = fixture::run_cycles("type-unknown-length", &setup, cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("-2".to_string())]]
    );
    for (format, pair) in [0, 1].into_iter().zip(batches[1..].as_chunks::<2>().0) {
        for ((_, has_row), frames) in cases.into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (705, -2, -1), format, true);
            let expected = if has_row {
                vec![vec![0, 1, 255, 255, 255, 255]]
            } else {
                Vec::new()
            };
            assert_eq!(fixture::data_row_payloads(frames), expected);
        }
    }
}

#[test]
fn should_preserve_declared_null_result_descriptors() {
    // Arrange
    let setup = [("CREATE TABLE metadata_empty (k INT)", Vec::new())];
    let cases = [
        ("SELECT CAST(NULL AS INT) AS v", (23, 4, -1), true),
        ("SELECT CAST(NULL AS BOOLEAN) AS v", (16, 1, -1), true),
        ("SELECT CAST(NULL AS JSON) AS v", (114, -1, -1), true),
        (
            "SELECT CAST(NULL AS INT) AS v FROM metadata_empty",
            (23, 4, -1),
            false,
        ),
        (
            "SELECT CAST(NULL AS BOOLEAN) AS v FROM metadata_empty",
            (16, 1, -1),
            false,
        ),
        (
            "SELECT CAST(NULL AS JSON) AS v FROM metadata_empty",
            (114, -1, -1),
            false,
        ),
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| cases.map(|(sql, _, _)| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-declared-null", &setup, cycles);

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.as_chunks::<6>().0) {
        for ((_, descriptor, has_row), frames) in cases.into_iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, descriptor, format, true);
            let expected = if has_row {
                vec![vec![0, 1, 255, 255, 255, 255]]
            } else {
                Vec::new()
            };
            assert_eq!(fixture::data_row_payloads(frames), expected);
        }
    }
}
