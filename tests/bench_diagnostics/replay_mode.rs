//! Selector probes use the same module compiled into the replay benchmark.

#[path = "../../benches/tier3_replay_durability/mode.rs"]
mod mode;

use mode::ReplayMode;
use std::ffi::OsStr;

#[test]
fn should_default_to_strict_replay_mode() {
    // Arrange
    let input = None;

    // Act
    let mode = ReplayMode::parse(input);

    // Assert
    assert_eq!(mode, Ok(ReplayMode::Strict));
}

#[test]
fn should_select_each_explicit_replay_mode() {
    // Arrange
    let modes = [OsStr::new("local_strict"), OsStr::new("local_buffered")];

    // Act
    let actual = modes.map(|mode| ReplayMode::parse(Some(mode)));

    // Assert
    assert_eq!(actual, [Ok(ReplayMode::Strict), Ok(ReplayMode::Buffered)]);
}

#[test]
fn should_reject_unknown_replay_mode() {
    // Arrange
    let mode = OsStr::new("buffered");

    // Act
    let actual = ReplayMode::parse(Some(mode));

    // Assert
    assert!(actual.is_err());
}

#[cfg(unix)]
#[test]
fn should_reject_non_unicode_replay_mode() {
    // Arrange
    use std::os::unix::ffi::OsStringExt;
    let mode = std::ffi::OsString::from_vec(vec![0xff]);

    // Act
    let actual = ReplayMode::parse(Some(&mode));

    // Assert
    assert!(actual.is_err());
}

#[test]
fn should_report_the_selected_replay_api_policy() {
    // Arrange
    let modes = [ReplayMode::Strict, ReplayMode::Buffered];

    // Act
    let actual = modes.map(|mode| (mode.label(), mode.api(), mode.native_policy()));

    // Assert
    assert_eq!(
        actual,
        [
            (
                "local_strict",
                "replay_projection_batch_strict",
                "local_sync"
            ),
            (
                "local_buffered",
                "replay_projection_batch",
                "local_buffered"
            ),
        ]
    );
}
