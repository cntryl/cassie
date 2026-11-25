//! Connection reset defaults are independent from transactional rollback snapshots.

use super::{CassieError, CassieSession};
use crate::app::session_settings::SETTINGS;

impl CassieSession {
    #[doc(hidden)]
    pub fn set_startup_setting(&self, name: &str, value: &str) -> Result<(), CassieError> {
        Self::reject_projection_write_mode_change(name)?;
        self.settings.lock().set_startup(name, value)?;
        if name.eq_ignore_ascii_case("search_path") {
            self.reset_setting(name)?;
        }
        Ok(())
    }

    pub(crate) fn reset_setting(&self, name: &str) -> Result<(), CassieError> {
        Self::reject_projection_write_mode_change(name)?;
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "transaction_read_only" | "transaction_isolation" | "transaction_deferrable"
        ) {
            return Err(CassieError::Unsupported(format!(
                "parameter \"{name}\" cannot be reset"
            )));
        }
        self.apply_session_write(&[name], || {
            let value = self.settings.lock().reset(name)?;
            if name.eq_ignore_ascii_case("search_path") {
                self.set_search_path_locked(
                    value
                        .split(',')
                        .map(|name| name.trim().to_owned())
                        .collect(),
                );
            }
            Ok(())
        })
    }

    pub(crate) fn reset_all_settings(&self) -> Result<(), CassieError> {
        let names: Vec<&str> = SETTINGS.iter().map(|spec| spec.name).collect();
        self.apply_session_write(&names, || {
            let mut settings = self.settings.lock();
            settings.reset_all();
            let search_path = settings
                .get_ref("search_path")
                .expect("registered search_path")
                .to_owned();
            drop(settings);
            self.set_search_path_locked(
                search_path
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .collect(),
            );
            Ok(())
        })
    }
}

impl CassieSession {
    /// Tightens `controls` to this session's `statement_timeout`, measured
    /// from `started`; `0` leaves the configured deadline unchanged.
    #[doc(hidden)]
    pub fn apply_statement_timeout(
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
