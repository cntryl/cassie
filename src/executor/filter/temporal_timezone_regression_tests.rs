use super::dispatch;
use crate::types::extended::temporal::{
    calendar::{fields_to_timestamp, Fields},
    zone::lookup_zone,
    TemporalSession,
};
use crate::types::typed::TypedValue;
use crate::types::Value;

#[test]
fn should_preserve_first_repeated_posix_designation_for_timestamp_timezone() {
    // Arrange
    let fields = |month| Fields {
        year: 2024,
        month,
        day: 1,
        hour: 12,
        ..Fields::default()
    };
    let winter = fields_to_timestamp(&fields(1), None).expect("winter");
    let summer = fields_to_timestamp(&fields(7), None).expect("summer");
    let session = TemporalSession {
        zone: lookup_zone("AAA0AAA-1,M3.2.0,M11.1.0").expect("POSIX zone"),
        transaction_start: winter,
        ..TemporalSession::default()
    };

    // Act
    let result = dispatch(
        "timezone",
        &[
            Value::String("AAA".to_owned()),
            Value::Typed(TypedValue::Timestamp(summer)),
        ],
        &session,
        None,
    );

    // Assert
    // PostgreSQL 18.6 selects the first AAA designation, UTC, in both seasons.
    assert!(matches!(result, Ok(Value::Typed(TypedValue::TimestampTz(value))) if value == summer));
}

#[test]
fn should_preserve_first_repeated_posix_designation_for_timestamptz_timezone() {
    // Arrange
    let fields = |month| Fields {
        year: 2024,
        month,
        day: 1,
        hour: 12,
        ..Fields::default()
    };
    let winter = fields_to_timestamp(&fields(1), None).expect("winter");
    let summer = fields_to_timestamp(&fields(7), None).expect("summer");
    let session = TemporalSession {
        zone: lookup_zone("AAA0AAA-1,M3.2.0,M11.1.0").expect("POSIX zone"),
        transaction_start: winter,
        ..TemporalSession::default()
    };

    // Act
    let result = dispatch(
        "timezone",
        &[
            Value::String("AAA".to_owned()),
            Value::Typed(TypedValue::TimestampTz(summer)),
        ],
        &session,
        None,
    );

    // Assert
    assert!(matches!(result, Ok(Value::Typed(TypedValue::Timestamp(value))) if value == summer));
}

#[test]
fn should_extract_timestamptz_epoch_without_local_offset_saturation() {
    // Arrange
    let session = TemporalSession {
        zone: crate::types::extended::temporal::zone::Zone::Fixed {
            east: i32::MAX,
            name: String::new(),
        },
        ..TemporalSession::default()
    };
    let maximum = crate::types::extended::temporal::calendar::END_TIMESTAMP - 1;

    // Act
    let result = super::date_part(
        &Value::String("epoch".to_owned()),
        &Value::Typed(TypedValue::TimestampTz(maximum)),
        &session,
    );

    // Assert
    assert!(matches!(result, Ok(Value::Float64(value)) if value == 9_224_318_016_000.0));
}
