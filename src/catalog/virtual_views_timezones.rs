//! `pg_timezone_names`: the bundled IANA zones with their current offsets.
use super::VirtualRow;
use crate::types::extended::temporal::calendar::USECS_PER_SEC;
use crate::types::extended::temporal::{session::current_timestamp, zone, Interval};
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

pub(super) fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    (name == "pg_catalog.pg_timezone_names").then(|| {
        vec![
            ("name".to_string(), DataType::Text),
            ("abbrev".to_string(), DataType::Text),
            (
                "utc_offset".to_string(),
                DataType::Extended(crate::types::extended::ExtendedType::Interval { typmod: -1 }),
            ),
            ("is_dst".to_string(), DataType::Boolean),
        ]
    })
}

pub(super) fn rows(name: &str) -> Option<Vec<VirtualRow>> {
    if name != "pg_catalog.pg_timezone_names" {
        return None;
    }
    let now = current_timestamp();
    let mut names: Vec<String> = jiff::tz::db()
        .available()
        .map(|name| name.as_str().to_string())
        .filter(|name| !name.starts_with("posix/") && !name.starts_with("right/"))
        .collect();
    names.sort();
    Some(
        names
            .into_iter()
            .filter_map(|name| {
                let zone = zone::lookup_zone(&name)?;
                let info = zone.info_at(now);
                Some(vec![
                    ("name".to_string(), Value::String(name)),
                    ("abbrev".to_string(), Value::String(info.abbreviation)),
                    (
                        "utc_offset".to_string(),
                        Value::Typed(TypedValue::Interval(Interval::new(
                            0,
                            0,
                            i64::from(info.east) * USECS_PER_SEC,
                        ))),
                    ),
                    ("is_dst".to_string(), Value::Bool(info.dst)),
                ])
            })
            .collect(),
    )
}
