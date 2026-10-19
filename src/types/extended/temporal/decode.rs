//! `DecodeDateTime` / `DecodeTimeOnly`: PostgreSQL's date and time input.
use super::calendar::{
    date2j, days_in_month, j2date, time_fields, timestamp_to_fields, Fields, USECS_PER_DAY,
};
use super::session::{DateOrder, TemporalSession};
use super::tokens::{
    date_keyword, leading_int, parse_fraction, parse_fractional_second, tokenize, DecodeError,
    Keyword, Meridiem, Reserved, Token, TokenKind, Unit,
};
use super::zone::{lookup_zone, Zone};

#[path = "time_input.rs"]
mod time_input;
pub use time_input::{decode_time_only, DecodedTime};

const YEAR: u32 = 1 << 2;
const MONTH: u32 = 1 << 1;
const DAY: u32 = 1 << 3;
const TZ: u32 = 1 << 5;
const DTZ: u32 = 1 << 6;
const HOUR: u32 = 1 << 10;
const MINUTE: u32 = 1 << 11;
const SECOND: u32 = 1 << 12;
const MILLISECOND: u32 = 1 << 13;
const MICROSECOND: u32 = 1 << 14;
const DOY: u32 = 1 << 15;
const DOW: u32 = 1 << 16;
const AMPM: u32 = 1 << 9;
const ADBC: u32 = 1 << 18;
const DTZMOD: u32 = 1 << 28;
const DATE_M: u32 = YEAR | MONTH | DAY;
const TIME_M: u32 = HOUR | MINUTE | SECOND | MILLISECOND | MICROSECOND;

/// A non-date result of date/time input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    Epoch,
    Late,
    Early,
}

/// Decoded date/time input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub special: Option<Special>,
    pub fields: Fields,
    /// The resolved displacement in seconds west (explicit or session).
    pub zone_west: i32,
}

/// How the year field was written (`isjulian` / `is2digits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum YearForm {
    #[default]
    Plain,
    TwoDigit,
    Julian,
}

/// A field label waiting for its value (`ptype`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    Julian,
    IsoTime,
    Other,
}

#[derive(Default)]
struct State {
    fields: Fields,
    fmask: u32,
    ptype: Option<Prefix>,
    year_form: YearForm,
    bc: bool,
    meridiem: Option<Meridiem>,
    zone_west: Option<i32>,
    named_zone: Option<Zone>,
    year_day: i32,
    have_text_month: bool,
    special: Option<Special>,
}

/// Decodes `timestamp`/`timestamptz`/`date` input.
///
/// # Errors
/// Returns PostgreSQL's decode error class.
pub fn decode_datetime(input: &str, session: &TemporalSession) -> Result<Decoded, DecodeError> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(DecodeError::BadFormat);
    }
    let mut state = State::default();
    for (index, token) in tokens.iter().enumerate() {
        let tmask = decode_datetime_field(&tokens, index, token, &mut state, session)?;
        let Some(tmask) = tmask else { continue };
        if tmask & state.fmask != 0 {
            return Err(DecodeError::BadFormat);
        }
        state.fmask |= tmask;
    }
    if state.ptype.is_some() {
        return Err(DecodeError::BadFormat);
    }
    if let Some(special) = state.special {
        return Ok(Decoded {
            special: Some(special),
            fields: state.fields,
            zone_west: 0,
        });
    }
    validate_date(&mut state)?;
    apply_meridiem(&mut state)?;
    if state.fmask & DATE_M != DATE_M {
        return Err(DecodeError::BadFormat);
    }
    let zone_west = resolve_zone(&state, session)?;
    Ok(Decoded {
        special: None,
        fields: state.fields,
        zone_west,
    })
}

