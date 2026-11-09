//! `to_char`, `to_date` and `to_timestamp` templates (`formatting.c`).
use super::calendar::{
    date2j, days_in_month, iso_week, iso_week_to_julian, iso_year, iso_year_day, j2date, j2day,
    Fields, USECS_PER_SEC,
};
use crate::types::extended::TypeInputError;

#[path = "format_zone_catalog.rs"]
mod catalog;
#[path = "format_clock.rs"]
mod clock;
#[path = "format_fraction.rs"]
mod fraction;
#[path = "format_interval.rs"]
mod interval;
#[path = "format_zone.rs"]
mod zone;
use super::zone::AbbreviationSet;
pub(crate) use catalog::{ZoneAbbreviation, ZoneRule};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const ROMAN: [&str; 12] = [
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
];

/// Template keywords, longest spellings first so prefixes never shadow them.
const KEYWORDS: &[&str] = &[
    "a.d.", "a.m.", "b.c.", "p.m.", "y,yyy", "hh24", "hh12", "iddd", "iyyy", "month", "sssss",
    "ssss", "yyyy", "day", "ddd", "ff1", "ff2", "ff3", "ff4", "ff5", "ff6", "iyy", "mon", "tzh",
    "tzm", "yyy", "ad", "am", "bc", "cc", "dd", "dy", "fx", "hh", "id", "iw", "iy", "mi", "mm",
    "ms", "of", "pm", "rm", "ss", "tz", "us", "ww", "yy", "d", "i", "j", "q", "w", "y",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Literal(String),
    Field {
        keyword: &'static str,
        /// The template's own spelling, for letter-case output rules.
        spelling: String,
        fill: bool,
        ordinal: Option<bool>,
    },
}

fn parse_template(template: &str) -> Vec<Node> {
    let chars: Vec<char> = template.chars().collect();
    let mut nodes = Vec::new();
    let mut index = 0;
    let mut fill = false;
    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        let lower = rest.to_ascii_lowercase();
        if lower.starts_with("fm") {
            fill = true;
            index += 2;
            continue;
        }
        if lower.starts_with("tm") {
            index += 2;
            continue;
        }
        if chars[index] == '"' {
            let mut literal = String::new();
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if chars[index] == '\\' && index + 1 < chars.len() {
                    index += 1;
                }
                literal.push(chars[index]);
                index += 1;
            }
            index += 1;
            nodes.push(Node::Literal(literal));
            continue;
        }
        if chars[index] == '\\' && index + 1 < chars.len() {
            nodes.push(Node::Literal(chars[index + 1].to_string()));
            index += 2;
            continue;
        }
        if let Some(keyword) = KEYWORDS.iter().find(|keyword| lower.starts_with(**keyword)) {
            let length = keyword.chars().count();
            let spelling: String = chars[index..index + length].iter().collect();
            index += length;
            let suffix: String = chars[index..chars.len().min(index + 2)].iter().collect();
            let ordinal = match suffix.as_str() {
                "TH" => Some(true),
                "th" => Some(false),
                _ => None,
            };
            if ordinal.is_some() {
                index += 2;
            }
            nodes.push(Node::Field {
                keyword,
                spelling,
                fill,
                ordinal,
            });
            fill = false;
            continue;
        }
        nodes.push(Node::Literal(chars[index].to_string()));
        index += 1;
    }
    nodes
}

/// The values `to_char` renders.
#[derive(Debug, Clone, Copy)]
pub struct Moment {
    pub fields: Fields,
    /// Seconds east and abbreviation availability for `timestamptz`.
    pub zone_east: Option<i32>,
    pub is_interval: bool,
}

fn case_like(spelling: &str, word: &str) -> String {
    let mut letters = spelling.chars().filter(char::is_ascii_alphabetic);
    let first = letters.next().is_some_and(|c| c.is_ascii_uppercase());
    let second = letters.next().is_some_and(|c| c.is_ascii_uppercase());
    if first && second {
        word.to_ascii_uppercase()
    } else if first {
        word.to_string()
    } else {
        word.to_ascii_lowercase()
    }
}

fn ordinal_suffix(value: i64, upper: bool) -> &'static str {
    let value = value.unsigned_abs();
    let suffix = if (11..=13).contains(&(value % 100)) {
        "th"
    } else {
        match value % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    match (upper, suffix) {
        (false, suffix) => suffix,
        (true, "st") => "ST",
        (true, "nd") => "ND",
        (true, "rd") => "RD",
        (true, _) => "TH",
    }
}

