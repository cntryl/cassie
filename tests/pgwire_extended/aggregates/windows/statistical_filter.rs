//! Selected statistical window parameter and result OIDs in both wire formats.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_statistical_window_result_identities_across_wire_formats() {
    // Arrange
    let calls = [
        ("var_pop(1::INT)", 1700),
        ("var_samp(1::NUMERIC)", 1700),
        ("variance(1::BIGINT)", 1700),
        ("stddev_pop(1::REAL)", 701),
        ("stddev_samp(1::FLOAT8)", 701),
        ("stddev(1::FLOAT8)", 701),
        ("corr(2,1)", 701),
        ("covar_pop(2,1)", 701),
        ("covar_samp(2,1)", 701),
        ("regr_count(2,1)", 20),
        ("regr_slope(2,1)", 701),
        ("regr_intercept(2,1)", 701),
        ("regr_r2(2,1)", 701),
        ("regr_sxx(2,1)", 701),
        ("regr_sxy(2,1)", 701),
        ("regr_syy(2,1)", 701),
        ("regr_avgx(2,1)", 701),
        ("regr_avgy(2,1)", 701),
    ];
    let cycles = calls
        .iter()
        .flat_map(|(call, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    &format!("SELECT pg_catalog.{call} FILTER(WHERE TRUE) OVER ()"),
                    None,
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("statistical-window-codecs", &[], cycles);

    // Assert
    assert_eq!(batches.len(), calls.len() * 2);
    for ((call, oid), frames) in calls.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, batch) in [0, 1].into_iter().zip(frames) {
            fixture::assert_success(batch);
            let description = batch.iter().find(|(tag, _)| *tag == b'T').expect("row description");
            assert_eq!(wire::parse_row_description(&description.1)[0].name, call.split('(').next().expect("function name"));
            fixture::assert_single_column_descriptors(
                batch,
                (oid, if oid == 1700 { -1 } else { 8 }, -1),
                format,
                true,
            );
            assert_eq!(fixture::data_row_payloads(batch).len(), 1, "{call}");
        }
    }
}

#[test]
fn should_infer_window_filter_parameters_as_boolean_before_argument_evaluation() {
    // Arrange
    let cases = [
        (
            "SELECT var_pop(1) FILTER(WHERE $1) OVER ()",
            b"false".as_slice(),
            16,
        ),
        (
            "SELECT corr($1,1::FLOAT8) FILTER(WHERE TRUE) OVER ()",
            b"2".as_slice(),
            701,
        ),
        (
            "SELECT regr_slope(1::FLOAT8,$1) OVER ()",
            b"2".as_slice(),
            701,
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, value, _)| fixture::execute_cycle(sql, Some((0, 0, Some(value))), 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("statistical-window-parameters", &[], cycles);

    // Assert
    for ((sql, _, oid), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![oid],
            "{sql}"
        );
        assert_eq!(wire::data_rows(frames), vec![vec![None]], "{sql}");
    }
}

#[test]
fn should_preserve_selected_statistical_window_error_sqlstates() {
    // Arrange
    let cases = [
        (
            "SELECT var_pop(n) FILTER(WHERE n) OVER() FROM (VALUES(1))v(n)",
            "42804",
        ),
        (
            "SELECT corr(DISTINCT y,x) OVER() FROM (VALUES(2::FLOAT8,1::FLOAT8))v(y,x)",
            "0A000",
        ),
        (
            "SELECT var_pop(n ORDER BY n) OVER() FROM (VALUES(1))v(n)",
            "0A000",
        ),
        ("SELECT corr(TRUE,1) OVER()", "42883"),
        ("SELECT corr(1) OVER()", "42883"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("statistical-window-errors", &[], cycles);

    // Assert
    for ((sql, expected), frames) in cases.into_iter().zip(&batches) {
        assert_eq!(wire::error_code(frames).as_deref(), Some(expected), "{sql}");
        assert!(!frames.iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(b"I".as_slice())
        );
    }
}
