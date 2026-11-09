use std::time::Instant;

use super::super::{Cursor, CursorError};
use super::{Literal, LiteralDiagnostic, LiteralError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_read_literals_on_demand_and_leave_following_input() {
    // Arrange
    let controls = controls();
    let cases = [
        ("true,", Literal::True),
        ("false]", Literal::False),
        ("null}", Literal::Null),
    ];

    // Act / Assert
    for (input, expected) in cases {
        let mut cursor = Cursor::from_str(input, &controls);
        assert_eq!(cursor.read_literal().expect("literal"), expected);
        assert!(matches!(cursor.peek(), Some(b',' | b']' | b'}')));
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_serde_literal_error_categories_and_positions() {
    // Arrange
    let controls = controls();
    let inputs = ["", "X", "trXe", "tr", "falze", "nu"];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
        let mut cursor = Cursor::from_str(input, &controls);
        let Err(CursorError::Literal(LiteralDiagnostic { kind, position })) = cursor.read_literal()
        else {
            panic!("literal error for {input:?}");
        };
        let category = if kind == LiteralError::Eof {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        assert_eq!(expected.classify(), category, "{input:?}");
        assert_eq!(
            (expected.line(), expected.column()),
            (position.line, position.column),
            "{input:?}"
        );
    }
}

#[test]
fn should_cancel_during_real_literal_consumption() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut cursor = Cursor::from_str("true", &controls);

    // Act
    let result = cursor.read_literal_observed(|| cancellation.cancel());

    // Assert
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(CassieError::QueryCancelled))
    ));
    assert_eq!(cursor.position().offset, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
