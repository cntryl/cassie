//! Query-owned text input for temporal SQL casts and wire parameters.

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::temporal::tokens::{self, TokenizeError};
use crate::types::extended::temporal::{self, DecodeError, TemporalSession};
use crate::types::extended::{ExtendedType, TypeInputError};
use crate::types::typed::TypedValue;

/// Parses temporal SQL text while accounting for tokenizer storage and
/// propagating the query's interruption signal through token and zone scans.
pub(crate) fn parse_text(
    kind: ExtendedType,
    text: &str,
    session: &TemporalSession,
    controls: &QueryExecutionControls,
) -> Result<TypedValue, CassieError> {
    let bytes = tokens::tokenization_memory_bound(text)
        .ok_or_else(|| CassieError::ResourceLimit("temporal input size overflow".into()))?;
    let _scratch = controls.reserve_query_memory(bytes)?;
    controls.check_interruption()?;
    let mut check = || controls.check_interruption();
    let value = match kind {
        ExtendedType::Date => {
            let decoded = temporal::decode::decode_datetime_with_check(text, session, &mut check)
                .map_err(|error| decode_error("date", text, error))?;
            TypedValue::Date(parse_date(text, decoded).map_err(CassieError::from)?)
        }
        ExtendedType::Time { precision } => {
            let decoded = temporal::decode::decode_time_only_with_check(text, session, &mut check)
                .map_err(|error| decode_error("time", text, error))?;
            TypedValue::Time(temporal::calendar::round_time(decoded.time, precision))
        }
        ExtendedType::TimeTz { precision } => {
            let decoded = temporal::decode::decode_time_only_with_check(text, session, &mut check)
                .map_err(|error| decode_error("time with time zone", text, error))?;
            TypedValue::TimeTz(temporal::TimeTz {
                time: temporal::calendar::round_time(decoded.time, precision),
                zone: decoded.zone_west,
            })
        }
        ExtendedType::Timestamp { precision } => {
            let decoded = temporal::decode::decode_datetime_with_check(text, session, &mut check)
                .map_err(|error| decode_error("timestamp", text, error))?;
            let timestamp = match decoded.special {
                Some(temporal::decode::Special::Late) => temporal::calendar::TIMESTAMP_NOEND,
                Some(temporal::decode::Special::Early) => temporal::calendar::TIMESTAMP_NOBEGIN,
                Some(temporal::decode::Special::Epoch) => {
                    -temporal::calendar::UNIX_TO_POSTGRES_SECS
                        * temporal::calendar::USECS_PER_SEC
                }
                None => temporal::calendar::fields_to_timestamp(&decoded.fields, None)
                    .ok_or_else(|| {
                        CassieError::from(temporal::range_error(format!(
                            "timestamp out of range: \"{text}\""
                        )))
                    })?,
            };
            TypedValue::Timestamp(temporal::calendar::round_timestamp(timestamp, precision))
        }
        ExtendedType::TimestampTz { precision } => {
            let decoded = temporal::decode::decode_datetime_with_check(text, session, &mut check)
                .map_err(|error| decode_error("timestamp with time zone", text, error))?;
            let timestamp = match decoded.special {
                Some(temporal::decode::Special::Late) => temporal::calendar::TIMESTAMP_NOEND,
                Some(temporal::decode::Special::Early) => temporal::calendar::TIMESTAMP_NOBEGIN,
                Some(temporal::decode::Special::Epoch) => {
                    -temporal::calendar::UNIX_TO_POSTGRES_SECS
                        * temporal::calendar::USECS_PER_SEC
                }
                None => temporal::calendar::fields_to_timestamp(
                    &decoded.fields,
                    Some(decoded.zone_west),
                )
                .ok_or_else(|| {
                    CassieError::from(temporal::range_error(format!(
                        "timestamp out of range: \"{text}\""
                    )))
                })?,
            };
            TypedValue::TimestampTz(temporal::calendar::round_timestamp(timestamp, precision))
        }
        ExtendedType::Interval { typmod } => {
            let range = if typmod < 0 {
                temporal::interval::FULL_RANGE
            } else {
                (typmod >> 16) & 0x7FFF
            };
            let value = temporal::interval::parse_interval_with_check(
                text,
                range,
                session.interval_style,
                &mut check,
            )
            .map_err(|error| decode_error("interval", text, error))?;
            TypedValue::Interval(value.apply_typmod(typmod).map_err(|_| {
                CassieError::from(temporal::range_error("interval out of range"))
            })?)
        }
        _ => {
            return Err(CassieError::TypeInput(TypeInputError::new(
                "42846",
                format!("cannot cast type text to {}", kind.name()),
            )))
        }
    };
    check()?;
    Ok(value)
}