fn number(value: i64, width: usize, fill: bool) -> String {
    if fill {
        value.to_string()
    } else if value < 0 {
        format!("-{:0width$}", value.unsigned_abs())
    } else {
        format!("{value:0width$}")
    }
}

fn era_year(year: i32, interval: bool) -> i64 {
    if interval || year > 0 {
        i64::from(year)
    } else {
        -(i64::from(year) - 1)
    }
}

/// `to_char` for a timestamp, timestamptz, date or interval.
#[must_use]
pub fn to_char(moment: &Moment, template: &str, zone_name: &str) -> String {
    to_char_with_hour(moment, template, zone_name, i64::from(moment.fields.hour))
}

pub(crate) fn to_char_with_hour(
    moment: &Moment,
    template: &str,
    zone_name: &str,
    hour: i64,
) -> String {
    let mut out = String::new();
    for node in parse_template(template) {
        match node {
            Node::Literal(text) => out.push_str(&text),
            Node::Field {
                keyword,
                spelling,
                fill,
                ordinal,
            } => {
                let (text, value) = render_field(moment, keyword, &spelling, fill, zone_name, hour);
                out.push_str(&text);
                if let (Some(upper), Some(value)) = (ordinal, value) {
                    out.push_str(ordinal_suffix(value, upper));
                }
            }
        }
    }
    out
}

fn padded(text: &str, fill: bool, width: usize) -> String {
    if fill {
        text.to_string()
    } else {
        format!("{text:<width$}")
    }
}

fn render_name_field(
    moment: &Moment,
    keyword: &str,
    spelling: &str,
    fill: bool,
    zone_name: &str,
) -> Option<(String, Option<i64>)> {
    let fields = moment.fields;
    Some(match keyword {
        "month" | "mon" => {
            let name = MONTHS[usize::try_from(fields.month - 1).unwrap_or(0).min(11)];
            if keyword == "mon" {
                (case_like(spelling, &name[..3]), None)
            } else {
                (padded(&case_like(spelling, name), fill, 9), None)
            }
        }
        "rm" if moment.is_interval && fields.year == 0 && fields.month == 0 => {
            (String::new(), None)
        }
        "rm" => {
            let index = if moment.is_interval {
                interval::roman_month(fields)
            } else {
                usize::try_from(fields.month - 1).unwrap_or(0).min(11)
            };
            let roman = ROMAN[index];
            (padded(&case_like(spelling, roman), fill, 4), None)
        }
        "tz" => (
            moment
                .zone_east
                .map(|_| case_like(spelling, zone_name))
                .unwrap_or_default(),
            None,
        ),
        "of" => (
            moment
                .zone_east
                .map(|east| zone::format_offset(east, fill))
                .unwrap_or_default(),
            None,
        ),
        "tzh" => (
            moment
                .zone_east
                .map(|east| {
                    format!(
                        "{}{:02}",
                        if east < 0 { '-' } else { '+' },
                        east.unsigned_abs() / 3600
                    )
                })
                .unwrap_or_default(),
            None,
        ),
        "tzm" => (
            moment
                .zone_east
                .map(|east| format!("{:02}", east.unsigned_abs() / 60 % 60))
                .unwrap_or_default(),
            None,
        ),
        _ => return None,
    })
}

