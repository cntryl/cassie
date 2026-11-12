//! Time zones: fixed displacements, IANA zones and POSIX rules (via the
//! bundled tz database), PostgreSQL's DST resolution and abbreviations.
use std::sync::Arc;

use jiff::tz::{AmbiguousOffset, TimeZone};

use super::calendar::{timestamp_to_fields, USECS_PER_SEC};
#[path = "tzif.rs"]
mod tzif;
use self::tzif::{RawLocalTimeType, RawTzif};

/// A session or literal time zone.
#[derive(Debug, Clone)]
pub enum Zone {
    /// A fixed displacement, in seconds east of UTC.
    Fixed { east: i32, name: String },
    /// An IANA, raw TZif or POSIX zone with its source retained.
    Rules { tz: RuleZone, name: String },
}

/// A transition zone whose source is inseparable from its Jiff rules.
#[derive(Debug, Clone)]
pub struct RuleZone {
    time_zone: TimeZone,
    source: RuleSource,
}

#[derive(Debug, Clone)]
enum RuleSource {
    Tzif(RawTzif),
    Posix(PosixRule),
    Special,
}

#[derive(Clone, Copy)]
struct RawAbbreviation<'a> {
    designation_index: usize,
    first_type: &'a RawLocalTimeType,
    dynamic: bool,
}

#[derive(Debug, Clone)]
struct PosixRule {
    rule: String,
    standard: PosixType,
    daylight: Option<PosixType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PosixType {
    abbreviation: String,
    offset_seconds_east: i32,
    is_dst: bool,
}

impl RuleZone {
    /// Returns the fixed offset when these rules contain no transitions.
    #[must_use]
    pub fn to_fixed_offset(&self) -> Result<jiff::tz::Offset, jiff::Error> {
        self.time_zone.to_fixed_offset()
    }
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
        Self::special(TimeZone::UTC, "UTC")
    }

    /// Constructs a zone from the exact TZif bytes supplied by the caller.
    /// The bytes, raw local-time types and Jiff zone stay coupled for the
    /// lifetime of the returned value.
    #[must_use]
    pub fn from_tzif(name: impl Into<String>, bytes: &[u8]) -> Result<Self, String> {
        let name = name.into();
        let bytes: Arc<[u8]> = Arc::from(bytes);
        let time_zone = TimeZone::tzif(&name, &bytes).map_err(|error| error.to_string())?;
        let raw = RawTzif::parse(Arc::clone(&bytes)).map_err(str::to_owned)?;
        Ok(Self::Rules {
            tz: RuleZone {
                time_zone,
                source: RuleSource::Tzif(raw),
            },
            name,
        })
    }

    /// Constructs a named IANA zone only from the pinned bundled TZif source.
    #[must_use]
    pub fn from_bundled_iana(name: &str) -> Option<Self> {
        let (canonical_name, bytes) = jiff_tzdb::get(name)?;
        Self::from_tzif(canonical_name, bytes).ok()
    }

    /// Constructs a zone from a POSIX rule while retaining the exact rule and
    /// the explicitly declared standard/daylight abbreviations.
    #[must_use]
    pub fn from_posix(rule: &str) -> Result<Self, String> {
        let time_zone = TimeZone::posix(rule).map_err(|error| error.to_string())?;
        let source = parse_posix_rule(rule)?;
        Ok(Self::Rules {
            tz: RuleZone {
                time_zone,
                source: RuleSource::Posix(source),
            },
            name: rule.to_string(),
        })
    }

