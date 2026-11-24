//! Equality-result oracle, shared with focused diagnostic probes.

use cassie::types::Value;

pub(super) fn assert_bucket(rows: &[Vec<Value>], expected: usize, title: &str) {
    assert_eq!(rows.len(), expected, "title bucket {title}");
    for row in rows {
        match row.as_slice() {
            [Value::String(actual)] => assert_eq!(actual, title, "title bucket value"),
            row => panic!("expected one non-null title column for {title}, got {row:?}"),
        }
    }
}
