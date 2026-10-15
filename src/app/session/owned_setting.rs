use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::runtime::accounted::Accounted;
use crate::runtime::owned_memory::allocation_error;
use crate::runtime::QueryExecutionControls;

use super::CassieSession;

impl CassieSession {
    pub(crate) fn with_search_path<R>(&self, inspect: impl FnOnce(&[String]) -> R) -> R {
        let path = self.search_path.lock();
        inspect(&path)
    }

    pub(crate) fn accounted_setting(
        &self,
        name: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Accounted<String>, CassieError> {
        controls.check_interruption()?;
        if name.trim().eq_ignore_ascii_case("search_path") {
            let path = self.search_path.lock();
            let bytes = path
                .iter()
                .enumerate()
                .try_fold(0, |bytes, (index, name)| {
                    add(add(bytes, name.len())?, if index == 0 { 0 } else { 2 })
                })?;
            let memory = controls.reserve_query_memory(add(bytes, size_of::<String>())?)?;
            let mut value = String::new();
            value
                .try_reserve_exact(bytes)
                .map_err(|error| allocation_error(&error))?;
            for (index, name) in path.iter().enumerate() {
                if index != 0 {
                    value.push_str(", ");
                }
                value.push_str(name);
            }
            return Ok(Accounted::from_admitted(value, memory));
        }
        if let Some(value) = self.transaction_setting(name) {
            return Accounted::try_new(controls, add(value.len(), size_of::<String>())?, || {
                value.to_owned()
            });
        }
        let settings = self.settings.lock();
        let value = settings.get_ref(name)?;
        Accounted::try_new(controls, add(value.len(), size_of::<String>())?, || {
            value.to_owned()
        })
    }
}