fn resolve_zone(state: &State, session: &TemporalSession) -> Result<i32, DecodeError> {
    let local = local_micros(&state.fields);
    if let Some(zone) = &state.named_zone {
        if state.fmask & DTZMOD != 0 {
            return Err(DecodeError::BadFormat);
        }
        return Ok(-zone.offset_for_local(local));
    }
    if let Some(west) = state.zone_west {
        return Ok(west);
    }
    if state.fmask & DTZMOD != 0 {
        return Err(DecodeError::BadFormat);
    }
    Ok(-session.zone.offset_for_local(local))
}

fn local_micros(fields: &Fields) -> i64 {
    let date = date2j(fields.year, fields.month, fields.day)
        - i64::from(super::calendar::POSTGRES_EPOCH_JDATE);
    date.saturating_mul(USECS_PER_DAY)
        .saturating_add(super::calendar::time_value(
            fields.hour,
            fields.minute,
            fields.second,
            fields.micros,
        ))
}

fn decode_datetime_field(
    tokens: &[Token],
    index: usize,
    token: &Token,
    state: &mut State,
    session: &TemporalSession,
) -> Result<Option<u32>, DecodeError> {
    let field = token.text.as_str();
    let tmask = match token.kind {
        TokenKind::Date => {
            if state.ptype == Some(Prefix::Julian) {
                return Err(DecodeError::BadFormat);
            }
            if state.fmask & DATE_M == DATE_M || state.ptype.is_some() {
                if field.starts_with(|c: char| c.is_ascii_digit()) || state.ptype.is_some() {
                    if state.ptype.is_some() {
                        if state.ptype != Some(Prefix::IsoTime) {
                            return Err(DecodeError::BadFormat);
                        }
                        state.ptype = None;
                    }
                    run_together_time_zone(field, state)?
                } else {
                    state.named_zone = Some(
                        lookup_zone(field)
                            .ok_or_else(|| DecodeError::BadTimezone(field.to_string()))?,
                    );
                    TZ
                }
            } else {
                decode_date(field, state.fmask, state, session.date_order)?
            }
        }
        TokenKind::Time => {
            if let Some(ptype) = state.ptype.take() {
                if ptype != Prefix::IsoTime {
                    return Err(DecodeError::BadFormat);
                }
            }
            decode_time(field, &mut state.fields)?;
            if time_overflows(&state.fields) {
                return Err(DecodeError::FieldOverflow);
            }
            TIME_M
        }
        TokenKind::Tz => {
            state.zone_west = Some(decode_timezone(field)?);
            TZ
        }
        TokenKind::Number => decode_number_token(tokens, index, field, state, session)?,
        TokenKind::String | TokenKind::Special => {
            return decode_keyword_field(tokens, index, field, state, session);
        }
    };
    Ok(Some(tmask))
}

fn run_together_time_zone(field: &str, state: &mut State) -> Result<u32, DecodeError> {
    if state.fmask & TIME_M == TIME_M {
        return Err(DecodeError::BadFormat);
    }
    let position = field.find('-').ok_or(DecodeError::BadFormat)?;
    state.zone_west = Some(decode_timezone(&field[position..])?);
    let (tmask, _) = decode_number_field(&field[..position], state.fmask, state)?;
    Ok(tmask | TZ)
}