fn render_field(
    moment: &Moment,
    keyword: &str,
    spelling: &str,
    fill: bool,
    zone_name: &str,
    hour: i64,
) -> (String, Option<i64>) {
    if let Some(rendered) = clock::render_clock_field(moment, keyword, spelling, fill, hour)
        .or_else(|| render_name_field(moment, keyword, spelling, fill, zone_name))
    {
        return rendered;
    }
    let fields = moment.fields;
    let year = era_year(fields.year, moment.is_interval);
    let numeric = |value: i64, width: usize| (number(value, width, fill), Some(value));
    let julian = || date2j(fields.year, fields.month, fields.day);
    match keyword {
        "ad" | "bc" | "a.d." | "b.c." => {
            let bc = fields.year <= 0;
            let text = match (keyword.len() > 2, bc) {
                (false, false) => "ad",
                (false, true) => "bc",
                (true, false) => "a.d.",
                (true, true) => "b.c.",
            };
            (case_like(spelling, text), None)
        }
        "y,yyy" => (format!("{},{:03}", year / 1000, year % 1000), Some(year)),
        "yyyy" => numeric(year, 4),
        "yyy" => numeric(year % 1000, 3),
        "yy" => numeric(year % 100, 2),
        "y" => numeric(year % 10, 1),
        "iyyy" | "iyy" | "iy" | "i" => {
            let iso = i64::from(iso_year(fields.year, fields.month, fields.day));
            let iso = if iso > 0 { iso } else { -(iso - 1) };
            match keyword {
                "iyyy" => numeric(iso, 4),
                "iyy" => numeric(iso % 1000, 3),
                "iy" => numeric(iso % 100, 2),
                _ => numeric(iso % 10, 1),
            }
        }
        "mm" => numeric(i64::from(fields.month), 2),
        "day" | "dy" => {
            let name = DAYS[usize::try_from(j2day(julian())).unwrap_or(0)];
            if keyword == "dy" {
                (case_like(spelling, &name[..3]), None)
            } else {
                (padded(&case_like(spelling, name), fill, 9), None)
            }
        }
        "ddd" if moment.is_interval => interval::span_field(fields, false, fill),
        "ddd" => numeric(julian() - date2j(fields.year, 1, 1) + 1, 3),
        "iddd" => numeric(
            i64::from(iso_year_day(fields.year, fields.month, fields.day)),
            3,
        ),
        "dd" if moment.is_interval && fields.day < 0 && !fill => {
            (format!("{:02}", fields.day), Some(i64::from(fields.day)))
        }
        "dd" => numeric(i64::from(fields.day), 2),
        "d" => numeric(i64::from(j2day(julian())) + 1, 1),
        "id" => {
            let day = i64::from(j2day(julian()));
            numeric(if day == 0 { 7 } else { day }, 1)
        }
        "w" if moment.is_interval => numeric(i64::from(fields.day.wrapping_sub(1) / 7 + 1), 1),
        "w" => numeric(i64::from((fields.day - 1) / 7 + 1), 1),
        "ww" if moment.is_interval => interval::span_field(fields, true, fill),
        "ww" => numeric((julian() - date2j(fields.year, 1, 1)) / 7 + 1, 2),
        "iw" => numeric(
            i64::from(iso_week(fields.year, fields.month, fields.day)),
            2,
        ),
        "q" if moment.is_interval && fields.month == 0 => (String::new(), None),
        "q" => numeric(i64::from((fields.month - 1) / 3 + 1), 1),
        "cc" => {
            let century = if moment.is_interval {
                i64::from(fields.year) / 100
            } else if fields.year > 0 {
                (i64::from(fields.year) + 99) / 100
            } else {
                -((99 - (i64::from(fields.year) - 1)) / 100)
            };
            numeric(century, 2)
        }
        "j" => numeric(julian(), 0),
        _ => (String::new(), None),
    }
}

/// Fields parsed by `to_date`/`to_timestamp`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Parsed {
    pub fields: Fields,
    /// Explicit `OF`/`TZH:TZM` displacement, seconds east.
    pub zone_east: Option<i32>,
}

fn invalid_value(text: &str, keyword: &str) -> TypeInputError {
    TypeInputError {
        sqlstate: "22007",
        message: format!(
            "invalid value \"{text}\" for \"{}\"",
            keyword.to_ascii_uppercase()
        ),
    }
}

fn field_overflow(message: impl Into<String>) -> TypeInputError {
    TypeInputError {
        sqlstate: "22008",
        message: message.into(),
    }
}

const fn width(keyword: &str) -> usize {
    match keyword.as_bytes() {
        b"yyyy" | b"iyyy" => 4,
        b"ddd" | b"iddd" | b"ms" | b"yyy" | b"iyy" => 3,
        b"us" => 6,
        b"y" | b"i" | b"d" | b"id" | b"q" | b"w" => 1,
        b"j" | b"ssss" | b"sssss" => 9,
        _ => 2,
    }
}

#[derive(Default)]
struct Builder {
    year: Option<i32>,
    month: Option<i32>,
    day: Option<i32>,
    year_day: Option<i32>,
    iso_year: Option<i32>,
    iso_week: Option<i32>,
    iso_day: Option<i32>,
    julian: Option<i64>,
    hour: i32,
    minute: i32,
    second: i32,
    micros: i64,
    pm: Option<bool>,
    twelve_hour: bool,
    bc: bool,
    zone: zone::ZoneFields,
    ff_precision: Option<u8>,
}

