//! Explicit rejection policy for PostgreSQL server filesystem helpers.

pub(crate) use cassie_sql::registry::functions::is_server_file_helper;

pub(crate) fn rejection() -> crate::CassieError {
    crate::CassieError::Unsupported("PostgreSQL server filesystem access is not supported".into())
}

#[cfg(test)]
mod tests {
    use super::is_server_file_helper;

    #[test]
    fn should_recognize_builtin_file_helper_names() {
        // Arrange
        let qualified = "pg_catalog.pg_read_file";
        let unqualified = "PG_LS_LOGDIR";

        // Act
        let qualified_is_file_helper = is_server_file_helper(qualified);
        let unqualified_is_file_helper = is_server_file_helper(unqualified);
        let unknown_is_file_helper = is_server_file_helper("pg_custom_read_file");

        // Assert
        assert!(qualified_is_file_helper);
        assert!(unqualified_is_file_helper);
        assert!(!unknown_is_file_helper);
    }
}
