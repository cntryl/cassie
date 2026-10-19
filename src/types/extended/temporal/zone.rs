//! Time zones: fixed displacements, IANA zones and POSIX rules (via the
//! bundled tz database), PostgreSQL's DST resolution and abbreviations.
use jiff::tz::{AmbiguousOffset, TimeZone};

use super::calendar::{timestamp_to_fields, USECS_PER_SEC};

/// A session or literal time zone.
#[derive(Debug, Clone)]
pub enum Zone {
    /// A fixed displacement, in seconds east of UTC.
    Fixed { east: i32, name: String },
    /// An IANA or POSIX zone with transition rules.
    Rules { tz: TimeZone, name: String },
}

impl PartialEq for Zone {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

/// The offset in effect at an instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneInfo {
    pub east: i32,
    pub abbreviation: String,
    pub dst: bool,
}

/// The Gregorian calendar repeats every 400 years (146097 days).
const CYCLE_SECS: i64 = 146_097 * 86_400;
/// jiff supports roughly years -9999..=9999; stay well inside.
const MIN_UNIX: i64 = -370_000_000_000;
const MAX_UNIX: i64 = 250_000_000_000;

fn unix_seconds(pg_micros: i64) -> i64 {
    let unix = pg_micros.div_euclid(USECS_PER_SEC) + super::calendar::UNIX_TO_POSTGRES_SECS;
    if unix > MAX_UNIX {
        unix - (unix - MAX_UNIX).div_euclid(CYCLE_SECS).saturating_add(1) * CYCLE_SECS
    } else if unix < MIN_UNIX {
        unix + (MIN_UNIX - unix).div_euclid(CYCLE_SECS).saturating_add(1) * CYCLE_SECS
    } else {
        unix
    }
}

fn jiff_timestamp(pg_micros: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(unix_seconds(pg_micros)).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
}

fn jiff_datetime(local_micros: i64) -> Option<jiff::civil::DateTime> {
    let shifted = (unix_seconds(local_micros) - super::calendar::UNIX_TO_POSTGRES_SECS)
        .checked_mul(USECS_PER_SEC)?;
    let fields = timestamp_to_fields(shifted)?;
    jiff::civil::DateTime::new(
        i16::try_from(fields.year).ok()?,
        i8::try_from(fields.month).ok()?,
        i8::try_from(fields.day).ok()?,
        i8::try_from(fields.hour).ok()?,
        i8::try_from(fields.minute).ok()?,
        i8::try_from(fields.second).ok()?,
        0,
    )
    .ok()
}

impl Zone {
    #[must_use]
    pub fn utc() -> Self {
        Self::Rules {
            tz: TimeZone::UTC,
            name: "UTC".to_string(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Fixed { name, .. } | Self::Rules { name, .. } => name,
        }
    }

    /// The offset, abbreviation and DST flag at a UTC instant.
    #[must_use]
    pub fn info_at(&self, utc_micros: i64) -> ZoneInfo {
        match self {
            Self::Fixed { east, .. } => ZoneInfo {
                east: *east,
                abbreviation: offset_abbreviation(*east),
                dst: false,
            },
            Self::Rules { tz, .. } => {
                let info = tz.to_offset_info(jiff_timestamp(utc_micros));
                ZoneInfo {
                    east: info.offset().seconds(),
                    abbreviation: info.abbreviation().to_string(),
                    dst: info.dst().is_dst(),
                }
            }
        }
    }

    #[must_use]
    pub fn offset_at(&self, utc_micros: i64) -> i32 {
        match self {
            Self::Fixed { east, .. } => *east,
            Self::Rules { tz, .. } => tz.to_offset(jiff_timestamp(utc_micros)).seconds(),
        }
    }

    /// `DetermineTimeZoneOffset`: the offset east for a local wall time. In a
    /// spring-forward gap the earlier offset applies; in a fall-back fold the
    /// later one does.
    #[must_use]
    pub fn offset_for_local(&self, local_micros: i64) -> i32 {
        match self {
            Self::Fixed { east, .. } => *east,
            Self::Rules { tz, .. } => {
                let Some(datetime) = jiff_datetime(local_micros) else {
                    return self.offset_at(local_micros);
                };
                match tz.to_ambiguous_timestamp(datetime).offset() {
                    AmbiguousOffset::Unambiguous { offset } => offset.seconds(),
                    AmbiguousOffset::Gap { before, .. } => before.seconds(),
                    AmbiguousOffset::Fold { after, .. } => after.seconds(),
                }
            }
        }
    }

    /// Converts a local wall time in this zone to UTC.
    #[must_use]
    pub fn local_to_utc(&self, local_micros: i64) -> i64 {
        local_micros.saturating_sub(i64::from(self.offset_for_local(local_micros)) * USECS_PER_SEC)
    }

    #[must_use]
    pub fn utc_to_local(&self, utc_micros: i64) -> i64 {
        utc_micros.saturating_add(i64::from(self.offset_at(utc_micros)) * USECS_PER_SEC)
    }

    /// The offset of `abbreviation` as used by this zone's own rules near
    /// `local_micros` (PostgreSQL 18 consults the session zone first).
    #[must_use]
    pub fn abbreviation_offset(
        &self,
        abbreviation: &str,
        local_micros: i64,
    ) -> Option<(i32, bool)> {
        let Self::Rules { tz, .. } = self else {
            return None;
        };
        let instant = jiff_timestamp(local_micros);
        let info = tz.to_offset_info(instant);
        if info.abbreviation().eq_ignore_ascii_case(abbreviation) {
            return Some((info.offset().seconds(), info.dst().is_dst()));
        }
        tz.preceding(instant)
            .take(64)
            .chain(tz.following(instant).take(64))
            .find(|transition| transition.abbreviation().eq_ignore_ascii_case(abbreviation))
            .map(|transition| (transition.offset().seconds(), transition.dst().is_dst()))
    }
}

/// The `+HH[:MM]` abbreviation PostgreSQL prints for fixed offsets.
#[must_use]
pub fn offset_abbreviation(east: i32) -> String {
    let sign = if east < 0 { '-' } else { '+' };
    let magnitude = east.unsigned_abs();
    let (hours, minutes, seconds) = (magnitude / 3600, magnitude / 60 % 60, magnitude % 60);
    if seconds != 0 {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    } else if minutes != 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}")
    }
}

