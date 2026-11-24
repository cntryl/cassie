use super::{checked_lengths, RawFrame};
use crate::config::CassieRuntimeLimits;
use crate::runtime::accounted::json_arena::PlainIdentity;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_legacy_frames_for_ordinary_values_in_each_json_type() {
    // Arrange
    let controls = controls();
    let source = r#"{"a":1}"#;
    // Act
    let mut outcomes = Vec::new();
    for (identity, oid, kind) in [
        (PlainIdentity::Json, 114, crate::types::DataType::Json),
        (PlainIdentity::Jsonb, 3802, crate::types::DataType::Jsonb),
    ] {
        for format in [0, 1] {
            let cell = crate::midge::row_blob::tests::owned_row_cell(source, identity, &controls)
                .expect("cell");
            let mut expected = Vec::new();
            let columns = [crate::executor::ColumnMeta::from_data_type("doc", &kind)];
            crate::pgwire::connection::writers::append_data_row_frame_in(
                &mut expected,
                &[crate::types::Value::Json(
                    serde_json::from_str(source).expect("legacy value"),
                )],
                &columns,
                &[format],
                &[],
                crate::types::extended::temporal::default_session(),
            )
            .expect("legacy frame");
            let actual = RawFrame::build(Some(&cell), oid, format, &controls).expect("raw frame");
            let actual_bytes = actual.bytes().to_vec();
            drop(actual);
            drop(cell);
            outcomes.push((
                actual_bytes,
                expected,
                controls.current_query_memory_bytes(),
            ));
        }
    }

    // Assert
    for (actual, expected, retained) in outcomes {
        assert_eq!(actual, expected);
        assert_eq!(retained, 0);
    }
}

