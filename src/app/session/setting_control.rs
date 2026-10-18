//! Connection reset defaults are independent from transactional rollback snapshots.

use super::{CassieError, CassieSession};

impl CassieSession {
    pub(crate) fn set_startup_setting(&self, name: &str, value: &str) -> Result<(), CassieError> {
        self.settings.lock().set_startup(name, value)
    }

    pub(crate) fn reset_setting(&self, name: &str) -> Result<(), CassieError> {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "transaction_read_only" | "transaction_isolation" | "transaction_deferrable"
        ) {
            return Err(CassieError::Unsupported(format!(
                "parameter \"{name}\" cannot be reset"
            )));
        }
        let value = self.settings.lock().reset(name)?;
        if name.eq_ignore_ascii_case("search_path") {
            self.set_search_path(
                value
                    .split(',')
                    .map(|name| name.trim().to_string())
                    .collect(),
            );
        }
        Ok(())
    }

    pub(crate) fn reset_all_settings(&self) {
        let mut settings = self.settings.lock();
        settings.reset_all();
        let search_path = settings
            .get_ref("search_path")
            .expect("registered search_path")
            .to_string();
        drop(settings);
        self.set_search_path(
            search_path
                .split(',')
                .map(|name| name.trim().to_string())
                .collect(),
        );
    }
}
