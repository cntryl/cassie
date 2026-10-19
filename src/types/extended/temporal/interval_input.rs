//! `DecodeInterval` and `DecodeISO8601Interval`: PostgreSQL interval input.
use super::super::calendar::{
    DAYS_PER_MONTH, MONTHS_PER_YEAR, USECS_PER_DAY, USECS_PER_HOUR, USECS_PER_MINUTE, USECS_PER_SEC,
};
use super::super::session::IntervalStyle;
use super::super::tokens::{
    date_keyword, leading_integer, parse_fraction, parse_fractional_second, tokenize, truncate_f64,
    truncate_f64_i32, unit_keyword, DecodeError, Keyword, Reserved, TokenKind, Unit,
};
use super::{
    Interval, DAY_MASK, FULL_RANGE, HOUR_MASK, MINUTE_MASK, MONTH_MASK, SECOND_MASK, YEAR_MASK,
};

/// `pg_itm_in`: accumulated interval components.
#[derive(Default)]
struct Accumulator {
    micros: i64,
    days: i32,
    months: i32,
    years: i32,
}

const TIME_FIELDS: u32 = (1 << 10) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 14);
const DATE_FIELDS: u32 = (1 << 1) | (1 << 2) | (1 << 3);

impl Accumulator {
    fn micros(&mut self, value: i64, fraction: f64, scale: i64) -> Result<(), DecodeError> {
        let micros = value.checked_mul(scale).ok_or(DecodeError::FieldOverflow)?;
        self.micros = self
            .micros
            .checked_add(micros)
            .ok_or(DecodeError::FieldOverflow)?;
        self.fraction_micros(fraction, scale)
    }