/// Parses `input` against a `to_date`/`to_timestamp` template.
///
/// # Errors
/// Returns `22007` for unmatched input and `22008` for out-of-range fields.
pub fn parse(input: &str, template: &str) -> Result<Parsed, TypeInputError> {
    let (mut parsed, _, abbreviation) =
        parse_with_precision(input, template, AbbreviationSet::Default)?;
    if let Some(abbreviation) = abbreviation {
        parsed.zone_east = match abbreviation.rule {
            ZoneRule::Fixed(east) => Some(east),
            ZoneRule::Dynamic(name) => {
                let local = super::calendar::fields_to_timestamp(&parsed.fields, None)
                    .ok_or_else(|| field_overflow("timestamp out of range"))?;
                let zone =
                    super::zone::lookup_zone(name).ok_or_else(|| invalid_value(name, "tz"))?;
                let offset = zone.offset_for_local(local);
                let instant = local.saturating_sub(i64::from(offset) * USECS_PER_SEC);
                let found =
                    zone.abbreviation_offset_with_checks(abbreviation.name, instant, &mut || {
                        Ok::<(), std::convert::Infallible>(())
                    });
                Some(
                    found
                        .unwrap_or_else(|never| match never {})
                        .map_or(offset, |(east, _)| east),
                )
            }
        };
    }
    Ok(parsed)
}

/// Internal precision metadata leaves the public parsed-fields API unchanged.
pub(crate) fn parse_with_precision(
    input: &str,
    template: &str,
    abbreviations: AbbreviationSet,
) -> Result<(Parsed, Option<u8>, Option<ZoneAbbreviation>), TypeInputError> {
    let nodes = parse_template(template);
    let chars: Vec<char> = input.chars().collect();
    let mut cursor = 0;
    let mut builder = Builder::default();
    for (index, node) in nodes.iter().enumerate() {
        match node {
            Node::Literal(text) => {
                let zone_follows = matches!(
                    nodes[index + 1..].iter().find(|node| {
                        !matches!(node, Node::Literal(text) if text.chars().all(char::is_whitespace))
                    }),
                    Some(Node::Field { keyword: "tzh" | "of" | "tz", .. })
                );
                for expected in text.chars() {
                    if expected.is_whitespace() || expected.is_ascii_punctuation() {
                        while chars.get(cursor).is_some_and(|c| c.is_whitespace()) {
                            cursor += 1;
                        }
                        if chars
                            .get(cursor)
                            .is_some_and(|c| !c.is_alphanumeric() && !c.is_whitespace())
                            && !(expected.is_whitespace()
                                && zone_follows
                                && matches!(chars.get(cursor), Some('+' | '-')))
                        {
                            cursor += 1;
                        }
                    } else if cursor < chars.len() {
                        cursor += 1;
                    }
                }
            }
            Node::Field { keyword, .. } => {
                while chars.get(cursor).is_some_and(|c| c.is_whitespace()) {
                    cursor += 1;
                }
                parse_field(
                    keyword,
                    &chars,
                    &mut cursor,
                    nodes.get(index + 1),
                    &mut builder,
                    abbreviations,
                )?;
            }
        }
    }
    let precision = builder.ff_precision;
    let abbreviation = builder.zone.abbreviation();
    builder
        .finish()
        .map(|parsed| (parsed, precision, abbreviation))
}

fn read_digits(
    chars: &[char],
    cursor: &mut usize,
    limit: usize,
    keyword: &str,
) -> Result<(i64, usize), TypeInputError> {
    let start = *cursor;
    let negative = chars.get(*cursor) == Some(&'-');
    if negative || chars.get(*cursor) == Some(&'+') {
        *cursor += 1;
    }
    let digits_start = *cursor;
    while *cursor - digits_start < limit && chars.get(*cursor).is_some_and(char::is_ascii_digit) {
        *cursor += 1;
    }
    let digits: String = chars[digits_start..*cursor].iter().collect();
    if digits.is_empty() {
        let shown: String = chars[start..chars.len().min(start + width(keyword).max(4))]
            .iter()
            .collect();
        return Err(invalid_value(&shown, keyword));
    }
    let value = digits
        .parse::<i64>()
        .map_err(|_| field_overflow("value for field is out of range"))?;
    Ok((if negative { -value } else { value }, digits.len()))
}