fn decode_number_token(
    _tokens: &[Token],
    _index: usize,
    field: &str,
    state: &mut State,
    session: &TemporalSession,
) -> Result<u32, DecodeError> {
    if let Some(ptype) = state.ptype.take() {
        let (value, rest) = leading_int(field)?;
        if !rest.is_empty() && !rest.starts_with('.') {
            return Err(DecodeError::BadFormat);
        }
        return match ptype {
            Prefix::Julian => {
                if value < 0 {
                    return Err(DecodeError::FieldOverflow);
                }
                let (year, month, day) = j2date(i64::from(value));
                state.fields.year = year;
                state.fields.month = month;
                state.fields.day = day;
                state.year_form = YearForm::Julian;
                let mut tmask = DATE_M;
                if rest.starts_with('.') {
                    let fraction = parse_fraction(rest)?;
                    let time = super::tokens::round_half_even(fraction * 86_400_000_000.0);
                    let (hour, minute, second, micros) = time_fields(time);
                    state.fields.hour = hour;
                    state.fields.minute = minute;
                    state.fields.second = second;
                    state.fields.micros = micros;
                    tmask |= TIME_M;
                }
                Ok(tmask)
            }
            Prefix::IsoTime => {
                let (tmask, _) = decode_number_field(field, state.fmask | DATE_M, state)?;
                if tmask != TIME_M {
                    return Err(DecodeError::BadFormat);
                }
                Ok(tmask)
            }
            Prefix::Other => Err(DecodeError::BadFormat),
        };
    }
    let length = field.len();
    let dot = field.find('.');
    if dot.is_some() && state.fmask & DATE_M == 0 {
        decode_date(field, state.fmask, state, session.date_order)
    } else if dot.is_some_and(|dot| dot > 2) {
        decode_number_field(field, state.fmask, state).map(|(tmask, _)| tmask)
    } else if length >= 6 && (state.fmask & DATE_M == 0 || state.fmask & TIME_M == 0) {
        decode_number_field(field, state.fmask, state).map(|(tmask, _)| tmask)
    } else {
        decode_number(field, state.fmask, state, session.date_order)
    }
}

fn decode_keyword_field(
    tokens: &[Token],
    index: usize,
    field: &str,
    state: &mut State,
    session: &TemporalSession,
) -> Result<Option<u32>, DecodeError> {
    if let Some((east, dst)) = zone_abbreviation(field, state, session) {
        if !dst {
            state.zone_west = Some(-east);
            return Ok(Some(TZ));
        }
        state.zone_west = Some(-east);
        return Ok(Some(DTZ | TZ));
    }
    let Some(keyword) = date_keyword(field) else {
        state.named_zone = Some(lookup_zone(field).ok_or(DecodeError::BadFormat)?);
        return Ok(Some(TZ));
    };
    let tmask = match keyword {
        Keyword::Ignore => return Ok(None),
        Keyword::Reserved(reserved) => decode_reserved(reserved, state, session),
        Keyword::Month(month) => {
            let mut tmask = MONTH;
            if state.fmask & MONTH != 0
                && !state.have_text_month
                && state.fmask & DAY == 0
                && (1..=31).contains(&state.fields.month)
            {
                state.fields.day = state.fields.month;
                tmask = DAY;
            }
            state.have_text_month = true;
            state.fields.month = month;
            tmask
        }
        Keyword::DstModifier(seconds) => {
            let west = state.zone_west.ok_or(DecodeError::BadFormat)?;
            state.zone_west = Some(west - seconds);
            DTZMOD | DTZ
        }
        Keyword::AmPm(meridiem) => {
            state.meridiem = Some(meridiem);
            AMPM
        }
        Keyword::AdBc(bc) => {
            state.bc = bc;
            ADBC
        }
        Keyword::Dow(_) => DOW,
        Keyword::Units(unit) => {
            if state.ptype.is_some() {
                return Err(DecodeError::BadFormat);
            }
            state.ptype = Some(if unit == Unit::Julian {
                Prefix::Julian
            } else {
                Prefix::Other
            });
            0
        }
        Keyword::IsoTime => {
            if state.fmask & DATE_M != DATE_M {
                return Err(DecodeError::BadFormat);
            }
            let next = tokens.get(index + 1).map(|token| token.kind);
            if !matches!(
                next,
                Some(TokenKind::Number | TokenKind::Time | TokenKind::Date)
            ) {
                return Err(DecodeError::BadFormat);
            }
            state.ptype = Some(Prefix::IsoTime);
            0
        }
        Keyword::Ago => return Err(DecodeError::BadFormat),
    };
    Ok(Some(tmask))
}

fn zone_abbreviation(field: &str, state: &State, session: &TemporalSession) -> Option<(i32, bool)> {
    if !field.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let local = if state.fmask & DATE_M == DATE_M {
        local_micros(&state.fields)
    } else {
        session.transaction_start
    };
    session
        .zone
        .abbreviation_offset(field, local)
        .or_else(|| session.abbreviations.lookup(field))
}

