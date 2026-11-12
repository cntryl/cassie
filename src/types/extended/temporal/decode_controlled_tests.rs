use super::{decode_datetime, decode_datetime_with_check, TokenizeError};
use crate::types::extended::temporal::{zone::lookup_zone, TemporalSession};

#[test]
fn should_preserve_decoder_behavior_with_a_passing_check() {
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
        let actual = decode_datetime_with_check(input, &session, &mut || Ok::<(), ()>(()));
        assert_eq!(actual, expected.map_err(TokenizeError::Decode));
    }
}

#[test]
fn should_stop_and_return_the_original_check_error() {
    // Arrange
    let session = TemporalSession::default();
    let mut calls = 0;

    // Act
    let actual = decode_datetime_with_check("January 1 2024", &session, &mut || {
        calls += 1;
        Err("canceled")
    });

    // Assert
    assert_eq!(actual, Err(TokenizeError::Check("canceled")));
    assert_eq!(calls, 1);
}
