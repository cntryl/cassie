use std::collections::BTreeMap;

use super::CassieError;

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
        kind: SettingKind::Fixed,
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
        kind: SettingKind::Fixed,
    },
];

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionSettings {
    mutable: BTreeMap<String, String>,
}

impl SessionSettings {
    pub(crate) fn get(&self, name: &str) -> Result<String, CassieError> {
        let spec = find(name)?;
        Ok(self
            .mutable
            .get(spec.name)
            .cloned()
            .unwrap_or_else(|| spec.default.to_string()))
    }

    pub(crate) fn set(&mut self, name: &str, value: &str) -> Result<String, CassieError> {
        let spec = find(name)?;
        let canonical = validate_value(spec, value)?;
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
    let normalized = name.trim().to_ascii_lowercase();
    SETTINGS
        .iter()
        .find(|spec| spec.name == normalized)
        .ok_or_else(|| {
            CassieError::InvalidParameterValue(format!(
                "unrecognized configuration parameter \"{name}\""
            ))
        })
}

fn validate_value(spec: &SettingSpec, value: &str) -> Result<String, CassieError> {
    let trimmed = value.trim();
    let valid = match spec.name {
        "application_name" => true,
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
        "datestyle" => matches!(trimmed.to_ascii_lowercase().as_str(), "iso" | "iso, mdy"),
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
        "datestyle" => "ISO, MDY".to_string(),
        "client_encoding" | "server_encoding" => "UTF8".to_string(),
        "client_min_messages" => trimmed.to_ascii_lowercase(),
        _ if spec.kind == SettingKind::Fixed => spec.default.to_string(),
        _ => trimmed.to_string(),
    })
}