fn decode_reserved(reserved: Reserved, state: &mut State, session: &TemporalSession) -> u32 {
    match reserved {
        Reserved::Now => {
            let now = session.transaction_start;
            let east = session.zone.offset_at(now);
            set_fields(
                state,
                now + i64::from(east) * super::calendar::USECS_PER_SEC,
            );
            state.zone_west = Some(-east);
            DATE_M | TIME_M | TZ
        }
        Reserved::Today | Reserved::Tomorrow | Reserved::Yesterday => {
            let local = session.zone.utc_to_local(session.transaction_start);
            let day = local.div_euclid(USECS_PER_DAY)
                + match reserved {
                    Reserved::Tomorrow => 1,
                    Reserved::Yesterday => -1,
                    _ => 0,
                };
            set_fields(state, day * USECS_PER_DAY);
            DATE_M
        }
        Reserved::Zulu => {
            state.fields.hour = 0;
            state.fields.minute = 0;
            state.fields.second = 0;
            state.fields.micros = 0;
            state.zone_west = Some(0);
            TIME_M | TZ
        }
        Reserved::Epoch | Reserved::Late | Reserved::Early => {
            state.special = Some(match reserved {
                Reserved::Epoch => Special::Epoch,
                Reserved::Late => Special::Late,
                _ => Special::Early,
            });
            DATE_M | TIME_M | TZ
        }
    }
}

fn set_fields(state: &mut State, local: i64) {
    if let Some(fields) = timestamp_to_fields(local) {
        state.fields = fields;
    }
}

/// `DecodeDate`: a delimited date field such as `1999-01-08` or `08-jan-99`.
fn decode_date(
    field: &str,
    fmask: u32,
    state: &mut State,
    order: DateOrder,
) -> Result<u32, DecodeError> {
    let mut parts: Vec<&str> = Vec::new();
    let bytes = field.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while cursor < bytes.len() && !bytes[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err(DecodeError::BadFormat);
        }
        let start = cursor;
        let digit = bytes[cursor].is_ascii_digit();
        while cursor < bytes.len()
            && (if digit {
                bytes[cursor].is_ascii_digit()
            } else {
                bytes[cursor].is_ascii_alphabetic()
            })
        {
            cursor += 1;
        }
        parts.push(&field[start..cursor]);
        if cursor < bytes.len() {
            cursor += 1;
        }
    }
    let outer_text_month = std::mem::replace(&mut state.have_text_month, false);
    let result = decode_date_parts(&parts, fmask, state, order);
    state.have_text_month = outer_text_month;
    result
}

fn decode_date_parts(
    parts: &[&str],
    fmask: u32,
    state: &mut State,
    order: DateOrder,
) -> Result<u32, DecodeError> {
    let mut fmask = fmask;
    let mut tmask = 0;
    for part in parts
        .iter()
        .filter(|part| part.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        match date_keyword(part) {
            Some(Keyword::Ignore) => continue,
            Some(Keyword::Month(month)) => {
                state.fields.month = month;
                state.have_text_month = true;
            }
            _ => return Err(DecodeError::BadFormat),
        }
        if fmask & MONTH != 0 {
            return Err(DecodeError::BadFormat);
        }
        fmask |= MONTH;
        tmask |= MONTH;
    }
    for part in parts
        .iter()
        .filter(|part| !part.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        if part.is_empty() {
            return Err(DecodeError::BadFormat);
        }
        let dmask = decode_number(part, fmask, state, order)?;
        if fmask & dmask != 0 {
            return Err(DecodeError::BadFormat);
        }
        fmask |= dmask;
        tmask |= dmask;
    }
    if fmask & !(DOY | TZ) != DATE_M {
        return Err(DecodeError::BadFormat);
    }
    Ok(tmask)
}

