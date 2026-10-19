//! Connection reset defaults are independent from transactional rollback snapshots.

use super::{CassieError, CassieSession};

impl CassieSession {
    pub(crate) fn set_startup_setting(&self, name: &str, value: &str) -> Result<(), CassieError> {
        self.settings.lock().set_startup(name, value)?;
        if name.eq_ignore_ascii_case("search_path") {
            self.reset_setting(name)?;
        }
        Ok(())
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

impl CassieSession {
    /// Tightens `controls` to this session's `statement_timeout`, measured
    /// from `started`; `0` leaves the configured deadline unchanged.
    pub(crate) fn apply_statement_timeout(
        &self,
        controls: &mut crate::runtime::QueryExecutionControls,
        started: std::time::Instant,
    ) {
        let millis = self
            .settings
            .lock()
            .get_ref("statement_timeout")
            .ok()
            .and_then(|value| {
                crate::app::session_settings::parse_milliseconds_setting("statement_timeout", value)
                    .ok()
            })
            .and_then(|millis| u64::try_from(millis).ok())
            .filter(|millis| *millis > 0);
        let Some(millis) = millis else {
            return;
        };
        let deadline = started + std::time::Duration::from_millis(millis);
        controls.deadline = Some(
            controls
                .deadline
                .map_or(deadline, |current| current.min(deadline)),
        );
    }
}
