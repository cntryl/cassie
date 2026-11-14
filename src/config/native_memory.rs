use super::CassieRuntimeConfigError;

pub(super) fn optional_positive_bytes(
    reader: &impl Fn(&str) -> Option<String>,
    key: &'static str,
) -> Result<Option<usize>, CassieRuntimeConfigError> {
    reader(key)
        .map(|raw| {
            raw.trim()
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(CassieRuntimeConfigError::InvalidNativeMemoryBytes { key })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::config::{CassieRuntimeConfig, CassieRuntimeConfigError};
    use std::collections::HashMap;

    #[test]
    fn should_leave_native_memory_settings_unset_by_default() {
        // Arrange
        // Act
        let config = CassieRuntimeConfig::from_env_reader(|_| None).expect("default config");

        // Assert
        assert_eq!(config.storage_memory_budget_bytes, None);
        assert_eq!(config.storage_memtable_size_bytes, None);
    }

    #[test]
    fn should_parse_explicit_positive_native_memory_settings() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_STORAGE_MEMORY_BUDGET_BYTES", " 67108864 "),
            ("CASSIE_STORAGE_MEMTABLE_SIZE_BYTES", "16777216"),
        ]);

        // Act
        let config = CassieRuntimeConfig::from_env_reader(|key| {
            values.get(key).map(|value| (*value).to_owned())
        })
        .expect("positive byte settings");

        // Assert
        assert_eq!(config.storage_memory_budget_bytes, Some(64 * 1024 * 1024));
        assert_eq!(config.storage_memtable_size_bytes, Some(16 * 1024 * 1024));
    }

    #[test]
    fn should_reject_invalid_native_memory_byte_settings() {
        // Arrange
        let keys = [
            "CASSIE_STORAGE_MEMORY_BUDGET_BYTES",
            "CASSIE_STORAGE_MEMTABLE_SIZE_BYTES",
        ];
        let invalid = [
            "0",
            "-1",
            "",
            "bytes",
            "9999999999999999999999999999999999999",
        ];

        // Act
        let errors = keys
            .into_iter()
            .flat_map(|key| {
                invalid.map(move |value| {
                    let result = CassieRuntimeConfig::from_env_reader(|candidate| {
                        (candidate == key).then(|| value.to_owned())
                    });
                    (
                        key,
                        result.expect_err("invalid bytes must reject configuration"),
                    )
                })
            })
            .collect::<Vec<_>>();

        // Assert
        for (key, error) in errors {
            assert!(
                matches!(error, CassieRuntimeConfigError::InvalidNativeMemoryBytes { key: actual } if actual == key)
            );
        }
    }
}