/// `DecodeNumber`: one numeric date field interpreted by position and order.
fn decode_number(
    field: &str,
    fmask: u32,
    state: &mut State,
    order: DateOrder,
) -> Result<u32, DecodeError> {
    let (value, rest) = leading_int(field)?;
    let digits = field.len() - rest.len();
    if rest.starts_with('.') {
        if digits > 2 {
            return decode_number_field(field, fmask | DATE_M, state).map(|(tmask, _)| tmask);
        }
        state.fields.micros = parse_fractional_second(rest)?;
    } else if !rest.is_empty() {
        return Err(DecodeError::BadFormat);
    }
    let flen = field.len();
    if flen == 3 && fmask & DATE_M == YEAR && (1..=366).contains(&value) {
        state.year_day = value;
        return Ok(DOY | MONTH | DAY);
    }
    let tmask = match fmask & DATE_M {
        0 => {
            if flen >= 3 || order == DateOrder::Ymd {
                state.fields.year = value;
                YEAR
            } else if order == DateOrder::Dmy {
                state.fields.day = value;
                DAY
            } else {
                state.fields.month = value;
                MONTH
            }
        }
        YEAR => {
            state.fields.month = value;
            MONTH
        }
        MONTH => {
            if state.have_text_month && (flen >= 3 || order == DateOrder::Ymd) {
                state.fields.year = value;
                YEAR
            } else {
                state.fields.day = value;
                DAY
            }
        }
        mask if mask == YEAR | MONTH => {
            if state.have_text_month && flen >= 3 && state.year_form == YearForm::TwoDigit {
                state.fields.day = state.fields.year;
                state.fields.year = value;
                state.year_form = YearForm::Plain;
            } else {
                state.fields.day = value;
            }
            DAY
        }
        DAY => {
            state.fields.month = value;
            MONTH
        }
        mask if mask == MONTH | DAY => {
            state.fields.year = value;
            YEAR
        }
        DATE_M => {
            return decode_number_field(field, fmask, state).map(|(tmask, _)| tmask);
        }
        _ => return Err(DecodeError::BadFormat),
    };
    if tmask == YEAR {
        state.year_form = if flen <= 2 {
            YearForm::TwoDigit
        } else {
            YearForm::Plain
        };
    }
    Ok(tmask)
}

/// `DecodeNumberField`: run-together `yyyymmdd`, `hhmmss` or `hhmm`.
fn decode_number_field(
    field: &str,
    fmask: u32,
    state: &mut State,
) -> Result<(u32, bool), DecodeError> {
    if !field
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(DecodeError::BadFormat);
    }
    let mut digits = field;
    if let Some(dot) = field.find('.') {
        let fraction = &field[dot..];
        state.fields.micros = if fraction.len() == 1 {
            0
        } else {
            let value = format!("0{fraction}")
                .parse::<f64>()
                .map_err(|_| DecodeError::BadFormat)?;
            super::tokens::round_half_even(value * 1_000_000.0)
        };
        digits = &field[..dot];
    } else if fmask & DATE_M != DATE_M && digits.len() >= 6 {
        let length = digits.len();
        let parse = |text: &str| text.parse::<i32>().map_err(|_| DecodeError::FieldOverflow);
        state.fields.day = parse(&digits[length - 2..])?;
        state.fields.month = parse(&digits[length - 4..length - 2])?;
        state.fields.year = parse(&digits[..length - 4])?;
        if length - 4 == 2 {
            state.year_form = YearForm::TwoDigit;
        }
        return Ok((DATE_M, true));
    }
    if fmask & TIME_M != TIME_M {
        let parse = |text: &str| text.parse::<i32>().map_err(|_| DecodeError::FieldOverflow);
        if digits.len() == 6 {
            state.fields.second = parse(&digits[4..6])?;
            state.fields.minute = parse(&digits[2..4])?;
            state.fields.hour = parse(&digits[..2])?;
            return Ok((TIME_M, false));
        }
        if digits.len() == 4 {
            state.fields.second = 0;
            state.fields.minute = parse(&digits[2..4])?;
            state.fields.hour = parse(&digits[..2])?;
            return Ok((TIME_M, false));
        }
    }
    Err(DecodeError::BadFormat)
}

