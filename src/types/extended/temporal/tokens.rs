//! `ParseDateTime` tokenization and the keyword tables shared by date/time
//! and interval input.

/// A decoder error, mapped to PostgreSQL's message and SQLSTATE by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    BadFormat,
    FieldOverflow,
    MdFieldOverflow,
    IntervalOverflow,
    TzDispOverflow,
    BadTimezone(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Number,
    String,
    Date,
    Time,
    Tz,
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

/// Keyword categories (`datetktbl` / `deltatktbl` types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Reserved(Reserved),
    Month(i32),
    AdBc(bool),
    AmPm(Meridiem),
    Ignore,
    Dow(i32),
    Units(Unit),
    IsoTime,
    DstModifier(i32),
    Ago,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reserved {
    Late,
    Early,
    Epoch,
    Now,
    Today,
    Tomorrow,
    Yesterday,
    Zulu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meridiem {
    Am,
    Pm,
}

/// Field units for interval input, `date_part` and `date_trunc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Microsecond,
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
    Decade,
    Century,
    Millennium,
    Julian,
    Dow,
    Doy,
    IsoDow,
    IsoYear,
    Timezone,
    TimezoneHour,
    TimezoneMinute,
    Epoch,
}

const DATE_KEYWORDS: &[(&str, Keyword)] = &[
    ("+infinity", Keyword::Reserved(Reserved::Late)),
    ("-infinity", Keyword::Reserved(Reserved::Early)),
    ("ad", Keyword::AdBc(false)),
    ("allballs", Keyword::Reserved(Reserved::Zulu)),
    ("am", Keyword::AmPm(Meridiem::Am)),
    ("apr", Keyword::Month(4)),
    ("april", Keyword::Month(4)),
    ("at", Keyword::Ignore),
    ("aug", Keyword::Month(8)),
    ("august", Keyword::Month(8)),
    ("bc", Keyword::AdBc(true)),
    ("d", Keyword::Units(Unit::Day)),
    ("dec", Keyword::Month(12)),
    ("december", Keyword::Month(12)),
    ("dow", Keyword::Units(Unit::Dow)),
    ("doy", Keyword::Units(Unit::Doy)),
    ("dst", Keyword::DstModifier(3600)),
    ("epoch", Keyword::Reserved(Reserved::Epoch)),
    ("feb", Keyword::Month(2)),
    ("february", Keyword::Month(2)),
    ("fri", Keyword::Dow(5)),
    ("friday", Keyword::Dow(5)),
    ("h", Keyword::Units(Unit::Hour)),
    ("infinity", Keyword::Reserved(Reserved::Late)),
    ("isodow", Keyword::Units(Unit::IsoDow)),
    ("isoyear", Keyword::Units(Unit::IsoYear)),
    ("j", Keyword::Units(Unit::Julian)),
    ("jan", Keyword::Month(1)),
    ("january", Keyword::Month(1)),
    ("jd", Keyword::Units(Unit::Julian)),
    ("jul", Keyword::Month(7)),
    ("julian", Keyword::Units(Unit::Julian)),
    ("july", Keyword::Month(7)),
    ("jun", Keyword::Month(6)),
    ("june", Keyword::Month(6)),
    ("m", Keyword::Units(Unit::Month)),
    ("mar", Keyword::Month(3)),
    ("march", Keyword::Month(3)),
    ("may", Keyword::Month(5)),
    ("mm", Keyword::Units(Unit::Minute)),
    ("mon", Keyword::Dow(1)),
    ("monday", Keyword::Dow(1)),
    ("nov", Keyword::Month(11)),
    ("november", Keyword::Month(11)),
    ("now", Keyword::Reserved(Reserved::Now)),
    ("oct", Keyword::Month(10)),
    ("october", Keyword::Month(10)),
    ("on", Keyword::Ignore),
    ("pm", Keyword::AmPm(Meridiem::Pm)),
    ("s", Keyword::Units(Unit::Second)),
    ("sat", Keyword::Dow(6)),
    ("saturday", Keyword::Dow(6)),
    ("sep", Keyword::Month(9)),
    ("sept", Keyword::Month(9)),
    ("september", Keyword::Month(9)),
    ("sun", Keyword::Dow(0)),
    ("sunday", Keyword::Dow(0)),
    ("t", Keyword::IsoTime),
    ("thu", Keyword::Dow(4)),
    ("thur", Keyword::Dow(4)),
    ("thurs", Keyword::Dow(4)),
    ("thursday", Keyword::Dow(4)),
    ("today", Keyword::Reserved(Reserved::Today)),
    ("tomorrow", Keyword::Reserved(Reserved::Tomorrow)),
    ("tue", Keyword::Dow(2)),
    ("tues", Keyword::Dow(2)),
    ("tuesday", Keyword::Dow(2)),
    ("wed", Keyword::Dow(3)),
    ("wednesday", Keyword::Dow(3)),
    ("weds", Keyword::Dow(3)),
    ("y", Keyword::Units(Unit::Year)),
    ("yesterday", Keyword::Reserved(Reserved::Yesterday)),
];

const UNIT_KEYWORDS: &[(&str, Keyword)] = &[
    ("@", Keyword::Ignore),
    ("ago", Keyword::Ago),
    ("c", Keyword::Units(Unit::Century)),
    ("cent", Keyword::Units(Unit::Century)),
    ("centuries", Keyword::Units(Unit::Century)),
    ("century", Keyword::Units(Unit::Century)),
    ("d", Keyword::Units(Unit::Day)),
    ("day", Keyword::Units(Unit::Day)),
    ("days", Keyword::Units(Unit::Day)),
    ("dec", Keyword::Units(Unit::Decade)),
    ("decade", Keyword::Units(Unit::Decade)),
    ("decades", Keyword::Units(Unit::Decade)),
    ("decs", Keyword::Units(Unit::Decade)),
    ("h", Keyword::Units(Unit::Hour)),
    ("hour", Keyword::Units(Unit::Hour)),
    ("hours", Keyword::Units(Unit::Hour)),
    ("hr", Keyword::Units(Unit::Hour)),
    ("hrs", Keyword::Units(Unit::Hour)),
    ("m", Keyword::Units(Unit::Minute)),
    ("microsecon", Keyword::Units(Unit::Microsecond)),
    ("mil", Keyword::Units(Unit::Millennium)),
    ("millennia", Keyword::Units(Unit::Millennium)),
    ("millennium", Keyword::Units(Unit::Millennium)),
    ("millisecon", Keyword::Units(Unit::Millisecond)),
    ("mils", Keyword::Units(Unit::Millennium)),
    ("min", Keyword::Units(Unit::Minute)),
    ("mins", Keyword::Units(Unit::Minute)),
    ("minute", Keyword::Units(Unit::Minute)),
    ("minutes", Keyword::Units(Unit::Minute)),
    ("mon", Keyword::Units(Unit::Month)),
    ("mons", Keyword::Units(Unit::Month)),
    ("month", Keyword::Units(Unit::Month)),
    ("months", Keyword::Units(Unit::Month)),
    ("ms", Keyword::Units(Unit::Millisecond)),
    ("msec", Keyword::Units(Unit::Millisecond)),
    ("msecond", Keyword::Units(Unit::Millisecond)),
    ("mseconds", Keyword::Units(Unit::Millisecond)),
    ("msecs", Keyword::Units(Unit::Millisecond)),
    ("qtr", Keyword::Units(Unit::Quarter)),
    ("quarter", Keyword::Units(Unit::Quarter)),
    ("s", Keyword::Units(Unit::Second)),
    ("sec", Keyword::Units(Unit::Second)),
    ("second", Keyword::Units(Unit::Second)),
    ("seconds", Keyword::Units(Unit::Second)),
    ("secs", Keyword::Units(Unit::Second)),
    ("timezone", Keyword::Units(Unit::Timezone)),
    ("timezone_h", Keyword::Units(Unit::TimezoneHour)),
    ("timezone_m", Keyword::Units(Unit::TimezoneMinute)),
    ("us", Keyword::Units(Unit::Microsecond)),
    ("usec", Keyword::Units(Unit::Microsecond)),
    ("usecond", Keyword::Units(Unit::Microsecond)),
    ("useconds", Keyword::Units(Unit::Microsecond)),
    ("usecs", Keyword::Units(Unit::Microsecond)),
    ("w", Keyword::Units(Unit::Week)),
    ("week", Keyword::Units(Unit::Week)),
    ("weeks", Keyword::Units(Unit::Week)),
    ("y", Keyword::Units(Unit::Year)),
    ("year", Keyword::Units(Unit::Year)),
    ("years", Keyword::Units(Unit::Year)),
    ("yr", Keyword::Units(Unit::Year)),
    ("yrs", Keyword::Units(Unit::Year)),
];

/// Tokens are compared on their first ten characters (`TOKMAXLEN`).
fn truncated(token: &str) -> &str {
    token
        .char_indices()
        .nth(10)
        .map_or(token, |(index, _)| &token[..index])
}

fn search(table: &[(&str, Keyword)], token: &str) -> Option<Keyword> {
    let key = truncated(token);
    table
        .iter()
        .find(|(name, _)| truncated(name) == key)
        .map(|(_, keyword)| *keyword)
}

/// `DecodeSpecial`: the date/time keyword table.
#[must_use]
pub fn date_keyword(token: &str) -> Option<Keyword> {
    search(DATE_KEYWORDS, token)
}

/// `DecodeUnits`: the interval and field-name table.
#[must_use]
pub fn unit_keyword(token: &str) -> Option<Keyword> {
    search(UNIT_KEYWORDS, token)
}

/// Resolves a field name for `date_part`/`date_trunc`/`EXTRACT`.
#[must_use]
pub fn field_unit(name: &str) -> Option<Unit> {
    let lower = name.to_ascii_lowercase();
    match unit_keyword(&lower).or_else(|| date_keyword(&lower)) {
        Some(Keyword::Units(unit)) => Some(unit),
        Some(Keyword::Reserved(Reserved::Epoch)) => Some(Unit::Epoch),
        _ => None,
    }
}

/// `ParseDateTime`: splits input into typed fields, lower-casing letters.
///
/// # Errors
/// Returns `BadFormat` for characters PostgreSQL rejects.
pub fn tokenize(input: &str) -> Result<Vec<Token>, DecodeError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let at = |index: usize| chars.get(index).copied().unwrap_or('\0');
    while index < chars.len() {
        let current = chars[index];
        if current.is_whitespace() {
            index += 1;
            continue;
        }
        let mut text = String::new();
        let kind;
        if current.is_ascii_digit() {
            kind = digit_field(&chars, &mut index, &mut text);
        } else if current == '.' {
            text.push('.');
            index += 1;
            while at(index).is_ascii_digit() {
                text.push(at(index));
                index += 1;
            }
            kind = TokenKind::Number;
        } else if current.is_ascii_alphabetic() {
            while at(index).is_ascii_alphabetic() {
                text.push(at(index).to_ascii_lowercase());
                index += 1;
            }
            let next = at(index);
            let is_date = matches!(next, '-' | '/' | '.')
                || ((next == '+' || next.is_ascii_digit()) && date_keyword(&text).is_none());
            if is_date {
                kind = TokenKind::Date;
                loop {
                    text.push(at(index).to_ascii_lowercase());
                    index += 1;
                    let next = at(index);
                    if !(matches!(next, '+' | '-' | '/' | '_' | '.' | ':')
                        || next.is_ascii_alphanumeric())
                    {
                        break;
                    }
                }
            } else {
                kind = TokenKind::String;
            }
        } else if current == '+' || current == '-' {
            text.push(current);
            index += 1;
            while at(index).is_whitespace() {
                index += 1;
            }
            if at(index).is_ascii_digit() {
                kind = TokenKind::Tz;
                text.push(at(index));
                index += 1;
                while matches!(at(index), '0'..='9' | ':' | '.' | '-') {
                    text.push(at(index));
                    index += 1;
                }
            } else if at(index).is_ascii_alphabetic() {
                kind = TokenKind::Special;
                while at(index).is_ascii_alphabetic() {
                    text.push(at(index).to_ascii_lowercase());
                    index += 1;
                }
            } else {
                return Err(DecodeError::BadFormat);
            }
        } else if current.is_ascii_punctuation() {
            index += 1;
            continue;
        } else {
            return Err(DecodeError::BadFormat);
        }
        tokens.push(Token { text, kind });
    }
    Ok(tokens)
}

