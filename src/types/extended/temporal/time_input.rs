//! `DecodeTimeOnly`: PostgreSQL's `time` and `timetz` input.
use super::{
    apply_meridiem, date_keyword, decode_date, decode_number, decode_number_field,
    decode_number_token, decode_time, decode_timezone, local_micros, lookup_zone, time_fields,
    time_overflows, validate_date, DecodeError, Keyword, Prefix,
    Reserved, State, TemporalSession, Token, TokenKind, Unit, ADBC, AMPM, DATE_M, DTZ, DTZMOD,
    TIME_M, TZ, USECS_PER_DAY, TokenizeError,
};

/// Decoded `time`/`timetz` input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTime {
    pub time: i64,
    pub zone_west: i32,
}

/// `DecodeTimeOnly`.
///
/// # Errors
/// Returns PostgreSQL's decode error class.
pub fn decode_time_only(
    input: &str,
    session: &TemporalSession,
) -> Result<DecodedTime, DecodeError> {
    let mut check = || Ok::<(), std::convert::Infallible>(());
    decode_time_only_with_check(input, session, &mut check).map_err(|error| match error {
        TokenizeError::Decode(error) => error,
        TokenizeError::Check(never) => match never {},
    })
}

pub(crate) fn decode_time_only_with_check<E>(
    input: &str,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<DecodedTime, TokenizeError<E>> {
    let tokens = super::tokenize_with_check(input, check)?;
    let mut state = State::default();
    for index in 0..tokens.len() {
        if index % 128 == 0 {
            check().map_err(TokenizeError::Check)?;
        }
        let Some(tmask) = time_field(&tokens, index, &mut state, session, check)? else {
            continue;
        };
        if tmask & state.fmask != 0 {
            return Err(DecodeError::BadFormat.into());
        }
        state.fmask |= tmask;
    }
    if state.ptype.is_some() {
        return Err(DecodeError::BadFormat.into());
    }
    validate_date(&mut state)?;
    apply_meridiem(&mut state)?;
    if time_overflows(&state.fields) {
        return Err(DecodeError::FieldOverflow.into());
    }
    if state.fmask & TIME_M != TIME_M {
        return Err(DecodeError::BadFormat.into());
    }
    let time = super::super::calendar::time_value(
        state.fields.hour,
        state.fields.minute,
        state.fields.second,
        state.fields.micros,
    );
    check().map_err(TokenizeError::Check)?;
    let zone_west = time_zone_west(&state, session, time, check)?;
    check().map_err(TokenizeError::Check)?;
    Ok(DecodedTime { time, zone_west })
}

fn time_zone_west<E>(
    state: &State,
    session: &TemporalSession,
    time: i64,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<i32, TokenizeError<E>> {
    let local_with_date = || -> Result<i64, DecodeError> {
        if state.fmask & DATE_M == DATE_M {
            Ok(local_micros(&state.fields))
        } else {
            Err(DecodeError::BadFormat)
        }
    };
    if let Some(zone) = &state.named_zone {
        if state.fmask & DTZMOD != 0 {
            return Err(DecodeError::BadFormat.into());
        }
        if let super::Zone::Rules { tz, .. } = zone {
            if let Ok(offset) = tz.to_fixed_offset() {
                return Ok(-offset.seconds());
            }
        }
        return Ok(-zone.offset_for_local(local_with_date()?));
    }
    if let Some(zone) = &state.dynamic_zone {
        let local = local_with_date()?;
        let abbreviation = state.dynamic_abbreviation.as_deref().unwrap_or("msk");
        let east = zone
            .abbreviation_offset_with_checks(abbreviation, local, check)
            .map_err(TokenizeError::Check)?
            .or_else(|| session.abbreviations.lookup(abbreviation))
            .map(|(east, _)| east)
            .ok_or_else(|| DecodeError::BadTimezone(abbreviation.to_string()))?;
        return Ok(-east + state.zone_west_adjustment);
    }
    if let Some(west) = state.zone_west {
        return Ok(west);
    }
    if state.fmask & DTZMOD != 0 {
        return Err(DecodeError::BadFormat.into());
    }
    let local = if state.fmask & DATE_M == 0 {
        let today = session
            .zone
            .utc_to_local(session.transaction_start)
            .div_euclid(USECS_PER_DAY);
        today * USECS_PER_DAY + time
    } else {
        local_with_date()?
    };
    Ok(-session.zone.offset_for_local(local))
}

fn time_field<E>(
    tokens: &[Token],
    index: usize,
    state: &mut State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<u32>, TokenizeError<E>> {
    let token = &tokens[index];
    let field = token.text.as_str();
    let count = tokens.len();
    let tmask = match token.kind {
        TokenKind::Date => {
            if index == 0
                && count >= 2
                && (tokens[count - 1].kind == TokenKind::Date || tokens[1].kind == TokenKind::Time)
            {
                decode_date(field, state.fmask, state, session.date_order)?
            } else if field.starts_with(|c: char| c.is_ascii_digit()) {
                if state.fmask & TIME_M == TIME_M {
                    return Err(DecodeError::BadFormat.into());
                }
                let position = field.find('-').ok_or(DecodeError::BadFormat)?;
                state.zone_west = Some(decode_timezone(&field[position..])?);
                let (tmask, _) =
                    decode_number_field(&field[..position], state.fmask | DATE_M, state)?;
                tmask | TZ
            } else {
                state.named_zone = Some(
                    lookup_zone(field)
                        .ok_or_else(|| DecodeError::BadTimezone(field.to_string()))?,
                );
                TZ
            }
        }
        TokenKind::Time => {
            if let Some(ptype) = state.ptype.take() {
                if ptype != Prefix::IsoTime {
                    return Err(DecodeError::BadFormat.into());
                }
            }
            decode_time(field, &mut state.fields)?;
            TIME_M
        }
        TokenKind::Tz => {
            state.zone_west = Some(decode_timezone(field)?);
            TZ
        }
        TokenKind::Number => time_number(tokens, index, field, state, session)?,
        TokenKind::String | TokenKind::Special => {
            return time_keyword(tokens, index, field, state, session, check)
        }
    };
    Ok(Some(tmask))
}

fn time_number(
    tokens: &[Token],
    index: usize,
    field: &str,
    state: &mut State,
    session: &TemporalSession,
) -> Result<u32, DecodeError> {
    if state.ptype.is_some() {
        return decode_number_token(tokens, index, field, state, session);
    }
    if let Some(dot) = field.find('.') {
        if index == 0 && tokens.len() >= 2 && tokens[tokens.len() - 1].kind == TokenKind::Date {
            decode_date(field, state.fmask, state, session.date_order)
        } else if dot > 2 {
            Ok(decode_number_field(field, state.fmask | DATE_M, state)?.0)
        } else {
            Err(DecodeError::BadFormat)
        }
    } else if field.len() > 4 {
        Ok(decode_number_field(field, state.fmask | DATE_M, state)?.0)
    } else {
        decode_number(field, state.fmask | DATE_M, state, session.date_order)
    }
}

fn time_keyword<E>(
    tokens: &[Token],
    index: usize,
    field: &str,
    state: &mut State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<u32>, TokenizeError<E>> {
    if state.fmask & DATE_M != DATE_M
        && tokens.iter().any(|token| token.kind == TokenKind::Date)
        && super::defer_dynamic_session_abbreviation(field, state, session, check)?
    {
        return Ok(Some(TZ));
    }
    if let Some((east, _)) = super::zone_abbreviation_with_check(field, state, session, check)? {
        state.zone_west = Some(-east);
        return Ok(Some(TZ));
    }
    let tmask = match date_keyword(field) {
        Some(Keyword::Ignore) => return Ok(None),
        Some(Keyword::Reserved(Reserved::Now)) => {
            let local = session.zone.utc_to_local(session.transaction_start);
            let (hour, minute, second, micros) = time_fields(local.rem_euclid(USECS_PER_DAY));
            state.fields.hour = hour;
            state.fields.minute = minute;
            state.fields.second = second;
            state.fields.micros = micros;
            TIME_M
        }
        Some(Keyword::Reserved(Reserved::Zulu)) => {
            state.fields.hour = 0;
            state.fields.minute = 0;
            state.fields.second = 0;
            state.fields.micros = 0;
            TIME_M | TZ
        }
        Some(Keyword::DstModifier(seconds)) => {
            let west = state.zone_west.ok_or(DecodeError::BadFormat)?;
            state.zone_west = Some(west - seconds);
            DTZMOD | DTZ
        }
        Some(Keyword::AmPm(meridiem)) => {
            state.meridiem = Some(meridiem);
            AMPM
        }
        Some(Keyword::AdBc(bc)) => {
            state.bc = bc;
            ADBC
        }
        Some(Keyword::Units(unit)) => {
            if state.ptype.is_some() {
                return Err(DecodeError::BadFormat.into());
            }
            state.ptype = Some(if unit == Unit::Julian {
                Prefix::Julian
            } else {
                Prefix::Other
            });
            0
        }
        Some(Keyword::IsoTime) => {
            if state.ptype.is_some() {
                return Err(DecodeError::BadFormat.into());
            }
            let next = tokens.get(index + 1).map(|token| token.kind);
            if !matches!(
                next,
                Some(TokenKind::Number | TokenKind::Time | TokenKind::Date)
            ) {
                return Err(DecodeError::BadFormat.into());
            }
            state.ptype = Some(Prefix::IsoTime);
            0
        }
        None => {
            state.named_zone = Some(lookup_zone(field).ok_or(DecodeError::BadFormat)?);
            TZ
        }
        _ => return Err(DecodeError::BadFormat.into()),
    };
    Ok(Some(tmask))
}

#[cfg(test)]
mod tests {
    use super::decode_time_only;
    use crate::types::extended::temporal::session::TemporalSession;

    #[test]
    fn should_decode_time_only_forms() {
        // Arrange
        let session = TemporalSession::default();

        // Act
        let compact = decode_time_only("040506", &session).expect("compact");
        let meridiem = decode_time_only("04:05 PM", &session).expect("pm");
        let zoned = decode_time_only("05:34:17-05", &session).expect("zoned");
        let midnight = decode_time_only("24:00:00", &session).expect("24:00");

        // Assert
        assert_eq!(compact.time, 14_706_000_000);
        assert_eq!(meridiem.time, 57_900_000_000);
        assert_eq!(zoned.zone_west, 5 * 3600);
        assert_eq!(midnight.time, 86_400_000_000);
    }
}