/// `DecodeTime`: `hh:mm[:ss[.fff]]` or `mm:ss.fff`.
pub(super) fn decode_time(field: &str, fields: &mut Fields) -> Result<(), DecodeError> {
    let (hour, rest) = leading_int(field)?;
    let rest = rest.strip_prefix(':').ok_or(DecodeError::BadFormat)?;
    let (minute, rest) = leading_int(rest)?;
    let (mut hour, mut minute, mut second, mut micros) = (hour, minute, 0, 0);
    if rest.is_empty() {
    } else if rest.starts_with('.') {
        micros = parse_fractional_second(rest)?;
        second = minute;
        minute = hour;
        hour = 0;
    } else if let Some(rest) = rest.strip_prefix(':') {
        let (value, rest) = leading_int(rest)?;
        second = value;
        if rest.starts_with('.') {
            micros = parse_fractional_second(rest)?;
        } else if !rest.is_empty() {
            return Err(DecodeError::BadFormat);
        }
    } else {
        return Err(DecodeError::BadFormat);
    }
    if hour < 0
        || !(0..60).contains(&minute)
        || !(0..=60).contains(&second)
        || !(0..=1_000_000).contains(&micros)
    {
        return Err(DecodeError::FieldOverflow);
    }
    fields.hour = hour;
    fields.minute = minute;
    fields.second = second;
    fields.micros = micros;
    Ok(())
}

fn time_overflows(fields: &Fields) -> bool {
    if !(0..=24).contains(&fields.hour)
        || !(0..60).contains(&fields.minute)
        || !(0..=60).contains(&fields.second)
        || !(0..=1_000_000).contains(&fields.micros)
    {
        return true;
    }
    super::calendar::time_value(fields.hour, fields.minute, fields.second, fields.micros)
        > USECS_PER_DAY
}

/// `DecodeTimezone`: `+hh[:mm[:ss]]` or `+hhmm`, returned as seconds west.
pub(super) fn decode_timezone(field: &str) -> Result<i32, DecodeError> {
    let negative = match field.as_bytes().first() {
        Some(b'+') => false,
        Some(b'-') => true,
        _ => return Err(DecodeError::BadFormat),
    };
    let body = &field[1..];
    let (mut hour, rest) = leading_int(body).map_err(|error| match error {
        DecodeError::FieldOverflow => DecodeError::TzDispOverflow,
        other => other,
    })?;
    let mut second = 0;
    let (minute, rest) = if let Some(rest) = rest.strip_prefix(':') {
        let (minute, rest) = leading_int(rest).map_err(|_| DecodeError::TzDispOverflow)?;
        if let Some(rest) = rest.strip_prefix(':') {
            let (value, rest) = leading_int(rest).map_err(|_| DecodeError::TzDispOverflow)?;
            second = value;
            (minute, rest)
        } else {
            (minute, rest)
        }
    } else if rest.is_empty() && field.len() > 3 {
        let minute = hour % 100;
        hour /= 100;
        (minute, rest)
    } else {
        (0, rest)
    };
    if !(0..=15).contains(&hour) || !(0..60).contains(&minute) || !(0..60).contains(&second) {
        return Err(DecodeError::TzDispOverflow);
    }
    let mut east = (hour * 60 + minute) * 60 + second;
    if negative {
        east = -east;
    }
    if !rest.is_empty() {
        return Err(DecodeError::BadFormat);
    }
    Ok(-east)
}