/// `pg_tzset`: resolves a zone name case-insensitively against the tz
/// database, then as a POSIX rule.
#[must_use]
pub fn lookup_zone(name: &str) -> Option<Zone> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return None;
    }
    if let Ok(tz) = TimeZone::get(trimmed) {
        let canonical = tz
            .iana_name()
            .map_or_else(|| trimmed.to_string(), str::to_string);
        let canonical = if canonical.eq_ignore_ascii_case(trimmed) {
            canonical
        } else {
            trimmed.to_string()
        };
        return Some(Zone::Rules {
            tz,
            name: canonical,
        });
    }
    let posix_like =
        trimmed.starts_with('<') || trimmed.chars().next().is_some_and(char::is_alphabetic);
    if posix_like && trimmed.chars().any(|c| c.is_ascii_digit()) {
        if let Ok(tz) = TimeZone::posix(trimmed) {
            return Some(Zone::Rules {
                tz,
                name: trimmed.to_string(),
            });
        }
    }
    None
}

/// A fixed zone in PostgreSQL's POSIX spelling, e.g. `<+07>-07`.
#[must_use]
pub fn fixed_zone(east: i32) -> Zone {
    let abbreviation = offset_abbreviation(east);
    let posix = offset_abbreviation(-east);
    Zone::Fixed {
        east,
        name: format!("<{abbreviation}>{posix}"),
    }
}

/// The active `timezone_abbreviations` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbbreviationSet {
    #[default]
    Default,
    Australia,
    India,
}

impl AbbreviationSet {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "default" => Some(Self::Default),
            "australia" => Some(Self::Australia),
            "india" => Some(Self::India),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Australia => "Australia",
            Self::India => "India",
        }
    }

    /// The offset east and DST flag of a lower-case abbreviation.
    #[must_use]
    pub fn lookup(self, abbreviation: &str) -> Option<(i32, bool)> {
        let overrides: &[(&str, i32, bool)] = match self {
            Self::Default => &[],
            Self::Australia => AUSTRALIA,
            Self::India => &[("ist", 19_800, false)],
        };
        overrides
            .iter()
            .chain(DEFAULT_ABBREVIATIONS)
            .find(|(name, _, _)| *name == abbreviation)
            .map(|(_, east, dst)| (*east, *dst))
    }
}

const fn hm(hours: i32, minutes: i32) -> i32 {
    hours * 3600 + if hours < 0 { -minutes } else { minutes } * 60
}

const AUSTRALIA: &[(&str, i32, bool)] = &[
    ("acst", hm(9, 30), false),
    ("cst", hm(10, 30), false),
    ("east", hm(10, 0), false),
    ("est", hm(10, 0), false),
    ("sat", hm(9, 30), false),
    ("wst", hm(8, 0), false),
    ("aesst", hm(11, 0), true),
    ("cadt", hm(10, 30), true),
];

