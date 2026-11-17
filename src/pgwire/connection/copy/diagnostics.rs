//! Resource-bounded COPY error diagnostics retain their operation owner.
use super::{
    cassie_pg_error, fail_copy_transaction, write_copy_error, AsyncWrite, CassieSession,
    PgWireError, PgWireSeverity, SimpleCopyOutcome,
};

pub(super) async fn write_copy_jsonb_diagnostic_with_length(
    write_half: &mut (impl AsyncWrite + Unpin),
    session: &CassieSession,
    message: &str,
    message_len: usize,
    controls: &crate::runtime::QueryExecutionControls,
) -> bool {
    if !copy_jsonb_diagnostic_fits(message_len) {
        fail_copy_transaction(session).await;
        return false;
    }
    let error =
        match PgWireError::controlled_diagnostic(PgWireSeverity::Error, "42601", message, controls)
        {
            Ok(error) => error,
            Err(error) => cassie_pg_error(&error),
        };
    matches!(
        write_copy_error(write_half, session, &error).await,
        SimpleCopyOutcome::Handled
    )
}

pub(super) fn copy_jsonb_diagnostic_fits(message_len: usize) -> bool {
    // 4-byte length, final terminator, and S/C/M tags with their terminators.
    message_len
        .checked_add(21)
        .is_some_and(|length| length <= super::super::writers::MAX_BACKEND_FRAME_BYTES)
}

pub(super) async fn write_copy_unknown_column_with_length(
    write_half: &mut (impl AsyncWrite + Unpin),
    session: &CassieSession,
    name: &str,
    name_len: usize,
    controls: &crate::runtime::QueryExecutionControls,
) -> bool {
    if !PgWireError::copy_undefined_column_fits(name_len) {
        fail_copy_transaction(session).await;
        return false;
    }
    let error = match PgWireError::copy_undefined_column(name, controls) {
        Ok(error) => error,
        Err(error) => cassie_pg_error(&error),
    };
    matches!(
        write_copy_error(write_half, session, &error).await,
        SimpleCopyOutcome::Handled
    )
}