/// `ValidateDate`: era, two-digit years, day-of-year and day-of-month checks.
fn validate_date(state: &mut State) -> Result<(), DecodeError> {
    let fields = &mut state.fields;
    if state.fmask & DATE_M != 0 {
        if state.year_form == YearForm::Julian {
        } else if state.bc {
            if fields.year <= 0 {
                return Err(DecodeError::FieldOverflow);
            }
            fields.year = -(fields.year - 1);
        } else if state.year_form == YearForm::TwoDigit {
            if fields.year < 0 {
                return Err(DecodeError::FieldOverflow);
            }
            if fields.year < 70 {
                fields.year += 2000;
            } else if fields.year < 100 {
                fields.year += 1900;
            }
        } else if fields.year <= 0 {
            return Err(DecodeError::FieldOverflow);
        }
    }
    if state.fmask & DOY != 0 {
        let (year, month, day) = j2date(date2j(fields.year, 1, 1) + i64::from(state.year_day) - 1);
        fields.year = year;
        fields.month = month;
        fields.day = day;
    }
    if state.fmask & MONTH != 0 && !(1..=12).contains(&fields.month) {
        return Err(DecodeError::MdFieldOverflow);
    }
    if state.fmask & DAY != 0 && !(1..=31).contains(&fields.day) {
        return Err(DecodeError::MdFieldOverflow);
    }
    if state.fmask & DATE_M == DATE_M && fields.day > days_in_month(fields.year, fields.month) {
        return Err(DecodeError::FieldOverflow);
    }
    Ok(())
}

fn apply_meridiem(state: &mut State) -> Result<(), DecodeError> {
    let Some(meridiem) = state.meridiem else {
        return Ok(());
    };
    if state.fields.hour > 12 {
        return Err(DecodeError::FieldOverflow);
    }
    match meridiem {
        Meridiem::Am if state.fields.hour == 12 => state.fields.hour = 0,
        Meridiem::Pm if state.fields.hour != 12 => state.fields.hour += 12,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_datetime, DecodeError, Special};
    use crate::types::extended::temporal::calendar::Fields;
    use crate::types::extended::temporal::session::{DateOrder, TemporalSession};

    fn fields(input: &str) -> Fields {
        decode_datetime(input, &TemporalSession::default())
            .expect("valid input")
            .fields
    }

    #[test]
    fn should_decode_documented_date_spellings() {
        // Arrange
        let inputs = [
            "January 8, 1999",
            "1999-Jan-08",
            "08-Jan-99",
            "19990108",
            "990108",
            "1999.008",
            "J2451187",
            "1/8/1999",
        ];

        // Act
        let dates = inputs.map(|input| {
            let fields = fields(input);
            (fields.year, fields.month, fields.day)
        });

        // Assert
        assert!(dates.iter().all(|date| *date == (1999, 1, 8)), "{dates:?}");
    }

    #[test]
    fn should_follow_date_order_for_ambiguous_input() {
        // Arrange
        let session = TemporalSession {
            date_order: DateOrder::Ymd,
            ..TemporalSession::default()
        };

        // Act
        let decoded = decode_datetime("01/02/03", &session)
            .expect("YMD input")
            .fields;

        // Assert
        assert_eq!((decoded.year, decoded.month, decoded.day), (2001, 2, 3));
    }

    #[test]
    fn should_decode_zones_and_specials() {
        // Arrange
        let session = TemporalSession::default();

        // Act
        let offset = decode_datetime("2001-02-16 20:38:40-8", &session).expect("offset");
        let named =
            decode_datetime("2024-07-01 12:00:00 America/New_York", &session).expect("named");
        let abbreviation =
            decode_datetime("2001-02-16 20:38:40 PST", &session).expect("abbreviation");
        let epoch = decode_datetime("epoch", &session).expect("epoch");
        let bad = decode_datetime("2001-02-30", &session);

        // Assert
        assert_eq!(offset.zone_west, 8 * 3600);
        assert_eq!(named.zone_west, 4 * 3600);
        assert_eq!(abbreviation.zone_west, 8 * 3600);
        assert_eq!(epoch.special, Some(Special::Epoch));
        assert_eq!(bad, Err(DecodeError::FieldOverflow));
    }
}
