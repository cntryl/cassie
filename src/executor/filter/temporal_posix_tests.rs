use super::{dispatch, posix_timezone};
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::temporal::{
    calendar::{fields_to_timestamp, Fields},
    zone::lookup_zone,
    TemporalSession,
};
use crate::types::typed::TypedValue;
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_resolve_equal_posix_designations_to_the_standard_offset() {
    // Arrange
    let values = [
        ("AAA0AAA-1,M3.2.0,M11.1.0", "AAA", 0),
        ("EEE5EEE4,M3.2.0,M11.1.0", "EEE", -18_000),
        ("EEE-5EEE-6,M3.2.0,M11.1.0", "EEE", 18_000),
        ("EEE5:30EEE4:30,M3.2.0,M11.1.0", "EEE", -19_800),
        ("EEE+05:30:00EEE+04:30:00,M3.2.0,M11.1.0", "EEE", -19_800),
        ("EEE+23:59EEE+22:59,M3.2.0,M11.1.0", "EEE", -86_340),
        ("EEE-23:59EEE-22:59,M3.2.0,M11.1.0", "EEE", 86_340),
        ("ABC0ABC-1,M3.2.0,M11.1.0", "ABC", 0),
        (
            "ABCDEFGHIJKLMNOP0ABCDEFGHIJKLMNOP-1,M3.2.0,M11.1.0",
            "ABCDEFGHIJKLMNOP",
            0,
        ),
        ("AAA-0AAA-1,M3.2.0,M11.1.0", "AAA", 0),
        ("AAA+0AAA-1,M3.2.0,M11.1.0", "AAA", 0),
        ("AAA0AAA,M3.2.0,M11.1.0", "AAA", 0),
        ("CST0CST-1,M3.2.0,M11.1.0", "cst", 0),
        ("AAA+23:59AAA-23:59,M3.2.0,M11.1.0", "AAA", -86_340),
        ("AAA+23:59AAA,M3.2.0,M11.1.0", "AAA", -86_340),
        ("AAA0AAA-1,M3.2.0,M5.2.0", "AAA", 0),
        ("AAA0AAA-1,M1.5.0,M11.1.0", "AAA", 0),
        ("AAA0AAA-1,M11.1.0,M3.2.0", "AAA", 0),
    ];

    for (rule, name, east) in values {
        // Act
        let result = posix_timezone::fixed_duplicate_offset(rule, name);

        // Assert
        assert_eq!(result, Some(east));
    }
}

#[test]
fn should_leave_nonmatching_posix_forms_to_the_legacy_resolver() {
    // Arrange
    let rules = [
        ("AAA0BBB-1,M3.2.0,M11.1.0", "AAA"),
        ("AAA0aaa-1,M3.2.0,M11.1.0", "AAA"),
        ("<AAA>0<AAA>-1,M3.2.0,M11.1.0", "AAA"),
        ("AAA0AAA-1,M3.2.0,M11.1.0", "BBB"),
        ("EEE5:30:45EEE4:30:45,M3.2.0,M11.1.0", "EEE"),
        ("EEE24EEE23,M3.2.0,M11.1.0", "EEE"),
        ("EEE5:60EEE4,M3.2.0,M11.1.0", "EEE"),
        ("EEE5:3EEE4:3,M3.2.0,M11.1.0", "EEE"),
        ("AAA0AAA", "AAA"),
        ("AAA0AAA-1,M3.2.0/2,M3.2.0/3", "AAA"),
        ("CST0CST-1,M3.2.0/2,M3.2.0/3", "CST"),
        ("AAA0AAA-1,M3.2.0/2,M11.1.0", "AAA"),
        ("AAA0AAA-1,M3.2.0,M3.3.0", "AAA"),
        ("AAA0AAA-1,M3.2.0,M4.2.0", "AAA"),
        ("AAA0AAA-1,M1.1.0,M12.1.0", "AAA"),
        ("AAA0AAA-25,M3.2.0,M11.1.0", "AAA"),
        ("AAA-23:59AAA,M3.2.0,M11.1.0", "AAA"),
        ("AAA0AAA-24:59:59,M3.2.0,M11.1.0", "AAA"),
        ("AAA0AAA-1:00:45,M3.2.0,M11.1.0", "AAA"),
        ("AAA0AAA-1:00:60,M3.2.0,M11.1.0", "AAA"),
        ("AAA0AAA-1,M13.2.0,M5.2.0", "AAA"),
        ("AAA0AAA-1,M3.0.0,M5.2.0", "AAA"),
        ("AAA0AAA-1,M3.2.7,M5.2.0", "AAA"),
        ("AAA0AAA-1,M3.2.0,M5.2.0x", "AAA"),
        (
            "ABCDEFGHIJKLMNOPQ0ABCDEFGHIJKLMNOPQ-1,M3.2.0,M11.1.0",
            "ABCDEFGHIJKLMNOPQ",
        ),
    ];

    for (rule, name) in rules {
        // Act
        let result = posix_timezone::fixed_duplicate_offset(rule, name);

        // Assert
        assert_eq!(result, None);
    }
}

