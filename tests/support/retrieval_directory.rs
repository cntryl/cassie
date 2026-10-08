//! Remove retrieval fixture files only after their engine owner has dropped.
use std::io::Write;

pub(super) struct RetrievalDirectory {
    path: String,
}

impl RetrievalDirectory {
    pub(super) fn new(path: String) -> Self {
        Self { path }
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for RetrievalDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if std::thread::panicking() {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "retrieval directory cleanup failed during unwind: {}: {error}",
                    self.path
                );
                tracing::warn!(directory = %self.path, %error,
                    "retrieval directory cleanup failed during unwind");
            } else {
                panic!(
                    "unable to remove retrieval directory {}: {error}",
                    self.path
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_retrieval_directory_cleanup_failure_on_normal_drop() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-retrieval-cleanup-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"file instead of directory").expect("create cleanup failure");
        let directory = RetrievalDirectory::new(path.to_string_lossy().into_owned());

        // Act
        let outcome = std::panic::catch_unwind(|| drop(directory));

        // Assert
        assert!(outcome.is_err());
        assert!(path.is_file());
        std::fs::remove_file(path).expect("remove failure fixture");
    }

    #[test]
    fn should_preserve_retrieval_cleanup_unwind_panic() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-retrieval-unwind-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"file instead of directory").expect("create cleanup failure");
        let mut outcome = Ok(());

        // Act
        let warnings = crate::support_teardown_evidence::capture_warnings(|| {
            outcome = std::panic::catch_unwind(|| {
                let _directory = RetrievalDirectory::new(path.to_string_lossy().into_owned());
                panic!("original retrieval panic");
            });
        });

        // Assert
        let error = outcome.expect_err("original panic must survive");
        assert_eq!(
            error.downcast_ref::<&str>(),
            Some(&"original retrieval panic")
        );
        assert!(warnings.contains("retrieval directory cleanup failed during unwind"));
        assert!(path.is_file());
        std::fs::remove_file(path).expect("remove failure fixture");
    }
}
