use std::thread::ScopedJoinHandle;

use super::QueryError;

pub(super) fn join_scoped_worker<T>(
    handle: ScopedJoinHandle<'_, T>,
    panic_message: &'static str,
) -> Result<T, QueryError> {
    handle
        .join()
        .map_err(|_| QueryError::General(panic_message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_convert_scoped_worker_panic_to_query_error() {
        // Arrange
        let panic_message = "test scoped worker panicked";

        // Act
        let result = std::thread::scope(|scope| {
            let handle = scope.spawn(|| panic!("injected worker panic"));
            join_scoped_worker(handle, panic_message)
        });

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::General(message)) if message == panic_message
        ));
    }
}
