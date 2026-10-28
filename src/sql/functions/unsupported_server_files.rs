//! Explicit rejection policy for PostgreSQL server filesystem helpers.

pub(crate) fn is_server_file_helper(name: &str) -> bool {
    let name = name
        .strip_prefix("pg_catalog.")
        .unwrap_or(name)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "pg_ls_archive_statusdir"
            | "pg_ls_dir"
            | "pg_ls_logdir"
            | "pg_ls_logicalmapdir"
            | "pg_ls_logicalsnapdir"
            | "pg_ls_replslotdir"
            | "pg_ls_summariesdir"
            | "pg_ls_tmpdir"
            | "pg_ls_waldir"
            | "pg_read_binary_file"
            | "pg_read_file"
            | "pg_stat_file"
    )
}

pub(crate) fn rejection() -> crate::CassieError {
    crate::CassieError::Unsupported("PostgreSQL server filesystem access is not supported".into())
}

#[cfg(test)]
mod tests {
    use super::is_server_file_helper;

    #[test]
    fn should_recognize_catalog_qualified_and_unqualified_file_helpers() {
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
