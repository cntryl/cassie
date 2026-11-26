//! Deterministic controls used by the synchronized and reopened oracles.

use super::{model, EventRow};

pub(super) fn should_advance_past_the_last_relation_id(
    previous: Option<i64>,
    rows: &[EventRow],
    page_limit: usize,
) -> i64 {
    // Arrange: the primary key is unique and the query orders by id.
    let mut cursor = previous;

    // Act: consume the page in the returned order.
    for row in rows {
        // Assert: duplicates, overlap or a stalled cursor cannot pass.
        assert!(
            cursor.is_none_or(|previous| row.id > previous),
            "relation cursor did not advance past {cursor:?}"
        );
        cursor = Some(row.id);
    }
    assert!(
        rows.len() <= page_limit,
        "relation page exceeded its row limit"
    );
    cursor.expect("relation page contained a row")
}

pub(super) fn should_include_the_acknowledged_checkpoint(
    checkpoint: &str,
    acknowledged_before_begin: u64,
) -> bool {
    // Arrange: the caller samples the acknowledged position before BEGIN.

    // Act: parse the checkpoint read inside the resulting snapshot.
    let position = model::checkpoint_position(checkpoint).expect("snapshot checkpoint position");

    // Assert: an internally consistent snapshot may still be stale.
    if position < acknowledged_before_begin {
        eprintln!(
            "mixed: stale snapshot checkpoint={checkpoint} acknowledged_before_begin={acknowledged_before_begin}"
        );
        return false;
    }
    true
}