/// A field starting with a digit: a number, a time, or a delimited date.
fn digit_field(chars: &[char], index: &mut usize, text: &mut String) -> TokenKind {
    let at = |index: usize| chars.get(index).copied().unwrap_or('\0');
    let kind;
    while at(*index).is_ascii_digit() {
        text.push(at(*index));
        *index += 1;
    }
    if at(*index) == ':' {
        kind = TokenKind::Time;
        while matches!(at(*index), '0'..='9' | ':' | '.') {
            text.push(at(*index));
            *index += 1;
        }
    } else if matches!(at(*index), '-' | '/' | '.') {
        let delimiter = at(*index);
        text.push(delimiter);
        *index += 1;
        if at(*index).is_ascii_digit() {
            let mut field_kind = if delimiter == '.' {
                TokenKind::Number
            } else {
                TokenKind::Date
            };
            while at(*index).is_ascii_digit() {
                text.push(at(*index));
                *index += 1;
            }
            if at(*index) == delimiter {
                field_kind = TokenKind::Date;
                while at(*index).is_ascii_digit() || at(*index) == delimiter {
                    text.push(at(*index));
                    *index += 1;
                }
            }
            kind = field_kind;
        } else {
            kind = TokenKind::Date;
            while at(*index).is_ascii_alphanumeric() || at(*index) == delimiter {
                text.push(at(*index).to_ascii_lowercase());
                *index += 1;
            }
        }
    } else {
        kind = TokenKind::Number;
    }
    kind
}

