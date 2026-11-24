//! Explicit rejection policy for PostgreSQL server control and signaling.

pub(crate) fn is_server_control_helper(name: &str) -> bool {
    let name = name
        .strip_prefix("pg_catalog.")
        .unwrap_or(name)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "pg_log_backend_memory_contexts"
            | "pg_reload_conf"
            | "pg_rotate_logfile"
            | "pg_stat_reset_backend_stats"
    )
}

pub(crate) fn rejection() -> crate::CassieError {
    crate::CassieError::Unsupported(
        "PostgreSQL server signaling and control operations are not supported".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::is_server_control_helper;

    #[test]
    fn should_recognize_qualified_case_insensitive_server_control_helpers() {
        // Arrange
        let qualified = "pg_catalog.pg_reload_conf";
        let mixed_case = "PG_ROTATE_LOGFILE";
        let unknown = "pg_custom_reload_conf";

        // Act
        let recognized = [
            is_server_control_helper(qualified),
            is_server_control_helper(mixed_case),
            is_server_control_helper("pg_log_backend_memory_contexts"),
            is_server_control_helper("pg_stat_reset_backend_stats"),
        ];

        // Assert
        assert_eq!(recognized, [true, true, true, true]);
        assert!(!is_server_control_helper(unknown));
    }
}
