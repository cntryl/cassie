use std::time::Instant;

use super::super::cursor::{Cursor, CursorError};
use super::{SourceProgress, SourceStack};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_preserve_cursor_position_and_reset_nested_source_depth() {
    // Arrange
    let controls = controls();
    let mut sources = SourceStack::from_str("a\nb", &controls).expect("root");
    assert_eq!(sources.step(Cursor::next).expect("a"), Some(b'a'));
    sources.test_set_remaining(23);
    let mut token_cursor = Cursor::from_str("\"nested\"", &controls);
    let nested = token_cursor.read_string().expect("nested");

    // Act
    sources.push_owned(nested).expect("nested frame");

    // Assert
    assert_eq!(sources.test_remaining(), 128);
    assert_eq!(sources.step(Cursor::next).expect("nested byte"), Some(b'n'));
    sources.pop().expect("pop");
    assert_eq!(sources.test_remaining(), 23);
    assert_eq!(sources.step(Cursor::next).expect("newline"), Some(b'\n'));
    let position = sources.test_position();
    assert_eq!((position.offset, position.line, position.column), (2, 2, 0));
    drop(sources);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_popped_frame_positions_and_release_nested_owners() {
    // Arrange
    let controls = controls();
    let mut sources = SourceStack::from_str("root", &controls).expect("root");
    let baseline = controls.current_query_memory_bytes();

    // Act / Assert
    for _ in 0..3 {
        let mut cursor = Cursor::from_str("\"abc\"", &controls);
        sources
            .push_owned(cursor.read_string().expect("token"))
            .expect("push");
        assert_eq!(sources.step(Cursor::next).expect("byte"), Some(b'a'));
        sources.pop().expect("pop");
        assert_eq!(controls.current_query_memory_bytes(), baseline);
    }
    drop(sources);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_nested_source_from_another_tracker() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let mut sources = SourceStack::from_str("root", &owner_controls).expect("root");
    let mut cursor = Cursor::from_str("\"abc\"", &other_controls);
    let token = cursor.read_string().expect("token");

    // Act
    let result = sources.push_owned(token);

    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    assert_eq!(sources.step(Cursor::next).expect("root"), Some(b'r'));
    drop(sources);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_sources_when_cancelled_during_frame_migration() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut sources = SourceStack::from_str("root", &controls).expect("root");
    for _ in 0..3 {
        let mut cursor = Cursor::from_str("\"x\"", &controls);
        sources
            .push_owned(cursor.read_string().expect("token"))
            .expect("push");
    }
    let mut cursor = Cursor::from_str("\"y\"", &controls);
    let candidate = cursor.read_string().expect("candidate");

    // Act
    let result = sources.push_owned_observed(candidate, |progress| {
        if matches!(progress, SourceProgress::Moved) {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(sources.test_len(), 4);
    assert_eq!(sources.test_owned_text(1), Some("x"));
    assert_eq!(sources.test_owned_text(3), Some("x"));
    drop(sources);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_invalid_binary_source_before_any_cursor_step() {
    // Arrange
    let controls = controls();

    // Act / Assert
    assert!(matches!(
        SourceStack::from_bytes(b"{x\xff", &controls),
        Err(CursorError::InvalidUtf8)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_save_consumed_position_even_when_a_cursor_step_fails() {
    // Arrange
    let controls = controls();
    let mut sources = SourceStack::from_str("abc", &controls).expect("root");

    // Act
    let result: Result<(), CursorError> = sources.step(|cursor| {
        let _ = cursor.next()?;
        Err(CursorError::InvalidUtf8)
    });

    // Assert
    assert!(matches!(result, Err(CursorError::InvalidUtf8)));
    assert_eq!(sources.test_position().offset, 1);
    assert_eq!(sources.step(Cursor::next).expect("next"), Some(b'b'));
    assert!(sources.pop().is_err());
    drop(sources);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_old_and_new_frame_directories_before_growth() {
    fn check(limit: usize, should_succeed: bool) {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: limit,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let mut sources = SourceStack::from_str("root", &controls).expect("root");
        for _ in 0..3 {
            let mut cursor = Cursor::from_str("\"x\"", &controls);
            sources
                .push_owned(cursor.read_string().expect("token"))
                .expect("push");
        }
        let baseline = controls.current_query_memory_bytes();
        let mut cursor = Cursor::from_str("\"y\"", &controls);
        let token = cursor.read_string().expect("candidate");
        let result = sources.push_owned(token);
        if should_succeed {
            assert!(result.is_ok());
            assert_eq!(sources.test_len(), 5);
        } else {
            assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
            assert_eq!(sources.test_len(), 4);
            assert_eq!(controls.current_query_memory_bytes(), baseline);
        }
        drop(sources);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    // Arrange
    let entry = std::mem::size_of::<Option<super::SourceFrame<'_>>>();
    let exact = 12 * entry + 16;

    // Act / Assert
    check(exact, true);
    check(exact - 1, false);
}