/// `strtoint`-style parse of a leading signed decimal integer; returns the
/// value and the remaining text.
///
/// # Errors
/// Returns `BadFormat` without digits and `FieldOverflow` past `i64`.
pub fn leading_integer(text: &str) -> Result<(i64, &str), DecodeError> {
    let bytes = text.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == digits_start {
        return Err(DecodeError::BadFormat);
    }
    let value = text[..end]
        .parse::<i64>()
        .map_err(|_| DecodeError::FieldOverflow)?;
    Ok((value, &text[end..]))
}

/// Like [`leading_integer`] but limited to `int` range.
///
/// # Errors
/// Returns `BadFormat` without digits and `FieldOverflow` past `i32`.
pub fn leading_int(text: &str) -> Result<(i32, &str), DecodeError> {
    let (value, rest) = leading_integer(text)?;
    let value = i32::try_from(value).map_err(|_| DecodeError::FieldOverflow)?;
    Ok((value, rest))
}

/// `ParseFraction`: `.digits` as a fraction in `[0, 1)`.
///
/// # Errors
/// Returns `BadFormat` for non-digit fraction text.
pub fn parse_fraction(text: &str) -> Result<f64, DecodeError> {
    let digits = text.strip_prefix('.').ok_or(DecodeError::BadFormat)?;
    if digits.is_empty() {
        return Ok(0.0);
    }
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DecodeError::BadFormat);
    }
    format!("0.{digits}")
        .parse::<f64>()
        .map_err(|_| DecodeError::BadFormat)
}