/// PostgreSQL's `Default` abbreviation set (`timezonesets/Default`).
const DEFAULT_ABBREVIATIONS: &[(&str, i32, bool)] = &[
    ("acdt", hm(10, 30), true),
    ("acsst", hm(10, 30), true),
    ("acst", hm(9, 30), false),
    ("act", hm(-5, 0), false),
    ("adt", hm(-3, 0), true),
    ("aedt", hm(11, 0), true),
    ("aesst", hm(11, 0), true),
    ("aest", hm(10, 0), false),
    ("aft", hm(4, 30), false),
    ("akdt", hm(-8, 0), true),
    ("akst", hm(-9, 0), false),
    ("almst", hm(7, 0), true),
    ("ast", hm(-4, 0), false),
    ("awsst", hm(9, 0), true),
    ("awst", hm(8, 0), false),
    ("bst", hm(1, 0), true),
    ("bt", hm(3, 0), false),
    ("cadt", hm(10, 30), true),
    ("cast", hm(9, 30), false),
    ("cct", hm(8, 0), false),
    ("cdt", hm(-5, 0), true),
    ("cest", hm(2, 0), true),
    ("cet", hm(1, 0), false),
    ("cetdst", hm(2, 0), true),
    ("cst", hm(-6, 0), false),
    ("cxt", hm(7, 0), false),
    ("eat", hm(3, 0), false),
    ("edt", hm(-4, 0), true),
    ("eest", hm(3, 0), true),
    ("eet", hm(2, 0), false),
    ("eetdst", hm(3, 0), true),
    ("est", hm(-5, 0), false),
    ("gmt", 0, false),
    ("hkt", hm(8, 0), false),
    ("hst", hm(-10, 0), false),
    ("idt", hm(3, 0), true),
    ("ist", hm(2, 0), false),
    ("jst", hm(9, 0), false),
    ("kst", hm(9, 0), false),
    ("mdt", hm(-6, 0), true),
    ("mest", hm(2, 0), true),
    ("mesz", hm(2, 0), true),
    ("met", hm(1, 0), false),
    ("metdst", hm(2, 0), true),
    ("mez", hm(1, 0), false),
    ("msd", hm(4, 0), true),
    ("msk", hm(3, 0), false),
    ("mst", hm(-7, 0), false),
    ("ndt", hm(-2, 30), true),
    ("nst", hm(-3, 30), false),
    ("nzdt", hm(13, 0), true),
    ("nzst", hm(12, 0), false),
    ("nzt", hm(12, 0), false),
    ("pdt", hm(-7, 0), true),
    ("pst", hm(-8, 0), false),
    ("sast", hm(2, 0), false),
    ("uct", 0, false),
    ("ut", 0, false),
    ("utc", 0, false),
    ("wadt", hm(8, 0), true),
    ("wast", hm(7, 0), false),
    ("wat", hm(1, 0), false),
    ("wet", 0, false),
    ("wetdst", hm(1, 0), true),
    ("ydt", hm(-8, 0), true),
    ("yst", hm(-9, 0), false),
    ("z", 0, false),
    ("zulu", 0, false),
];

#[cfg(test)]
mod tests {
    use super::{lookup_zone, offset_abbreviation, AbbreviationSet};
    use crate::types::extended::temporal::calendar::{fields_to_timestamp, Fields};

    fn local(year: i32, month: i32, day: i32, hour: i32, minute: i32) -> i64 {
        fields_to_timestamp(
            &Fields {
                year,
                month,
                day,
                hour,
                minute,
                ..Fields::default()
            },
            None,
        )
        .expect("valid local time")
    }

    #[test]
    fn should_resolve_dst_gap_and_fold_like_postgresql() {
        // Arrange
        let zone = lookup_zone("america/new_york").expect("New York zone");

        // Act
        let gap = zone.offset_for_local(local(2024, 3, 10, 2, 30));
        let fold = zone.offset_for_local(local(2024, 11, 3, 1, 30));

        // Assert
        assert_eq!((gap, fold), (-5 * 3600, -5 * 3600));
        assert_eq!(zone.name(), "America/New_York");
    }

    #[test]
    fn should_format_offset_abbreviations() {
        // Arrange
        let offsets = [7 * 3600, -(5 * 3600 + 30 * 60), 0];

        // Act
        let formatted = offsets.map(offset_abbreviation);

        // Assert
        assert_eq!(formatted, ["+07", "-05:30", "+00"]);
        assert_eq!(
            AbbreviationSet::Default.lookup("pst"),
            Some((-8 * 3600, false))
        );
    }
}
