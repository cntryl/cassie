//! Checked session and named-zone abbreviation resolution for date/time input.
use super::super::session::TemporalSession;
use super::super::tokens::{DecodeError, TokenizeError};
use super::local_micros;
use super::State;
use super::{DATE_M, DTZMOD};

pub(super) fn resolve_zone_with_check<E>(
    state: &State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<i32, TokenizeError<E>> {
    let local = local_micros(&state.fields);
    if let Some(zone) = &state.named_zone {
        if state.fmask & DTZMOD != 0 {
            return Err(DecodeError::BadFormat.into());
        }
        return Ok(-zone.offset_for_local(local));
    }
    if let Some(zone) = &state.dynamic_zone {
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
    Ok(-session.zone.offset_for_local(local))
}

pub(super) fn global_zone_abbreviation<E>(
    field: &str,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<(i32, bool)>, TokenizeError<E>> {
    for (index, byte) in field.bytes().enumerate() {
        if index % 128 == 0 {
            check().map_err(TokenizeError::Check)?;
        }
        if !byte.is_ascii_alphabetic() {
            return Ok(None);
        }
    }
    Ok(session.abbreviations.lookup(field))
}

pub(super) fn session_zone_abbreviation<E>(
    field: &str,
    state: &State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<(i32, bool)>, TokenizeError<E>> {
    for (index, byte) in field.bytes().enumerate() {
        if index % 128 == 0 {
            check().map_err(TokenizeError::Check)?;
        }
        if !byte.is_ascii_alphabetic() {
            return Ok(None);
        }
    }
    let local = if state.fmask & DATE_M == DATE_M {
        local_micros(&state.fields)
    } else {
        session.transaction_start
    };
    session
        .zone
        .abbreviation_offset_with_checks(field, local, check)
        .map_err(TokenizeError::Check)
}

pub(super) fn defer_dynamic_session_abbreviation<E>(
    field: &str,
    state: &mut State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, TokenizeError<E>> {
    if state.fmask & DATE_M == DATE_M {
        return Ok(false);
    }
    for (index, byte) in field.bytes().enumerate() {
        if index % 128 == 0 {
            check().map_err(TokenizeError::Check)?;
        }
        if !byte.is_ascii_alphabetic() {
            return Ok(false);
        }
    }
    if session
        .zone
        .abbreviation_is_dynamic_with_checks(field, check)
        .map_err(TokenizeError::Check)?
        != Some(true)
    {
        return Ok(false);
    }
    state.dynamic_zone = Some(session.zone.clone());
    state.dynamic_abbreviation = Some(field.to_string());
    state.zone_west = Some(-session.zone.offset_at(session.transaction_start));
    Ok(true)
}

pub(super) fn zone_abbreviation_with_check<E>(
    field: &str,
    state: &State,
    session: &TemporalSession,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<(i32, bool)>, TokenizeError<E>> {
    if let Some(found) = session_zone_abbreviation(field, state, session, check)? {
        return Ok(Some(found));
    }
    global_zone_abbreviation(field, session, check)
}