#[test]
fn should_emit_postgresql_json_frame_payloads() {
    // Arrange
    let controls = controls();
    let raw = r#" {"z":1e+2, "a": 2, "a":3} "#;
    let sentinel = r#"{"$serde_json::private::Number":"7"}"#;
    // Act
    let mut outcomes = Vec::new();
    for source in [raw, sentinel] {
        for (identity, oid) in [(PlainIdentity::Json, 114), (PlainIdentity::Jsonb, 3802)] {
            let cell = crate::midge::row_blob::tests::owned_row_cell(source, identity, &controls)
                .expect("cell");
            for format in [0, 1] {
                let frame = RawFrame::build(Some(&cell), oid, format, &controls).expect("frame");
                let expected = if identity == PlainIdentity::Json {
                    source
                } else if source == raw {
                    r#"{"a": 3, "z": 100}"#
                } else {
                    r#"{"$serde_json::private::Number": "7"}"#
                };
                let binary_jsonb = identity == PlainIdentity::Jsonb && format == 1;
                outcomes.push((frame.bytes()[11..].to_vec(), expected, binary_jsonb));
                drop(frame);
            }
            drop(cell);
        }
    }

    // Assert
    for (payload, expected, binary_jsonb) in outcomes {
        if binary_jsonb {
            assert_eq!(payload[0], 1);
            assert_eq!(&payload[1..], expected.as_bytes());
        } else {
            assert_eq!(payload, expected.as_bytes());
        }
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_record_json_value_writer_spelling_difference_from_postgresql() {
    // Arrange
    let controls = controls();
    let source = r#" {"z":1e+2, "a": 2, "a":3} "#;
    let cell =
        crate::midge::row_blob::tests::owned_row_cell(source, PlainIdentity::Json, &controls)
            .expect("cell");
    let parsed: serde_json::Value = serde_json::from_str(source).expect("legacy parse");
    let mut legacy = Vec::new();
    let columns = [crate::executor::ColumnMeta::from_data_type(
        "doc",
        &crate::types::DataType::Json,
    )];
    // Act
    crate::pgwire::connection::writers::append_data_row_frame_in(
        &mut legacy,
        &[crate::types::Value::Json(parsed)],
        &columns,
        &[0],
        &[],
        crate::types::extended::temporal::default_session(),
    )
    .expect("legacy frame");
    let frame = RawFrame::build(Some(&cell), 114, 0, &controls).expect("frame");
    // Assert
    assert_eq!(&frame.bytes()[11..], source.as_bytes());
    assert_ne!(&legacy[11..], source.as_bytes());
    drop(frame);
    drop(cell);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_mismatched_raw_frame_arguments_while_distinguishing_nulls() {
    // Arrange
    let controls = controls();
    let cell =
        crate::midge::row_blob::tests::owned_row_cell("null", PlainIdentity::Jsonb, &controls)
            .expect("cell");
    // Act
    let sql_null = RawFrame::build(None, 3802, 1, &controls).expect("SQL null");
    let json_null = RawFrame::build(Some(&cell), 3802, 1, &controls).expect("JSON null");
    // Assert
    assert_eq!(&sql_null.bytes()[7..11], &(-1_i32).to_be_bytes());
    assert_eq!(&json_null.bytes()[11..], b"\x01null");
    assert!(RawFrame::build(Some(&cell), 114, 1, &controls).is_err());
    assert!(RawFrame::build(Some(&cell), 3802, 2, &controls).is_err());
    assert!(checked_lengths(usize::MAX, false).is_err());
    drop(sql_null);
    drop(json_null);
    drop(cell);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_account_raw_frame_to_owning_tracker_across_source_drop() {
    // Arrange
    let owner = controls();
    let other = controls();
    let cell =
        crate::midge::row_blob::tests::owned_row_cell(r#"{"a":1}"#, PlainIdentity::Json, &owner)
            .expect("cell");
    // Act
    let foreign = RawFrame::build(Some(&cell), 114, 0, &other);
    let frame = RawFrame::build(Some(&cell), 114, 0, &owner).expect("frame");
    drop(cell);
    // Assert
    assert!(foreign.is_err());
    assert_eq!(&frame.bytes()[11..], br#"{"a":1}"#);
    assert!(owner.current_query_memory_bytes() > 0);
    assert_eq!(other.current_query_memory_bytes(), 0);
    drop(frame);
    assert_eq!(owner.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_frame_after_actual_cell_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let cell = crate::midge::row_blob::tests::owned_row_cell(
        r#"{"a":"payload"}"#,
        PlainIdentity::Json,
        &controls,
    )
    .expect("cell");
    let retained = controls.current_query_memory_bytes();
    let mut copies = 0;
    // Act
    let frame = RawFrame::build_observing(Some(&cell), 114, 0, &controls, || {
        copies += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(frame.is_err());
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(cell);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_cell_raw_frame_overlap() {
    // Arrange
    let baseline = controls();
    let cell = crate::midge::row_blob::tests::owned_row_cell(
        r#"{"a":"payload"}"#,
        PlainIdentity::Jsonb,
        &baseline,
    )
    .expect("cell");
    let filler = baseline.reserve_query_memory(65_536).expect("filler");
    let frame = RawFrame::build(Some(&cell), 3802, 1, &baseline).expect("frame");
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(peak, baseline.current_query_memory_bytes());
    drop(frame);
    drop(filler);
    drop(cell);
    // Act
    let outcomes = [(peak, true), (peak - 1, false)].map(|(budget, succeeds)| {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let cell = crate::midge::row_blob::tests::owned_row_cell(
            r#"{"a":"payload"}"#,
            PlainIdentity::Jsonb,
            &controls,
        )
        .expect("cell");
        let filler = controls.reserve_query_memory(65_536).expect("filler");
        let frame = RawFrame::build(Some(&cell), 3802, 1, &controls);
        let admitted = frame.is_ok();
        drop(frame);
        drop(filler);
        drop(cell);
        (succeeds, admitted, controls.current_query_memory_bytes())
    });

    // Assert
    for (succeeds, admitted, retained) in outcomes {
        assert_eq!(admitted, succeeds);
        assert_eq!(retained, 0);
    }
}