/// `ParseFractionalSecond`: microseconds of a `.digits` suffix.
///
/// # Errors
/// Returns `BadFormat` for non-digit fraction text.
pub fn parse_fractional_second(text: &str) -> Result<i64, DecodeError> {
    let fraction = parse_fraction(text)?;
    Ok(round_half_even(fraction * 1_000_000.0))
}

/// `rint`: rounds to the nearest integer, ties to even.
#[must_use]
pub fn round_half_even(value: f64) -> i64 {
    let rounded = value.round_ties_even();
    num_traits::ToPrimitive::to_i64(&rounded).unwrap_or(if rounded < 0.0 {
        i64::MIN
    } else {
        i64::MAX
    })
}

/// Truncates toward zero to `i64`, or `None` outside its range or for NaN.
#[must_use]
pub fn truncate_f64(value: f64) -> Option<i64> {
    num_traits::ToPrimitive::to_i64(&value.trunc())
}

/// Truncates toward zero to `i32`, or `None` outside its range or for NaN.
#[must_use]
pub fn truncate_f64_i32(value: f64) -> Option<i32> {
    num_traits::ToPrimitive::to_i32(&value.trunc())
}

#[cfg(test)]
mod tests {
    use super::{tokenize, TokenKind};

    #[test]
    fn should_tokenize_postgresql_datetime_fields() {
        // Arrange
        let input = "1999-01-08T04:05:06 America/New_York -8";

        // Act
        let tokens = tokenize(input).expect("tokens");

        // Assert
        let kinds = tokens.iter().map(|token| token.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                TokenKind::Date,
                TokenKind::String,
                TokenKind::Time,
                TokenKind::Date,
                TokenKind::Tz
            ]
        );
        assert_eq!(tokens[3].text, "america/new_york");
    }
}
