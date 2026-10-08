//! A local-storage Cassie instance seeded by SQL statements, for tests that
//! only need to run queries and inspect rows or errors.
#![allow(dead_code)]

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;
use std::io::Write;

pub struct SqlFixture {
    // Fields drop in declaration order; directory cleanup must come last.
    pub session: CassieSession,
    pub cassie: Cassie,
    path: FixtureDirectory,
}

struct FixtureDirectory {
    path: String,
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if std::thread::panicking() {
                // Report even without a subscriber; diagnostic I/O failure must not
                // replace the panic that is already unwinding.
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "fixture directory cleanup failed during unwind: {}: {error}",
                    self.path
                );
                tracing::warn!(
                    directory = %self.path,
                    %error,
                    "fixture directory cleanup failed during unwind"
                );
            } else {
                panic!("unable to remove fixture directory {}: {error}", self.path);
            }
        }
    }
}

/// Opens a fresh local-storage instance and runs every `setup` statement.
///
/// # Panics
///
/// Panics when the instance cannot be created or a setup statement fails.
pub fn sql_fixture(label: &str, setup: &[&str]) -> SqlFixture {
    sql_fixture_with_config(
        label,
        setup,
        cassie::config::CassieRuntimeConfig::from_env().expect("runtime config"),
    )
}

/// Opens a fresh instance with explicit query controls and seeds it by SQL.
///
/// # Panics
///
/// Panics when the instance cannot be created or a setup statement fails.
pub fn sql_fixture_with_config(
    label: &str,
    setup: &[&str],
    config: cassie::config::CassieRuntimeConfig,
) -> SqlFixture {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        assert!(
            cassie.execute_sql(&session, sql, vec![]).is_ok(),
            "setup statement failed: {sql}"
        );
    }
    SqlFixture {
        cassie,
        session,
        path: FixtureDirectory { path },
    }
}

impl SqlFixture {
    /// Runs `sql` and returns the full result.
    ///
    /// # Errors
    ///
    /// Returns the engine error for a failing statement.
    pub fn execute(&self, sql: &str) -> Result<QueryResult, CassieError> {
        self.cassie.execute_sql(&self.session, sql, vec![])
    }

    /// Runs `sql` and returns its rows.
    ///
    /// # Panics
    ///
    /// Panics when the statement fails.
    pub fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        let Ok(result) = self.execute(sql) else {
            panic!("query failed: {sql}");
        };
        result.rows
    }

    /// Runs `sql`, which must fail, and returns the error.
    ///
    /// # Panics
    ///
    /// Panics when the statement succeeds.
    pub fn error(&self, sql: &str) -> CassieError {
        match self.execute(sql) {
            Ok(_) => panic!("query should fail: {sql}"),
            Err(error) => error,
        }
    }
}

impl Drop for SqlFixture {
    fn drop(&mut self) {
        // Retain the prohibition on moving public owners out of this fixture.
        // The session, Cassie and directory guard then drop in declaration order.
    }
}

#[cfg(test)]
#[path = "teardown_evidence.rs"]
mod teardown_evidence;

#[cfg(test)]
mod teardown_tests {
    use super::*;

    #[test]
    fn should_drop_sql_fixture_owners_before_removing_directory() {
        // Arrange
        crate::support_sql::use_local_storage();
        for startup in [false, true] {
            let fixture = sql_fixture("782-generic-drop-order", &[]);
            if startup {
                fixture.cassie.startup().expect("startup control");
            }
            let path = fixture.path.path.clone();
            let result = fixture
                .execute("SELECT CAST(7 AS BIGINT) AS n UNION ALL SELECT CAST(8 AS BIGINT) AS n")
                .expect("same generic SQL");
            assert_eq!(
                result.rows,
                vec![vec![Value::Int64(7)], vec![Value::Int64(8)]]
            );
            drop(result);

            // Act
            let captured = teardown_evidence::capture_warnings(|| drop(fixture));

            // Assert
            assert!(
                !std::path::Path::new(&path).exists(),
                "strict cleanup removed owned path"
            );
            assert!(
                !captured.contains("Midge graceful shutdown did not complete"),
                "owners must close before directory cleanup: {captured}"
            );
        }
    }

    #[test]
    fn should_report_directory_cleanup_failure_on_normal_drop() {
        // Arrange
        let directory = FixtureDirectory {
            path: std::env::temp_dir()
                .join(format!("cassie-missing-fixture-{}", uuid::Uuid::new_v4()))
                .to_string_lossy()
                .into_owned(),
        };

        // Act
        let result = std::panic::catch_unwind(|| drop(directory));

        // Assert
        assert!(
            result.is_err(),
            "normal cleanup failure must remain visible"
        );
    }

    #[test]
    fn should_report_cleanup_error_without_replacing_an_existing_panic() {
        // Arrange
        let path = std::env::temp_dir()
            .join(format!("cassie-missing-fixture-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let mut caught = None;

        // Act
        let captured = teardown_evidence::capture_warnings(|| {
            caught = Some(std::panic::catch_unwind(|| {
                let _directory = FixtureDirectory { path };
                panic!("original fixture failure");
            }));
        });

        // Assert
        let error = caught
            .expect("unwind observed")
            .expect_err("original panic");
        assert_eq!(
            error.downcast_ref::<&str>(),
            Some(&"original fixture failure")
        );
        assert!(captured.contains("fixture directory cleanup failed during unwind"));
    }

    #[test]
    fn should_report_cleanup_error_during_unwind_without_a_subscriber() {
        // Arrange
        const CHILD_PROBE: &str = "CASSIE_TEST_FIXTURE_CLEANUP_REPORT_CHILD";
        if std::env::var_os(CHILD_PROBE).is_some() {
            let path = std::env::temp_dir()
                .join(format!("cassie-missing-fixture-{}", uuid::Uuid::new_v4()))
                .to_string_lossy()
                .into_owned();

            // Execute the isolated child's unwind probe.
            let result = std::panic::catch_unwind(|| {
                let _directory = FixtureDirectory { path };
                panic!("original fixture failure");
            });

            // Preserve the original panic for the parent protocol.
            assert_eq!(
                result.expect_err("original panic").downcast_ref::<&str>(),
                Some(&"original fixture failure")
            );
            return;
        }
        let executable = std::env::current_exe().expect("test executable");

        // Act
        let result = std::process::Command::new(executable)
            .arg("support_sql_fixture::teardown_tests::should_report_cleanup_error_during_unwind_without_a_subscriber")
            .arg("--exact")
            .arg("--nocapture")
            .env(CHILD_PROBE, "1")
            .output()
            .expect("isolated unwind probe");

        // Assert
        assert!(result.status.success(), "original unwind remains catchable");
        let stderr = String::from_utf8(result.stderr).expect("UTF8 diagnostics");
        assert!(
            stderr.contains("fixture directory cleanup failed during unwind"),
            "cleanup failure must remain visible without logging configuration: {stderr}"
        );
    }
}