    fn special(time_zone: TimeZone, name: &str) -> Self {
        Self::Rules {
            tz: RuleZone {
                time_zone,
                source: RuleSource::Special,
            },
            name: name.to_string(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Fixed { name, .. } => name,
            Self::Rules { tz, name } => match &tz.source {
                RuleSource::Posix(source) => &source.rule,
                RuleSource::Tzif(_) | RuleSource::Special => name,
            },
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
                let info = tz.time_zone.to_offset_info(jiff_timestamp(utc_micros));
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
            Self::Rules { tz, .. } => tz
                .time_zone
                .to_offset(jiff_timestamp(utc_micros))
                .seconds(),
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
                match tz.time_zone.to_ambiguous_timestamp(datetime).offset() {
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

    /// Reports whether a known abbreviation varies by raw source type.
    /// `None` means the abbreviation is not present in this zone.
    pub(crate) fn abbreviation_is_dynamic_with_checks<E>(
        &self,
        abbreviation: &str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<bool>, E> {
        check()?;
        let Self::Rules { tz, .. } = self else {
            return Ok(None);
        };
        match &tz.source {
            RuleSource::Tzif(raw) => Ok(classify_raw_abbreviation_with_checks(
                raw,
                abbreviation,
                check,
            )?
            .map(|classification| classification.dynamic)),
            RuleSource::Posix(posix) => Ok(posix
                .abbreviation_offset(abbreviation)
                .map(|_| false)),
            RuleSource::Special => {
                let info = tz.time_zone.to_offset_info(jiff_timestamp(0));
                Ok(info
                    .abbreviation()
                    .eq_ignore_ascii_case(abbreviation)
                    .then_some(false))
            }
        }
    }

    /// Resolves the most recent matching historical abbreviation, then future history.
    pub(crate) fn abbreviation_offset_with_checks<E>(
        &self,
        abbreviation: &str,
        utc_micros: i64,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<(i32, bool)>, E> {
        check()?;
        let Self::Rules { tz, .. } = self else {
            return Ok(None);
        };
        match &tz.source {
            RuleSource::Tzif(raw) => {
                let Some(classification) =
                    classify_raw_abbreviation_with_checks(raw, abbreviation, check)?
                else {
                    return Ok(None);
                };
                if !classification.dynamic {
                    return Ok(Some((
                        classification.first_type.offset_seconds_east,
                        classification.first_type.is_dst,
                    )));
                }
                checked_dynamic_abbreviation_offset(
                    &tz.time_zone,
                    raw,
                    classification.designation_index,
                    abbreviation,
                    utc_micros,
                    check,
                )
            }
            RuleSource::Posix(posix) => Ok(posix.abbreviation_offset(abbreviation)),
            RuleSource::Special => {
                let info = tz.time_zone.to_offset_info(jiff_timestamp(utc_micros));
                Ok(info
                    .abbreviation()
                    .eq_ignore_ascii_case(abbreviation)
                    .then_some((info.offset().seconds(), info.dst().is_dst())))
            }
        }
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
        match &tz.source {
            RuleSource::Tzif(raw) => {
                let classification = classify_raw_abbreviation(raw, abbreviation)?;
                if classification.dynamic {
                    bounded_dynamic_abbreviation_offset(
                        &tz.time_zone,
                        raw,
                        classification.designation_index,
                        abbreviation,
                        local_micros,
                    )
                } else {
                    Some((
                        classification.first_type.offset_seconds_east,
                        classification.first_type.is_dst,
                    ))
                }
            }
            RuleSource::Posix(posix) => posix.abbreviation_offset(abbreviation),
            RuleSource::Special => {
                let info = tz.time_zone.to_offset_info(jiff_timestamp(local_micros));
                info.abbreviation()
                    .eq_ignore_ascii_case(abbreviation)
                    .then_some((info.offset().seconds(), info.dst().is_dst()))
            }
        }
    }
}

fn classify_raw_abbreviation<'a>(raw: &'a RawTzif, abbreviation: &str) -> Option<RawAbbreviation<'a>> {
    let designation_index = raw.first_matching_designation_index(abbreviation)?;
    let first_type = raw
        .local_time_types
        .iter()
        .find(|local_type| local_type.designation_index == designation_index)?;
    let dynamic = raw.local_time_types.iter().any(|local_type| {
        local_type.designation_index == designation_index
            && (local_type.offset_seconds_east, local_type.is_dst)
                != (first_type.offset_seconds_east, first_type.is_dst)
    });
    Some(RawAbbreviation {
        designation_index,
        first_type,
        dynamic,
    })
}

fn classify_raw_abbreviation_with_checks<'a, E>(
    raw: &'a RawTzif,
    abbreviation: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<RawAbbreviation<'a>>, E> {
    let Some(designation_index) = raw
        .first_matching_designation_index_with_checks(abbreviation, check)?
    else {
        return Ok(None);
    };
    let Some(first_type) = raw
        .local_time_types
        .iter()
        .find(|local_type| local_type.designation_index == designation_index)
    else {
        return Ok(None);
    };
    let mut dynamic = false;
    for (index, local_type) in raw.local_time_types.iter().enumerate() {
        if index % 128 == 127 {
            check()?;
        }
        if local_type.designation_index == designation_index
            && (local_type.offset_seconds_east, local_type.is_dst)
                != (first_type.offset_seconds_east, first_type.is_dst)
        {
            dynamic = true;
            break;
        }
    }
    Ok(Some(RawAbbreviation {
        designation_index,
        first_type,
        dynamic,
    }))
}

fn raw_type_has_offset(
    raw: &RawTzif,
    designation_index: usize,
    offset_seconds_east: i32,
    is_dst: bool,
) -> bool {
    raw.local_time_types.iter().any(|local_type| {
        local_type.designation_index == designation_index
            && local_type.offset_seconds_east == offset_seconds_east
            && local_type.is_dst == is_dst
    })
}

fn checked_dynamic_abbreviation_offset<E>(
    tz: &TimeZone,
    raw: &RawTzif,
    designation_index: usize,
    abbreviation: &str,
    utc_micros: i64,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<(i32, bool)>, E> {
    let instant = jiff_timestamp(utc_micros);
    let info = tz.to_offset_info(instant);
    if info.abbreviation().eq_ignore_ascii_case(abbreviation)
        && raw_type_has_offset(
            raw,
            designation_index,
            info.offset().seconds(),
            info.dst().is_dst(),
        )
    {
        return Ok(Some((info.offset().seconds(), info.dst().is_dst())));
    }
    for (index, transition) in tz.preceding(instant).enumerate() {
        if index % 128 == 127 {
            check()?;
        }
        let offset = transition.offset().seconds();
        let is_dst = transition.dst().is_dst();
        if transition.abbreviation().eq_ignore_ascii_case(abbreviation)
            && raw_type_has_offset(raw, designation_index, offset, is_dst)
        {
            return Ok(Some((offset, is_dst)));
        }
    }
    let info = tz.to_offset_info(jiff::Timestamp::MIN);
    if info.abbreviation().eq_ignore_ascii_case(abbreviation)
        && raw_type_has_offset(
            raw,
            designation_index,
            info.offset().seconds(),
            info.dst().is_dst(),
        )
    {
        return Ok(Some((info.offset().seconds(), info.dst().is_dst())));
    }
    for (index, transition) in tz.following(instant).enumerate() {
        if index % 128 == 127 {
            check()?;
        }
        let offset = transition.offset().seconds();
        let is_dst = transition.dst().is_dst();
        if transition.abbreviation().eq_ignore_ascii_case(abbreviation)
            && raw_type_has_offset(raw, designation_index, offset, is_dst)
        {
            return Ok(Some((offset, is_dst)));
        }
    }
    Ok(None)
}

fn bounded_dynamic_abbreviation_offset(
    tz: &TimeZone,
    raw: &RawTzif,
    designation_index: usize,
    abbreviation: &str,
    utc_micros: i64,
) -> Option<(i32, bool)> {
    let instant = jiff_timestamp(utc_micros);
    let info = tz.to_offset_info(instant);
    let current = (info.offset().seconds(), info.dst().is_dst());
    if info.abbreviation().eq_ignore_ascii_case(abbreviation)
        && raw_type_has_offset(raw, designation_index, current.0, current.1)
    {
        return Some(current);
    }
    tz.preceding(instant)
        .take(64)
        .chain(tz.following(instant).take(64))
        .find_map(|transition| {
            let offset = transition.offset().seconds();
            let is_dst = transition.dst().is_dst();
            (transition.abbreviation().eq_ignore_ascii_case(abbreviation)
                && raw_type_has_offset(raw, designation_index, offset, is_dst))
            .then_some((offset, is_dst))
        })
}

impl PosixRule {
    fn abbreviation_offset(&self, abbreviation: &str) -> Option<(i32, bool)> {
        if self
            .standard
            .abbreviation
            .eq_ignore_ascii_case(abbreviation)
        {
            return Some((
                self.standard.offset_seconds_east,
                self.standard.is_dst,
            ));
        }
        self.daylight
            .as_ref()
            .filter(|posix_type| posix_type.abbreviation.eq_ignore_ascii_case(abbreviation))
            .map(|posix_type| (posix_type.offset_seconds_east, posix_type.is_dst))
    }
}

fn parse_posix_rule(rule: &str) -> Result<PosixRule, String> {
    let bytes = rule.as_bytes();
    let mut cursor = 0;
    let standard_abbreviation = parse_posix_name(rule, &mut cursor)?;
    let standard_offset = parse_posix_offset(bytes, &mut cursor)?;
    let daylight = if cursor == bytes.len() {
        None
    } else {
        let abbreviation = parse_posix_name(rule, &mut cursor)?;
        let offset_seconds_east = if cursor == bytes.len() || bytes[cursor] == b',' {
            standard_offset
                .checked_add(3_600)
                .ok_or_else(|| "POSIX daylight offset is out of range".to_string())?
        } else {
            parse_posix_offset(bytes, &mut cursor)?
        };
        if cursor < bytes.len() && bytes[cursor] != b',' {
            return Err("invalid POSIX rule suffix".to_string());
        }
        Some(PosixType {
            abbreviation,
            offset_seconds_east,
            is_dst: true,
        })
    };
    Ok(PosixRule {
        rule: rule.to_string(),
        standard: PosixType {
            abbreviation: standard_abbreviation,
            offset_seconds_east: standard_offset,
            is_dst: false,
        },
        daylight,
    })
}

fn parse_posix_name(rule: &str, cursor: &mut usize) -> Result<String, String> {
    let bytes = rule.as_bytes();
    let start = *cursor;
    if bytes.get(*cursor) == Some(&b'<') {
        *cursor += 1;
        let name_start = *cursor;
        let end = bytes
            .get(name_start..)
            .and_then(|suffix| suffix.iter().position(|&byte| byte == b'>'))
            .map(|offset| name_start + offset)
            .ok_or_else(|| "unterminated POSIX abbreviation".to_string())?;
        if end == name_start {
            return Err("empty POSIX abbreviation".to_string());
        }
        let name = rule
            .get(name_start..end)
            .ok_or_else(|| "invalid POSIX abbreviation".to_string())?;
        *cursor = end + 1;
        return Ok(name.to_string());
    }
    while bytes.get(*cursor).is_some_and(u8::is_ascii_alphabetic) {
        *cursor += 1;
    }
    if *cursor - start < 3 {
        return Err("POSIX abbreviations must contain at least three letters".to_string());
    }
    Ok(rule[start..*cursor].to_string())
}

fn parse_posix_offset(bytes: &[u8], cursor: &mut usize) -> Result<i32, String> {
    let sign = match bytes.get(*cursor) {
        Some(b'+') => {
            *cursor += 1;
            1_i64
        }
        Some(b'-') => {
            *cursor += 1;
            -1_i64
        }
        _ => 1_i64,
    };
    let hours = parse_decimal(bytes, cursor)?;
    let minutes = if bytes.get(*cursor) == Some(&b':') {
        *cursor += 1;
        parse_decimal(bytes, cursor)?
    } else {
        0
    };
    let seconds = if bytes.get(*cursor) == Some(&b':') {
        *cursor += 1;
        parse_decimal(bytes, cursor)?
    } else {
        0
    };
    if minutes > 59 || seconds > 59 {
        return Err("POSIX offset minute or second is out of range".to_string());
    }
    let minute_seconds = minutes
        .checked_mul(60)
        .ok_or_else(|| "POSIX offset is out of range".to_string())?;
    let west = hours
        .checked_mul(3_600)
        .and_then(|value| value.checked_add(minute_seconds))
        .and_then(|value| value.checked_add(seconds))
        .and_then(|value| value.checked_mul(sign))
        .ok_or_else(|| "POSIX offset is out of range".to_string())?;
    i32::try_from(-west).map_err(|_| "POSIX offset is out of range".to_string())
}

fn parse_decimal(bytes: &[u8], cursor: &mut usize) -> Result<i64, String> {
    let start = *cursor;
    let mut value = 0_i64;
    while let Some(byte) = bytes.get(*cursor).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(i64::from(**byte - b'0')))
            .ok_or_else(|| "POSIX numeric field is out of range".to_string())?;
        *cursor += 1;
    }
    if *cursor == start {
        return Err("missing POSIX numeric field".to_string());
    }
    Ok(value)
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
    lookup_zone_with_origin(name).map(|(zone, _)| zone)
}

pub(crate) fn lookup_zone_with_origin(name: &str) -> Option<(Zone, bool)> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("UTC") {
        return Some((Zone::utc(), false));
    }
    if trimmed.eq_ignore_ascii_case("Etc/Unknown") {
        return Some((Zone::special(TimeZone::unknown(), "Etc/Unknown"), false));
    }
    if let Some(zone) = Zone::from_bundled_iana(trimmed) {
        return Some((zone, false));
    }
    let posix_like =
        trimmed.starts_with('<') || trimmed.chars().next().is_some_and(char::is_alphabetic);
    if posix_like && trimmed.chars().any(|c| c.is_ascii_digit()) {
        return Zone::from_posix(trimmed).ok().map(|zone| (zone, true));
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
#[path = "zone_source_tests.rs"]
mod source_tests;

#[cfg(test)]
mod tests {
    use super::{lookup_zone, offset_abbreviation, AbbreviationSet};
    use crate::types::extended::temporal::calendar::{fields_to_timestamp, Fields};

    #[test]
    fn should_interrupt_a_long_formatted_timezone_abbreviation_search() {
        // Arrange
        let zone = lookup_zone("Europe/London").expect("London timezone");
        let mut checks = 0;
        let mut check = || {
            checks += 1;
            if checks == 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        };

        // Act
        let result = zone.abbreviation_offset_with_checks(
            "not-a-timezone-abbreviation",
            local(2400, 1, 1, 0, 0),
            &mut check,
        );

        // Assert
        assert_eq!(result, Err("cancelled"));
        assert_eq!(checks, 2);
    }

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
    fn should_resolve_ambiguous_local_times_like_postgresql() {
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