#[test]
fn should_use_the_provenance_owned_by_the_exact_statement_session() {
    // Arrange
    let summer = fields_to_timestamp(
        &Fields {
            year: 2024,
            month: 7,
            day: 1,
            hour: 12,
            ..Fields::default()
        },
        None,
    )
    .expect("summer");
    let session = Arc::new(TemporalSession {
        zone: lookup_zone("AAA0AAA-1,M3.2.0,M11.1.0").expect("zone"),
        transaction_start: summer,
        ..TemporalSession::default()
    });
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    controls.install_temporal_session(Arc::clone(&session), true);
    let input = [
        Value::String("AAA".to_owned()),
        Value::Typed(TypedValue::Timestamp(summer)),
    ];

    // Act
    let result = dispatch("timezone", &input, &session, Some(&controls));
    let other = TemporalSession {
        zone: session.zone.clone(),
        transaction_start: summer,
        ..TemporalSession::default()
    };
    let fallback = dispatch("timezone", &input, &other, Some(&controls));

    // Assert
    assert!(matches!(result, Ok(Value::Typed(TypedValue::TimestampTz(value))) if value == summer));
    assert!(
        matches!(fallback, Ok(Value::Typed(TypedValue::TimestampTz(value))) if value == summer - 3_600_000_000)
    );
}

#[test]
fn should_invalidate_posix_provenance_on_session_replacement() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let first = Arc::new(TemporalSession {
        zone: lookup_zone("AAA0AAA-1,M3.2.0,M11.1.0").expect("zone"),
        ..TemporalSession::default()
    });
    let replacement = Arc::new(TemporalSession::default());
    controls.install_temporal_session(Arc::clone(&first), true);

    // Act
    controls.install_temporal_session(Arc::clone(&replacement), false);

    // Assert
    assert!(!controls
        .temporal_session_is_posix(&first)
        .expect("old session"));
    assert!(!controls
        .temporal_session_is_posix(&replacement)
        .expect("replacement"));
}

#[test]
fn should_resolve_timestamptz_with_the_standard_posix_designation() {
    // Arrange
    let summer = fields_to_timestamp(
        &Fields {
            year: 2024,
            month: 7,
            day: 1,
            hour: 12,
            ..Fields::default()
        },
        None,
    )
    .expect("summer");
    let session = Arc::new(TemporalSession {
        zone: lookup_zone("EEE5EEE4,M3.2.0,M11.1.0").expect("zone"),
        transaction_start: summer,
        ..TemporalSession::default()
    });
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    controls.install_temporal_session(Arc::clone(&session), true);

    // Act
    let result = dispatch(
        "timezone",
        &[
            Value::String("EEE".to_owned()),
            Value::Typed(TypedValue::TimestampTz(summer)),
        ],
        &session,
        Some(&controls),
    );

    // Assert
    assert!(
        matches!(result, Ok(Value::Typed(TypedValue::Timestamp(value))) if value == summer - 18_000_000_000)
    );
}

#[test]
fn should_not_infer_posix_origin_from_a_custom_zone_name() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = Arc::new(TemporalSession {
        zone: crate::types::extended::temporal::zone::Zone::Rules {
            tz: jiff::tz::TimeZone::get("America/New_York").expect("zone"),
            name: "AAA0AAA-1,M3.2.0,M11.1.0".to_owned(),
        },
        ..TemporalSession::default()
    });
    controls.install_temporal_session(Arc::clone(&session), false);

    // Act
    let result = posix_timezone::resolve(
        &Value::String("AAA".to_owned()),
        &Value::Typed(TypedValue::Timestamp(0)),
        &session,
        Some(&controls),
    );

    // Assert
    assert!(matches!(result, Ok(None)));
}

#[test]
fn should_propagate_cancellation_before_recognizing_a_posix_rule() {
    // Arrange
    let cancel = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let session = Arc::new(TemporalSession {
        zone: lookup_zone("AAA0AAA-1,M3.2.0,M11.1.0").expect("zone"),
        ..TemporalSession::default()
    });
    controls.install_temporal_session(Arc::clone(&session), true);
    cancel.cancel();

    // Act
    let result = posix_timezone::resolve(
        &Value::String("AAA".to_owned()),
        &Value::Typed(TypedValue::Timestamp(0)),
        &session,
        Some(&controls),
    );

    // Assert
    assert!(matches!(
        result,
        Err(crate::executor::QueryError::Cassie(
            crate::app::CassieError::QueryCanceled
        ))
    ));
}

#[test]
fn should_preserve_excluded_rule_fallback_with_precanceled_controls() {
    // Arrange
    let cancel = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let session = Arc::new(TemporalSession {
        zone: lookup_zone("America/New_York").expect("zone"),
        ..TemporalSession::default()
    });
    controls.install_temporal_session(Arc::clone(&session), false);
    cancel.cancel();

    // Act
    let result = posix_timezone::resolve(
        &Value::String("UTC".to_owned()),
        &Value::Typed(TypedValue::Timestamp(0)),
        &session,
        Some(&controls),
    );

    // Assert
    assert!(matches!(result, Ok(None)));
}

#[test]
fn should_report_precancellation_for_matching_rule_before_origin_fallback() {
    // Arrange
    let cancel = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let session = Arc::new(TemporalSession {
        zone: crate::types::extended::temporal::zone::Zone::Rules {
            tz: jiff::tz::TimeZone::get("America/New_York").expect("zone"),
            name: "AAA0AAA-1,M3.2.0,M11.1.0".to_owned(),
        },
        ..TemporalSession::default()
    });
    controls.install_temporal_session(Arc::clone(&session), false);
    cancel.cancel();

    // Act
    let result = posix_timezone::resolve(
        &Value::String("AAA".to_owned()),
        &Value::Typed(TypedValue::Timestamp(0)),
        &session,
        Some(&controls),
    );

    // Assert
    assert!(matches!(
        result,
        Err(crate::executor::QueryError::Cassie(
            crate::app::CassieError::QueryCanceled
        ))
    ));
}
