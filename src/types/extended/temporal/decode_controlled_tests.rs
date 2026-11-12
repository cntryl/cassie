use super::{decode_datetime, decode_datetime_checked, ControlledDecodeError};
use crate::types::extended::temporal::{zone::lookup_zone, TemporalSession};

#[test]
fn should_preserve_decoder_behavior_with_the_existing_resolver() {
    // Arrange
    let session = TemporalSession {
        zone: lookup_zone("America/New_York").expect("New York"),
        ..TemporalSession::default()
    };
    let inputs = [
        "2024-01-01 12:34:56 UTC",
        "January 1 2024 12:34:56",
        "2024-13-01 12:34:56",
        "2024-01-01 nonsense",
    ];

    // Act and assert
    for input in inputs {
        let expected = decode_datetime(input, &session);
        let actual = decode_datetime_checked(input, &session, &mut |field, local| {
            Ok::<_, ()>(
                session
                    .zone
                    .abbreviation_offset(field, local)
                    .or_else(|| session.abbreviations.lookup(field)),
            )
        });
        assert_eq!(actual, expected.map_err(ControlledDecodeError::Decode));
    }
}

#[test]
fn should_stop_and_return_the_original_resolver_error() {
    // Arrange
    let session = TemporalSession::default();
    let mut calls = 0;

    // Act
    let actual = decode_datetime_checked("January 1 2024", &session, &mut |_, _| {
        calls += 1;
        Err::<Option<(i32, bool)>, _>("cancelled")
    });

    // Assert
    assert_eq!(actual, Err(ControlledDecodeError::Control("cancelled")));
    assert_eq!(calls, 1);
}

#[test]
fn should_resolve_session_abbreviations_before_builtin_keywords() {
    // Arrange
    let session = TemporalSession::default();
    let mut saw_keyword = false;

    // Act
    let actual = decode_datetime_checked("January 1 2024", &session, &mut |field, _| {
        saw_keyword |= field == "january";
        Ok::<_, ()>(None)
    });

    // Assert
    assert!(saw_keyword);
    assert!(actual.is_ok());
}
