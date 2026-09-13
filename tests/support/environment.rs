use std::env;
use std::sync::{Mutex, MutexGuard, PoisonError};

static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

pub struct EnvironmentGuard {
    previous: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvironmentGuard {
    pub fn set(key: &'static str, value: &str) -> Self {
        let lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let previous = vec![(key, env::var(key).ok())];
        env::set_var(key, value);
        Self {
            previous,
            _lock: lock,
        }
    }

    pub fn unset(keys: &[&'static str]) -> Self {
        let lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let previous = keys.iter().map(|key| (*key, env::var(key).ok())).collect();
        for key in keys {
            env::remove_var(key);
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}
