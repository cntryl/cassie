//! Native NULL element errors are atomic and Sync preserves connection use.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_null_at_position_0_given_option_arrays_when_execute_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY[NULL,'a=b']::TEXT[])";
    // Act
    let batches = planning::pruned("options-null-0", sql);
    // Assert
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22004"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}

#[test]
fn should_reject_null_at_position_1_given_option_arrays_when_execute_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=b',NULL]::TEXT[])";
    // Act
    let batches = planning::pruned("options-null-1", sql);
    // Assert
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22004"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}

#[test]
fn should_reject_null_at_position_2_given_option_arrays_when_execute_is_attempted() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=b',NULL,'x=y']::TEXT[])";
    // Act
    let batches = planning::pruned("options-null-2", sql);
    // Assert
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22004"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}
