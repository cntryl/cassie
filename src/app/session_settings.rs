use std::collections::BTreeMap;

use serde::Serialize;

use super::CassieError;

#[path = "session_settings/temporal.rs"]
mod temporal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingKind {
    Mutable,
    Fixed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingSpec {
    pub(crate) name: &'static str,
    pub(crate) default: &'static str,
    pub(crate) kind: SettingKind,
}

pub(crate) const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        name: "application_name",
        default: "",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "bytea_output",
        default: "hex",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "client_encoding",
        default: "UTF8",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "client_min_messages",
        default: "notice",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "datestyle",
        default: "ISO, MDY",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "default_transaction_read_only",
        default: "off",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "default_transaction_deferrable",
        default: "off",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "extra_float_digits",
        default: "3",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "integer_datetimes",
        default: "on",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "intervalstyle",
        default: "postgres",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "search_path",
        default: "public",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "server_encoding",
        default: "UTF8",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "server_version",
        default: "16.0",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "standard_conforming_strings",
        default: "on",
        kind: SettingKind::Fixed,
    },
    SettingSpec {
        name: "timezone",
        default: "UTC",
        kind: SettingKind::Mutable,
    },
    SettingSpec {
        name: "timezone_abbreviations",
        default: "Default",
        kind: SettingKind::Mutable,
    },
];

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SessionSettings {
    mutable: BTreeMap<String, String>,
    startup: BTreeMap<String, String>,
}

impl SessionSettings {
    pub(crate) fn get(&self, name: &str) -> Result<String, CassieError> {
        Ok(self.get_ref(name)?.to_owned())
    }

    pub(crate) fn get_ref(&self, name: &str) -> Result<&str, CassieError> {
        let spec = find(name)?;
        Ok(self
            .mutable
            .get(spec.name)
            .map_or(spec.default, String::as_str))
    }

    pub(crate) fn set_startup(&mut self, name: &str, value: &str) -> Result<(), CassieError> {
        let canonical = self.set(name, value)?;
        self.startup.insert(find(name)?.name.to_string(), canonical);
        Ok(())
    }

    pub(crate) fn reset(&mut self, name: &str) -> Result<String, CassieError> {
        let spec = find(name)?;
        let value = self
            .startup
            .get(spec.name)
            .map_or(spec.default, String::as_str)
            .to_string();
        if spec.kind == SettingKind::Mutable {
            self.mutable.insert(spec.name.to_string(), value.clone());
        }
        Ok(value)
    }

    /// Restores one setting's value from an earlier snapshot.
    pub(crate) fn restore_from(&mut self, name: &str, snapshot: &Self) {
        let Ok(spec) = find(name) else {
            return;
        };
        match snapshot.mutable.get(spec.name) {
            Some(value) => {
                self.mutable.insert(spec.name.to_string(), value.clone());
            }
            None => {
                self.mutable.remove(spec.name);
            }
        }
    }

    pub(crate) fn reset_all(&mut self) {
        self.mutable.clone_from(&self.startup);
    }

    pub(crate) fn set(&mut self, name: &str, value: &str) -> Result<String, CassieError> {
        let spec = find(name)?;
        let current = self
            .mutable
            .get(spec.name)
            .map_or(spec.default, String::as_str);
        let canonical = match temporal::canonical(spec.name, value, current) {
            Some(canonical) => canonical?,
            None => validate_value(spec, value)?,
        };
        if spec.kind == SettingKind::Mutable {
            self.mutable
                .insert(spec.name.to_string(), canonical.clone());
        }
        Ok(canonical)
    }
}

pub(crate) fn all() -> &'static [SettingSpec] {
    SETTINGS
}

fn find(name: &str) -> Result<&'static SettingSpec, CassieError> {
    SETTINGS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            CassieError::InvalidParameterValue(format!(
                "unrecognized configuration parameter \"{name}\""
            ))
        })
}

fn validate_value(spec: &SettingSpec, value: &str) -> Result<String, CassieError> {
    let trimmed = value.trim();
    let valid = match spec.name {
        "application_name" | "search_path" => true,
        "default_transaction_read_only" | "default_transaction_deferrable" => {
            parse_bool(trimmed).is_ok()
        }
        "client_min_messages" => matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "debug5"
                | "debug4"
                | "debug3"
                | "debug2"
                | "debug1"
                | "log"
                | "notice"
                | "warning"
                | "error"
        ),
        "client_encoding" | "server_encoding" => {
            matches!(trimmed.to_ascii_lowercase().as_str(), "utf8" | "utf-8")
        }
        _ => trimmed.eq_ignore_ascii_case(spec.default),
    };
    if !valid {
        return Err(CassieError::InvalidParameterValue(format!(
            "invalid value for parameter \"{}\": \"{trimmed}\"",
            spec.name
        )));
    }
    Ok(match spec.name {
        "client_encoding" | "server_encoding" => "UTF8".to_string(),
        "client_min_messages" => trimmed.to_ascii_lowercase(),
        "default_transaction_read_only" | "default_transaction_deferrable" => {
            if parse_bool(trimmed)? { "on" } else { "off" }.to_string()
        }
        _ if spec.kind == SettingKind::Fixed => spec.default.to_string(),
        _ => trimmed.to_string(),
    })
}

pub(super) fn parse_bool(value: &str) -> Result<bool, CassieError> {
    crate::types::boolean::parse_text(value).ok_or_else(|| {
        CassieError::InvalidParameterValue(format!("invalid boolean value: {value}"))
    })
}