    fn fraction_micros(&mut self, fraction: f64, scale: i64) -> Result<(), DecodeError> {
        if fraction == 0.0 {
            return Ok(());
        }
        let scaled = fraction * num_traits::ToPrimitive::to_f64(&scale).unwrap_or(0.0);
        let mut micros = truncate_f64(scaled).ok_or(DecodeError::FieldOverflow)?;
        let remainder = scaled - num_traits::ToPrimitive::to_f64(&micros).unwrap_or(0.0);
        if remainder > 0.5 {
            micros += 1;
        } else if remainder < -0.5 {
            micros -= 1;
        }
        self.micros = self
            .micros
            .checked_add(micros)
            .ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn fraction_days(&mut self, fraction: f64, scale: i32) -> Result<(), DecodeError> {
        if fraction == 0.0 {
            return Ok(());
        }
        let scaled = fraction * f64::from(scale);
        let extra = truncate_f64_i32(scaled).ok_or(DecodeError::FieldOverflow)?;
        self.days = self
            .days
            .checked_add(extra)
            .ok_or(DecodeError::FieldOverflow)?;
        self.fraction_micros(scaled - f64::from(extra), USECS_PER_DAY)
    }

    fn fraction_years(&mut self, fraction: f64, scale: i32) -> Result<(), DecodeError> {
        let extra =
            truncate_f64_i32((fraction * f64::from(scale) * f64::from(MONTHS_PER_YEAR)).round())
                .ok_or(DecodeError::FieldOverflow)?;
        self.months = self
            .months
            .checked_add(extra)
            .ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn days(&mut self, value: i64, scale: i32) -> Result<(), DecodeError> {
        let value = i32::try_from(value).map_err(|_| DecodeError::FieldOverflow)?;
        let days = value.checked_mul(scale).ok_or(DecodeError::FieldOverflow)?;
        self.days = self
            .days
            .checked_add(days)
            .ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn months(&mut self, value: i64) -> Result<(), DecodeError> {
        let value = i32::try_from(value).map_err(|_| DecodeError::FieldOverflow)?;
        self.months = self
            .months
            .checked_add(value)
            .ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn years(&mut self, value: i64, scale: i32) -> Result<(), DecodeError> {
        let value = i32::try_from(value).map_err(|_| DecodeError::FieldOverflow)?;
        let years = value.checked_mul(scale).ok_or(DecodeError::FieldOverflow)?;
        self.years = self
            .years
            .checked_add(years)
            .ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn apply(&mut self, unit: Unit, value: i64, fraction: f64) -> Result<u32, DecodeError> {
        let mask = match unit {
            Unit::Microsecond => {
                self.micros(value, fraction, 1)?;
                1 << 14
            }
            Unit::Millisecond => {
                self.micros(value, fraction, 1000)?;
                1 << 13
            }
            Unit::Second => {
                self.micros(value, fraction, USECS_PER_SEC)?;
                if fraction == 0.0 {
                    1 << 12
                } else {
                    (1 << 12) | (1 << 13) | (1 << 14)
                }
            }
            Unit::Minute => {
                self.micros(value, fraction, USECS_PER_MINUTE)?;
                1 << 11
            }
            Unit::Hour => {
                self.micros(value, fraction, USECS_PER_HOUR)?;
                1 << 10
            }
            Unit::Day => {
                self.days(value, 1)?;
                self.fraction_micros(fraction, USECS_PER_DAY)?;
                1 << 3
            }
            Unit::Week => {
                self.days(value, 7)?;
                self.fraction_days(fraction, 7)?;
                1 << 24
            }
            Unit::Month => {
                self.months(value)?;
                self.fraction_days(fraction, DAYS_PER_MONTH)?;
                1 << 1
            }
            Unit::Year => {
                self.years(value, 1)?;
                self.fraction_years(fraction, 1)?;
                1 << 2
            }
            Unit::Decade => {
                self.years(value, 10)?;
                self.fraction_years(fraction, 10)?;
                1 << 25
            }
            Unit::Century => {
                self.years(value, 100)?;
                self.fraction_years(fraction, 100)?;
                1 << 26
            }
            Unit::Millennium => {
                self.years(value, 1000)?;
                self.fraction_years(fraction, 1000)?;
                1 << 27
            }
            _ => return Err(DecodeError::BadFormat),
        };
        Ok(mask)
    }

    /// `ago`: negates every component.
    fn negate(&mut self) -> Result<(), DecodeError> {
        self.micros = self
            .micros
            .checked_neg()
            .ok_or(DecodeError::FieldOverflow)?;
        self.days = self.days.checked_neg().ok_or(DecodeError::FieldOverflow)?;
        self.months = self
            .months
            .checked_neg()
            .ok_or(DecodeError::FieldOverflow)?;
        self.years = self.years.checked_neg().ok_or(DecodeError::FieldOverflow)?;
        Ok(())
    }

    fn finish(self) -> Result<Interval, DecodeError> {
        let months = self
            .years
            .checked_mul(MONTHS_PER_YEAR)
            .and_then(|months| months.checked_add(self.months))
            .ok_or(DecodeError::IntervalOverflow)?;
        let interval = Interval::new(months, self.days, self.micros);
        if interval.is_infinite() {
            return Err(DecodeError::IntervalOverflow);
        }
        Ok(interval)
    }
}

/// `interval_in`: decodes interval text for the field `range` of a typmod.
///
/// # Errors
/// Returns PostgreSQL's decode error class.
pub fn parse_interval(
    input: &str,
    range: i32,
    style: IntervalStyle,
) -> Result<Interval, DecodeError> {
    let decoded = tokenize(input).and_then(|tokens| decode_interval(&tokens, range, style));
    match decoded {
        Err(DecodeError::BadFormat) => decode_iso8601(input.trim()),
        other => other,
    }
}

fn default_unit(range: i32) -> Unit {
    match range {
        YEAR_MASK => Unit::Year,
        MONTH_MASK => Unit::Month,
        range if range == YEAR_MASK | MONTH_MASK => Unit::Month,
        DAY_MASK => Unit::Day,
        HOUR_MASK => Unit::Hour,
        range if range == DAY_MASK | HOUR_MASK => Unit::Hour,
        MINUTE_MASK => Unit::Minute,
        range
            if range == HOUR_MASK | MINUTE_MASK || range == DAY_MASK | HOUR_MASK | MINUTE_MASK =>
        {
            Unit::Minute
        }
        _ => Unit::Second,
    }
}

fn decode_interval(
    tokens: &[super::super::tokens::Token],
    range: i32,
    style: IntervalStyle,
) -> Result<Interval, DecodeError> {
    let force_negative = style == IntervalStyle::SqlStandard
        && tokens
            .first()
            .is_some_and(|token| token.text.starts_with('-'))
        && !tokens[1..]
            .iter()
            .any(|token| token.text.starts_with('-') || token.text.starts_with('+'));
    let mut accumulator = Accumulator::default();
    let (mut fmask, mut unit, mut parsing_unit, mut is_before) =
        (0_u32, None::<Unit>, false, false);
    let mut special = None;
    for (index, token) in tokens.iter().enumerate().rev() {
        let field = token.text.as_str();
        let mut tmask = 0;
        match token.kind {
            TokenKind::Time => {
                tmask =
                    signed_interval_time(field, false, range, force_negative, &mut accumulator)?;
                unit = Some(Unit::Day);
                parsing_unit = false;
            }
            TokenKind::Tz
                if field[1..].contains(':') && interval_time_probe(&field[1..], range) =>
            {
                tmask = signed_interval_time(
                    &field[1..],
                    field.starts_with('-'),
                    range,
                    force_negative,
                    &mut accumulator,
                )?;
                unit = Some(Unit::Day);
                parsing_unit = false;
            }
            TokenKind::Tz | TokenKind::Date | TokenKind::Number => {
                let current = *unit.get_or_insert_with(|| default_unit(range));
                let (value, fraction, current) = interval_number(field, current, force_negative)?;
                tmask = accumulator.apply(current, value, fraction)?;
                unit = Some(if current == Unit::Hour {
                    Unit::Day
                } else {
                    current
                });
                parsing_unit = false;
            }
            TokenKind::String | TokenKind::Special => {
                if parsing_unit {
                    return Err(DecodeError::BadFormat);
                }
                match unit_keyword(field).or_else(|| date_keyword(field)) {
                    Some(Keyword::Ignore) => continue,
                    Some(Keyword::Units(found)) => {
                        unit = Some(found);
                        parsing_unit = true;
                    }
                    Some(Keyword::Ago) => {
                        if index != tokens.len() - 1 {
                            return Err(DecodeError::BadFormat);
                        }
                        is_before = true;
                    }
                    Some(Keyword::Reserved(reserved @ (Reserved::Late | Reserved::Early))) => {
                        tmask = DATE_FIELDS | TIME_FIELDS;
                        special = Some(reserved);
                    }
                    _ => return Err(DecodeError::BadFormat),
                }
            }
        }
        if tmask & fmask != 0 {
            return Err(DecodeError::BadFormat);
        }
        fmask |= tmask;
    }
    if fmask == 0 || parsing_unit {
        return Err(DecodeError::BadFormat);
    }
    if let Some(reserved) = special {
        if tokens.len() != 1 {
            return Err(DecodeError::BadFormat);
        }
        return Ok(if reserved == Reserved::Late {
            Interval::INFINITY
        } else {
            Interval::NEG_INFINITY
        });
    }
    if is_before {
        accumulator.negate()?;
    }
    accumulator.finish()
}

/// A signed number, `years-months`, or a fraction, with its effective unit.
fn interval_number(
    field: &str,
    unit: Unit,
    force_negative: bool,
) -> Result<(i64, f64, Unit), DecodeError> {
    let (mut value, rest) = leading_integer(field)?;
    let negative = field.starts_with('-');
    let (mut fraction, unit) = if let Some(months) = rest.strip_prefix('-') {
        let (months, tail) = leading_integer(months)?;
        if !(0..12).contains(&months) {
            return Err(DecodeError::FieldOverflow);
        }
        if !tail.is_empty() {
            return Err(DecodeError::BadFormat);
        }
        let months = if negative { -months } else { months };
        value = value
            .checked_mul(12)
            .and_then(|value| value.checked_add(months))
            .ok_or(DecodeError::FieldOverflow)?;
        (0.0, Unit::Month)
    } else if rest.starts_with('.') {
        let fraction = parse_fraction(rest)?;
        (if negative { -fraction } else { fraction }, unit)
    } else if rest.is_empty() {
        (0.0, unit)
    } else {
        return Err(DecodeError::BadFormat);
    };
    if force_negative {
        if value > 0 {
            value = -value;
        }
        if fraction > 0.0 {
            fraction = -fraction;
        }
    }
    Ok((value, fraction, unit))
}

/// A time field, negated by its own sign or by SQL-standard sign spreading.
fn signed_interval_time(
    field: &str,
    negative: bool,
    range: i32,
    force_negative: bool,
    accumulator: &mut Accumulator,
) -> Result<u32, DecodeError> {
    let tmask = interval_time(field, range, accumulator)?;
    if negative {
        accumulator.micros = accumulator
            .micros
            .checked_neg()
            .ok_or(DecodeError::FieldOverflow)?;
    }
    if force_negative && accumulator.micros > 0 {
        accumulator.micros = -accumulator.micros;
    }
    Ok(tmask)
}

fn interval_time_probe(field: &str, range: i32) -> bool {
    interval_time(field, range, &mut Accumulator::default()).is_ok()
}

/// `DecodeTimeForInterval`: `h:mm[:ss[.f]]` (or `mm:ss` for MINUTE TO SECOND).
fn interval_time(
    field: &str,
    range: i32,
    accumulator: &mut Accumulator,
) -> Result<u32, DecodeError> {
    let (hour, rest) = leading_integer(field)?;
    let rest = rest.strip_prefix(':').ok_or(DecodeError::BadFormat)?;
    let (minute, rest) = leading_integer(rest)?;
    let (mut hour, mut minute, mut second, mut micros) = (hour, minute, 0, 0);
    if rest.is_empty() {
        if range == MINUTE_MASK | SECOND_MASK {
            second = minute;
            minute = hour;
            hour = 0;
        }
    } else if rest.starts_with('.') {
        micros = parse_fractional_second(rest)?;
        second = minute;
        minute = hour;
        hour = 0;
    } else if let Some(rest) = rest.strip_prefix(':') {
        let (value, rest) = leading_integer(rest)?;
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
    let total = hour
        .checked_mul(USECS_PER_HOUR)
        .and_then(|value| value.checked_add(minute * USECS_PER_MINUTE))
        .and_then(|value| value.checked_add(second * USECS_PER_SEC))
        .and_then(|value| value.checked_add(micros))
        .ok_or(DecodeError::FieldOverflow)?;
    accumulator.micros = total;
    Ok(TIME_FIELDS)
}

/// `ParseISO8601Number`: integer and fractional parts of a decimal number.
fn iso_number(text: &str) -> Result<(i64, f64, &str), DecodeError> {
    let bytes = text.as_bytes();
    if !bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'-' || *byte == b'.')
    {
        return Err(DecodeError::BadFormat);
    }
    let mut end = usize::from(bytes[0] == b'-' || bytes[0] == b'+');
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
    {
        end += 1;
    }
    let value = text[..end]
        .parse::<f64>()
        .map_err(|_| DecodeError::BadFormat)?;
    if !(-1.0e15..=1.0e15).contains(&value) {
        return Err(DecodeError::FieldOverflow);
    }
    let integer = truncate_f64(value).ok_or(DecodeError::FieldOverflow)?;
    let fraction = value - num_traits::ToPrimitive::to_f64(&integer).unwrap_or(0.0);
    Ok((integer, fraction, &text[end..]))
}

fn integer_width(text: &str) -> usize {
    let text = text.strip_prefix('-').unwrap_or(text);
    text.bytes().take_while(u8::is_ascii_digit).count()
}

/// What the ISO 8601 decoder does after one field.
enum IsoStep {
    /// The input is complete.
    Finish,
    /// Continue in the time part (after `T`).
    TimePart,
    /// One designated field was consumed.
    Field,
}

#[derive(Clone, Copy)]
struct IsoField<'a> {
    start: &'a str,
    value: i64,
    fraction: f64,
    unit: char,
    have_field: bool,
}

/// `DecodeISO8601Interval`: `P1Y2M3DT4H5M6S` or `P0001-02-03T04:05:06`.
fn decode_iso8601(input: &str) -> Result<Interval, DecodeError> {
    let mut rest = input.strip_prefix('P').ok_or(DecodeError::BadFormat)?;
    if rest.is_empty() {
        return Err(DecodeError::BadFormat);
    }
    let mut accumulator = Accumulator::default();
    let (mut date_part, mut have_field) = (true, false);
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('T') {
            date_part = false;
            have_field = false;
            rest = after;
            continue;
        }
        let start = rest;
        let (value, fraction, after) = iso_number(rest)?;
        let unit = after.chars().next().unwrap_or('\0');
        rest = after.get(unit.len_utf8().min(after.len())..).unwrap_or("");
        let field = IsoField {
            start,
            value,
            fraction,
            unit,
            have_field,
        };
        let step = if date_part {
            iso_date_field(&field, &mut rest, &mut accumulator)?
        } else {
            iso_time_field(&field, &mut rest, &mut accumulator)?
        };
        match step {
            IsoStep::Finish => return accumulator.finish(),
            IsoStep::TimePart => {
                date_part = false;
                have_field = false;
            }
            IsoStep::Field => have_field = true,
        }
    }
    accumulator.finish()
}

fn iso_date_field(
    field: &IsoField<'_>,
    rest: &mut &str,
    accumulator: &mut Accumulator,
) -> Result<IsoStep, DecodeError> {
    let IsoField {
        start,
        value,
        fraction,
        unit,
        have_field,
    } = *field;
    match unit {
        'Y' => {
            accumulator.years(value, 1)?;
            accumulator.fraction_years(fraction, 1)?;
        }
        'M' => {
            accumulator.months(value)?;
            accumulator.fraction_days(fraction, DAYS_PER_MONTH)?;
        }
        'W' => {
            accumulator.days(value, 7)?;
            accumulator.fraction_days(fraction, 7)?;
        }
        'D' => {
            accumulator.days(value, 1)?;
            accumulator.fraction_micros(fraction, USECS_PER_DAY)?;
        }
        'T' | '\0' if integer_width(start) == 8 && !have_field => {
            accumulator.years(value / 10_000, 1)?;
            accumulator.months((value / 100) % 100)?;
            accumulator.days(value % 100, 1)?;
            accumulator.fraction_micros(fraction, USECS_PER_DAY)?;
            return Ok(if unit == '\0' {
                IsoStep::Finish
            } else {
                IsoStep::TimePart
            });
        }
        'T' | '\0' | '-' => return iso_extended_date(field, rest, accumulator),
        _ => return Err(DecodeError::BadFormat),
    }
    Ok(IsoStep::Field)
}

/// ISO 8601 alternative extended format: `YYYY-MM-DD`.
fn iso_extended_date(
    field: &IsoField<'_>,
    rest: &mut &str,
    accumulator: &mut Accumulator,
) -> Result<IsoStep, DecodeError> {
    if field.have_field {
        return Err(DecodeError::BadFormat);
    }
    accumulator.years(field.value, 1)?;
    accumulator.fraction_years(field.fraction, 1)?;
    match field.unit {
        '\0' => return Ok(IsoStep::Finish),
        'T' => return Ok(IsoStep::TimePart),
        _ => {}
    }
    let (months, fraction, after) = iso_number(rest)?;
    accumulator.months(months)?;
    accumulator.fraction_days(fraction, DAYS_PER_MONTH)?;
    *rest = after;
    if rest.is_empty() {
        return Ok(IsoStep::Finish);
    }
    if let Some(after) = rest.strip_prefix('T') {
        *rest = after;
        return Ok(IsoStep::TimePart);
    }
    *rest = rest.strip_prefix('-').ok_or(DecodeError::BadFormat)?;
    let (days, fraction, after) = iso_number(rest)?;
    accumulator.days(days, 1)?;
    accumulator.fraction_micros(fraction, USECS_PER_DAY)?;
    *rest = after;
    if rest.is_empty() {
        return Ok(IsoStep::Finish);
    }
    *rest = rest.strip_prefix('T').ok_or(DecodeError::BadFormat)?;
    Ok(IsoStep::TimePart)
}

fn iso_time_field(
    field: &IsoField<'_>,
    rest: &mut &str,
    accumulator: &mut Accumulator,
) -> Result<IsoStep, DecodeError> {
    let IsoField {
        start,
        value,
        fraction,
        unit,
        have_field,
    } = *field;
    match unit {
        'H' => accumulator.micros(value, fraction, USECS_PER_HOUR)?,
        'M' => accumulator.micros(value, fraction, USECS_PER_MINUTE)?,
        'S' => accumulator.micros(value, fraction, USECS_PER_SEC)?,
        '\0' if integer_width(start) == 6 && !have_field => {
            accumulator.micros(value / 10_000, 0.0, USECS_PER_HOUR)?;
            accumulator.micros((value / 100) % 100, 0.0, USECS_PER_MINUTE)?;
            accumulator.micros(value % 100, 0.0, USECS_PER_SEC)?;
            accumulator.fraction_micros(fraction, 1)?;
            return Ok(IsoStep::Finish);
        }
        '\0' | ':' => {
            if have_field {
                return Err(DecodeError::BadFormat);
            }
            accumulator.micros(value, fraction, USECS_PER_HOUR)?;
            if unit == '\0' {
                return Ok(IsoStep::Finish);
            }
            let (minutes, fraction, after) = iso_number(rest)?;
            accumulator.micros(minutes, fraction, USECS_PER_MINUTE)?;
            *rest = after;
            if rest.is_empty() {
                return Ok(IsoStep::Finish);
            }
            *rest = rest.strip_prefix(':').ok_or(DecodeError::BadFormat)?;
            let (seconds, fraction, after) = iso_number(rest)?;
            accumulator.micros(seconds, fraction, USECS_PER_SEC)?;
            if !after.is_empty() {
                return Err(DecodeError::BadFormat);
            }
            return Ok(IsoStep::Finish);
        }
        _ => return Err(DecodeError::BadFormat),
    }
    Ok(IsoStep::Field)
}

/// The `INTERVAL` typmod for a field qualifier and optional precision.
#[must_use]
pub fn interval_typmod(range: i32, precision: Option<u8>) -> i32 {
    let precision = precision.map_or(super::FULL_PRECISION, i32::from);
    (range << 16) | (precision & 0xFFFF)
}

/// Parses `YEAR`, `DAY TO SECOND` and the other interval qualifiers.
#[must_use]
pub fn interval_range(qualifier: &str) -> Option<i32> {
    let words: Vec<String> = qualifier
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mask = |word: &str| match word {
        "year" => Some(YEAR_MASK),
        "month" => Some(MONTH_MASK),
        "day" => Some(DAY_MASK),
        "hour" => Some(HOUR_MASK),
        "minute" => Some(MINUTE_MASK),
        "second" => Some(SECOND_MASK),
        _ => None,
    };
    match words.as_slice() {
        [] => Some(FULL_RANGE),
        [single] => mask(single),
        [from, to, until] if to == "to" => {
            let (from, until) = (mask(from)?, mask(until)?);
            let range = match (from, until) {
                (YEAR_MASK, MONTH_MASK) => YEAR_MASK | MONTH_MASK,
                (DAY_MASK, HOUR_MASK) => DAY_MASK | HOUR_MASK,
                (DAY_MASK, MINUTE_MASK) => DAY_MASK | HOUR_MASK | MINUTE_MASK,
                (DAY_MASK, SECOND_MASK) => DAY_MASK | HOUR_MASK | MINUTE_MASK | SECOND_MASK,
                (HOUR_MASK, MINUTE_MASK) => HOUR_MASK | MINUTE_MASK,
                (HOUR_MASK, SECOND_MASK) => HOUR_MASK | MINUTE_MASK | SECOND_MASK,
                (MINUTE_MASK, SECOND_MASK) => MINUTE_MASK | SECOND_MASK,
                _ => return None,
            };
            Some(range)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{interval_range, interval_typmod, parse_interval};
    use crate::types::extended::temporal::interval::{Interval, FULL_RANGE, SECOND_MASK};
    use crate::types::extended::temporal::session::IntervalStyle;

    fn parse(text: &str) -> Interval {
        parse_interval(text, FULL_RANGE, IntervalStyle::Postgres).expect("valid interval")
    }

    #[test]
    fn should_decode_postgresql_interval_spellings() {
        // Arrange
        let pairs = [
            ("@ 1 year 2 mons ago", "-1 year -2 months"),
            ("1.5 weeks", "10 days 12 hours"),
            ("1500 milliseconds", "1.5 seconds"),
            ("2 decades", "20 years"),
            ("P1Y2M3DT4H5M6S", "P0001-02-03T04:05:06"),
            ("1-2", "1 year 2 months"),
            ("3 4:05:06", "3 days 4 hours 5 minutes 6 seconds"),
            ("1.5 years", "1 year 6 months"),
            ("-08:00", "-8 hours"),
        ];

        // Act
        let decoded = pairs.map(|(left, right)| (parse(left), parse(right)));

        // Assert
        for (index, (left, right)) in decoded.iter().enumerate() {
            assert_eq!(left, right, "pair {index}");
        }
    }

    #[test]
    fn should_encode_interval_qualifier_typmods() {
        // Arrange
        let qualifier = "SECOND";

        // Act
        let typmod = interval_typmod(interval_range(qualifier).expect("range"), Some(3));

        // Assert
        assert_eq!(typmod, 268_435_459);
        assert_eq!(
            interval_range("hour to minute"),
            Some((1 << 10) | (1 << 11))
        );
        assert_eq!(SECOND_MASK, 1 << 12);
    }
}