fn read_name(
    chars: &[char],
    cursor: &mut usize,
    names: &[&str],
    abbreviated: bool,
    keyword: &str,
) -> Result<usize, TypeInputError> {
    let rest: String = chars[*cursor..]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    for (index, name) in names.iter().enumerate() {
        let candidate = if abbreviated { &name[..3] } else { name };
        if rest.starts_with(&candidate.to_ascii_lowercase()) {
            *cursor += candidate.len();
            return Ok(index);
        }
    }
    let shown: String = chars[*cursor..].iter().take(9).collect();
    Err(invalid_value(&shown, keyword))
}

fn narrow(value: i64) -> Result<i32, TypeInputError> {
    i32::try_from(value).map_err(|_| field_overflow("value for field is out of range"))
}

/// Zone displacement and fractional-second template fields.
fn parse_zone_or_fraction(
    keyword: &str,
    chars: &[char],
    cursor: &mut usize,
    adjacent: bool,
    builder: &mut Builder,
) -> Result<bool, TypeInputError> {
    {
        match keyword {
            "of" | "tzh" => {
                let negative = chars.get(*cursor) == Some(&'-');
                let (hours, _) = read_digits(chars, cursor, 2, keyword)?;
                builder.zone.set_hours(hours.abs(), negative)?;
                if keyword == "of" && chars.get(*cursor) == Some(&':') {
                    *cursor += 1;
                    let (minutes, _) = read_digits(chars, cursor, 2, keyword)?;
                    builder.zone.set_minutes(minutes)?;
                }
            }
            "tzm" => {
                let (minutes, _) = read_digits(chars, cursor, 2, keyword)?;
                builder.zone.set_minutes(minutes)?;
            }
            "us" | "ms" | "ff1" | "ff2" | "ff3" | "ff4" | "ff5" | "ff6" => {
                let digits_limit = match keyword {
                    "ms" => 3,
                    "us" => 6,
                    other => usize::from(other.as_bytes()[2] - b'0'),
                };
                if keyword.starts_with("ff") {
                    builder.ff_precision = Some(keyword.as_bytes()[2] - b'0');
                }
                let limit = if !adjacent { chars.len() } else { digits_limit };
                let (value, length) = read_digits(chars, cursor, limit, keyword)?;
                let maximum = if keyword == "ms" { 1000 } else { USECS_PER_SEC };
                if !(0..maximum).contains(&value) {
                    return Err(field_overflow("date/time field value out of range"));
                }
                let scale = if keyword == "ms" { 1000 } else { 1 };
                let precision = if keyword == "ms" { 3_usize } else { 6 };
                let fraction_scale =
                    10_i64.pow(u32::try_from(precision.saturating_sub(length)).unwrap_or(0));
                builder.micros = value * scale * fraction_scale;
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn parse_field(
    keyword: &str,
    chars: &[char],
    cursor: &mut usize,
    next: Option<&Node>,
    builder: &mut Builder,
    abbreviations: AbbreviationSet,
) -> Result<(), TypeInputError> {
    let fraction_adjacent = match next {
        Some(Node::Field { keyword, .. }) => !matches!(
            *keyword,
            "am" | "pm"
                | "a.m."
                | "p.m."
                | "ad"
                | "bc"
                | "a.d."
                | "b.c."
                | "month"
                | "mon"
                | "day"
                | "dy"
                | "tz"
                | "tzh"
                | "of"
        ),
        Some(Node::Literal(text)) => text.len() == 1 && text.as_bytes()[0].is_ascii_digit(),
        None => false,
    };
    if parse_zone_or_fraction(keyword, chars, cursor, fraction_adjacent, builder)? {
        return Ok(());
    }
    let adjacent = matches!(next, Some(Node::Field { .. }));
    let limit = if adjacent { width(keyword) } else { 9 };
    match keyword {
        "month" | "mon" => {
            builder.month = Some(
                narrow(
                    i64::try_from(read_name(
                        chars,
                        cursor,
                        &MONTHS,
                        keyword == "mon",
                        keyword,
                    )?)
                    .unwrap_or(0),
                )? + 1,
            );
        }
        "day" | "dy" => {
            read_name(chars, cursor, &DAYS, keyword == "dy", keyword)?;
        }
        "am" | "pm" | "a.m." | "p.m." => {
            let rest: String = chars[*cursor..]
                .iter()
                .take(4)
                .collect::<String>()
                .to_ascii_lowercase();
            let (pm, length) = if rest.starts_with("pm") {
                (true, 2)
            } else if rest.starts_with("am") {
                (false, 2)
            } else if rest.starts_with("p.m.") {
                (true, 4)
            } else if rest.starts_with("a.m.") {
                (false, 4)
            } else {
                return Err(invalid_value(&rest, keyword));
            };
            builder.pm = Some(pm);
            *cursor += length;
        }
        "ad" | "bc" | "a.d." | "b.c." => {
            let rest: String = chars[*cursor..]
                .iter()
                .take(4)
                .collect::<String>()
                .to_ascii_lowercase();
            builder.bc = rest.starts_with("bc") || rest.starts_with("b.c.");
            *cursor += if rest.contains('.') { 4 } else { 2 };
        }
        "tz" => {
            if chars.get(*cursor).is_some_and(|c| c.is_ascii_alphabetic()) {
                let (abbreviation, length) =
                    catalog::find(chars.get(*cursor..).unwrap_or_default(), abbreviations)
                        .ok_or_else(|| {
                            invalid_value(
                                &chars
                                    .get(*cursor..)
                                    .unwrap_or_default()
                                    .iter()
                                    .take(16)
                                    .collect::<String>(),
                                keyword,
                            )
                        })?;
                builder.zone.set_abbreviation(abbreviation);
                *cursor += length;
            } else {
                parse_zone_or_fraction("of", chars, cursor, fraction_adjacent, builder)?;
            }
        }
        "rm" => {
            let rest: String = chars[*cursor..]
                .iter()
                .take(4)
                .collect::<String>()
                .to_ascii_uppercase();
            let index = ROMAN
                .iter()
                .enumerate()
                .rev()
                .find(|(_, roman)| rest.starts_with(**roman))
                .map(|(index, _)| index)
                .ok_or_else(|| invalid_value(&rest, keyword))?;
            *cursor += ROMAN[index].len();
            builder.month = Some(narrow(i64::try_from(index).unwrap_or(0))? + 1);
        }
        _ => numeric_field(keyword, chars, cursor, limit, builder)?,
    }
    Ok(())
}

fn numeric_field(
    keyword: &str,
    chars: &[char],
    cursor: &mut usize,
    limit: usize,
    builder: &mut Builder,
) -> Result<(), TypeInputError> {
    if keyword == "y,yyy" {
        let (thousands, _) = read_digits(chars, cursor, 3, keyword)?;
        if chars.get(*cursor) == Some(&',') {
            *cursor += 1;
        }
        let (rest, _) = read_digits(chars, cursor, 3, keyword)?;
        builder.year = Some(narrow(thousands * 1000 + rest)?);
        return Ok(());
    }
    let (value, length) = read_digits(chars, cursor, limit, keyword)?;
    let value32 = narrow(value);
    match keyword {
        "yyyy" | "yyy" | "yy" | "y" => {
            let mut year = value32?;
            if keyword != "yyyy" && length <= 2 && keyword != "y" {
                year += if year < 70 { 2000 } else { 1900 };
            }
            builder.year = Some(year);
        }
        "iyyy" | "iyy" | "iy" | "i" => builder.iso_year = Some(value32?),
        "mm" => builder.month = Some(value32?),
        "dd" => builder.day = Some(value32?),
        "ddd" | "iddd" => builder.year_day = Some(value32?),
        "iw" => builder.iso_week = Some(value32?),
        "id" => builder.iso_day = Some(value32?),
        "hh" | "hh12" => {
            builder.hour = value32?;
            builder.twelve_hour = true;
        }
        "hh24" => builder.hour = value32?,
        "mi" => builder.minute = value32?,
        "ss" => builder.second = value32?,
        "ssss" | "sssss" => {
            let total = value32?;
            builder.hour = total / 3600;
            builder.minute = total / 60 % 60;
            builder.second = total % 60;
        }
        "j" => builder.julian = Some(value),
        "cc" | "q" | "w" | "ww" | "d" => {}
        _ => return Err(invalid_value(&value.to_string(), keyword)),
    }
    Ok(())
}

impl Builder {
    fn finish(self) -> Result<Parsed, TypeInputError> {
        let mut fields = Fields::default();
        if let Some(julian) = self.julian {
            let (year, month, day) = j2date(julian);
            fields.year = year;
            fields.month = month;
            fields.day = day;
        } else if let (Some(year), Some(week)) = (self.iso_year, self.iso_week) {
            let day = self.iso_day.unwrap_or(1);
            let (y, m, d) = j2date(iso_week_to_julian(year, week) + i64::from(day - 1));
            fields.year = y;
            fields.month = m;
            fields.day = d;
        } else {
            fields.year = self.year.unwrap_or(1);
            if self.bc {
                fields.year = -(fields.year - 1);
            }
            if let Some(year_day) = self.year_day {
                let (y, m, d) = j2date(date2j(fields.year, 1, 1) + i64::from(year_day - 1));
                fields.year = y;
                fields.month = m;
                fields.day = d;
            } else {
                fields.month = self.month.unwrap_or(1);
                fields.day = self.day.unwrap_or(1);
            }
        }
        if !(1..=12).contains(&fields.month) {
            return Err(field_overflow(format!(
                "date/time field value out of range: \"{}\"",
                fields.month
            )));
        }
        if fields.day < 1 || fields.day > days_in_month(fields.year, fields.month) {
            return Err(field_overflow("date/time field value out of range"));
        }
        let mut hour = self.hour;
        if self.twelve_hour || self.pm.is_some() {
            if !(1..=12).contains(&hour) {
                return Err(field_overflow(format!(
                    "hour \"{hour}\" is invalid for the 12-hour clock"
                )));
            }
            match self.pm {
                Some(true) if hour != 12 => hour += 12,
                Some(false) if hour == 12 => hour = 0,
                _ => {}
            }
        }
        if !(0..24).contains(&hour)
            || !(0..60).contains(&self.minute)
            || !(0..60).contains(&self.second)
            || !(0..USECS_PER_SEC).contains(&self.micros)
        {
            return Err(field_overflow("date/time field value out of range"));
        }
        fields.hour = hour;
        fields.minute = self.minute;
        fields.second = self.second;
        fields.micros = self.micros;
        Ok(Parsed {
            fields,
            zone_east: self.zone.offset_east()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, to_char, Moment};
    use crate::types::extended::temporal::calendar::Fields;

    fn moment(
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        micros: i64,
    ) -> Moment {
        Moment {
            fields: Fields {
                year,
                month,
                day,
                hour,
                minute,
                second,
                micros,
            },
            zone_east: None,
            is_interval: false,
        }
    }

    #[test]
    fn should_render_postgresql_templates() {
        // Arrange
        let value = moment(2002, 4, 20, 17, 31, 12, 660_123);

        // Act
        let twelve = to_char(&value, "HH12:MI:SS", "");
        let filled = to_char(&value, "FMMonth DD, YYYY", "");
        let micros = to_char(&value, "YYYY-MM-DD\"T\"HH24:MI:SS.US", "");
        let iso = to_char(&moment(2006, 1, 1, 0, 0, 0, 0), "IYYY-IW-ID", "");

        // Assert
        assert_eq!(twelve, "05:31:12");
        assert_eq!(filled, "April 20, 2002");
        assert_eq!(micros, "2002-04-20T17:31:12.660123");
        assert_eq!(iso, "2005-52-7");
    }

    #[test]
    fn should_parse_formatted_input() {
        // Arrange
        let inputs = [
            ("05 Dec 2000", "DD Mon YYYY"),
            ("05 dec 2000", "DD MON YYYY"),
            ("2006-42-4", "IYYY-IW-ID"),
            ("05 Dec 2000 15:12:02.020001", "DD Mon YYYY HH24:MI:SS.US"),
        ];

        // Act
        let parsed =
            inputs.map(|(input, template)| parse(input, template).map(|parsed| parsed.fields));

        // Assert
        let first = parsed[0].as_ref().expect("date");
        assert_eq!((first.year, first.month, first.day), (2000, 12, 5));
        assert_eq!(parsed[1].as_ref().map(|fields| fields.month).ok(), Some(12));
        let iso = parsed[2].as_ref().expect("iso");
        assert_eq!((iso.year, iso.month, iso.day), (2006, 10, 19));
        assert_eq!(
            parsed[3].as_ref().map(|fields| fields.micros).ok(),
            Some(20_001)
        );
        assert_eq!(
            parse("not-a-date", "YYYY-MM-DD")
                .map_err(|error| error.sqlstate)
                .err(),
            Some("22007")
        );
    }
}
