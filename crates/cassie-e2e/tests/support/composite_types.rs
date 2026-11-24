//! Fixtures for composite type suites (#828): restartable local storage and
//! small result helpers. Ephemeral cases use `support_sql_fixture` instead.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

/// An isolated local-storage directory that outlives individual `Cassie` opens.
pub struct LocalStore {
    path: String,
}

impl LocalStore {
    #[must_use]
    pub fn new(label: &str) -> Self {
        crate::support_sql::use_local_storage();
        Self {
            path: crate::support_sql::data_dir(label),
        }
    }

    /// Opens the store, hydrating persisted catalog metadata.
    ///
    /// # Panics
    ///
    /// Panics when the store cannot be opened or started.
    #[must_use]
    pub fn open(&self) -> Cassie {
        let config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        let cassie =
            Cassie::new_with_data_dir_and_config(&self.path, config).expect("open local Cassie");
        cassie.startup().expect("start local Cassie");
        cassie
    }
}

impl Drop for LocalStore {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.path);
        assert!(
            removed.is_ok() || std::thread::panicking(),
            "unable to remove composite fixture directory {}",
            self.path
        );
    }
}

/// The single text or integer cell of a one-row, one-column result, as text.
///
/// # Panics
///
/// Panics when the rows are not exactly one text cell.
#[must_use]
pub fn only_text(rows: &[Vec<Value>]) -> Option<String> {
    let [row] = rows else {
        panic!("expected one row, got {}", rows.len());
    };
    match row.as_slice() {
        [Value::String(text)] => Some(text.clone()),
        [Value::Int64(number)] => Some(number.to_string()),
        [Value::Null] => None,
        _ => panic!("expected one text cell"),
    }
}

/// Every first-column cell as optional text, preserving order.
///
/// # Panics
///
/// Panics when a first cell is neither text nor NULL.
#[must_use]
pub fn first_column_text(rows: &[Vec<Value>]) -> Vec<Option<String>> {
    rows.iter()
        .map(|row| match row.first() {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Null) => None,
            _ => panic!("expected a text or NULL first cell"),
        })
        .collect()
}