fn decode_error(
    type_name: &str,
    text: &str,
    error: TokenizeError<CassieError>,
) -> CassieError {
    match error {
        TokenizeError::Decode(error) => CassieError::from(temporal::input_error(
            type_name,
            text,
            &error,
        )),
        TokenizeError::Check(error) => error,
    }
}

fn parse_date(text: &str, decoded: temporal::decode::Decoded) -> Result<i32, TypeInputError> {
    match decoded.special {
        Some(temporal::decode::Special::Late) => return Ok(temporal::calendar::DATE_NOEND),
        Some(temporal::decode::Special::Early) => return Ok(temporal::calendar::DATE_NOBEGIN),
        Some(temporal::decode::Special::Epoch) => return Ok(-10_957),
        None => {}
    }
    let fields = decoded.fields;
    let out_of_range = || temporal::range_error(format!("date out of range: \"{text}\""));
    if !temporal::calendar::is_valid_julian(fields.year, fields.month, fields.day) {
        return Err(out_of_range());
    }
    let date = temporal::calendar::date2j(fields.year, fields.month, fields.day)
        - i64::from(temporal::calendar::POSTGRES_EPOCH_JDATE);
    if !temporal::calendar::is_valid_date(date) {
        return Err(out_of_range());
    }
    i32::try_from(date).map_err(|_| out_of_range())
}

#[cfg(test)]
mod tests {
    use super::parse_text;
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::extended::temporal::{self, TemporalSession};
    use crate::types::extended::ExtendedType;
    use crate::types::typed::TypedValue;

    const INPUT: &str = "2024-01-15 12:00:00 UTC";

    fn controls(budget: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            std::time::Instant::now(),
        )
    }

    #[test]
    fn should_admit_temporal_token_scratch_at_exact_budget_and_release_it() {
        // Arrange
        let required = temporal::tokens::tokenization_memory_bound(INPUT).expect("bound");
        let exact = controls(required);
        let below = controls(required - 1);
        let session = TemporalSession::default();

        // Act
        let accepted = parse_text(
            ExtendedType::TimestampTz { precision: None },
            INPUT,
            &session,
            &exact,
        );
        let rejected = parse_text(
            ExtendedType::TimestampTz { precision: None },
            INPUT,
            &session,
            &below,
        );
        let exact_after = exact.current_query_memory_bytes();
        let below_after = below.current_query_memory_bytes();

        // Assert
        assert!(matches!(accepted, Ok(TypedValue::TimestampTz(_))));
        assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
        assert_eq!(exact_after, 0);
        assert_eq!(below_after, 0);
    }

    #[test]
    fn should_preserve_query_cancellation_during_temporal_input() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();

        // Act
        let result = parse_text(
            ExtendedType::TimestampTz { precision: None },
            INPUT,
            &TemporalSession::default(),
            &controls,
        );
        let retained = controls.current_query_memory_bytes();

        // Assert
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
        assert_eq!(retained, 0);
    }

    #[test]
    fn should_resolve_session_dynamic_abbreviation_from_the_complete_date() {
        // Arrange
        let session = TemporalSession {
            zone: temporal::zone::lookup_zone("Europe/Moscow").expect("bundled Moscow zone"),
            ..TemporalSession::default()
        };
        let controls = controls(1_000_000);
        let kind = ExtendedType::TimestampTz { precision: None };

        // Act
        let winter_2012 = parse_text(
            kind,
            "2012-01-01 00:00:00 MSK",
            &session,
            &controls,
        );
        let expected_2012 = kind
            .parse_text_in("2012-01-01 00:00:00+04", &session)
            .expect("explicit historical offset");
        let winter_2015 = parse_text(
            kind,
            "2015-01-01 00:00:00 MSK",
            &session,
            &controls,
        );
        let expected_2015 = kind
            .parse_text_in("2015-01-01 00:00:00+03", &session)
            .expect("explicit current offset");

        // Assert
        assert_eq!(winter_2012.expect("MSK 2012"), expected_2012);
        assert_eq!(winter_2015.expect("MSK 2015"), expected_2015);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
