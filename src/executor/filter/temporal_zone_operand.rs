//! Operand-aware session-zone abbreviation resolution for timestamp timezone().
use super::{type_error, QueryError};
use crate::runtime::QueryExecutionControls;
use crate::types::extended::temporal::functions as temporal;
use crate::types::extended::temporal::zone::{lookup_zone, Zone};
use crate::types::extended::temporal::TemporalSession;
use crate::types::extended::TypeInputError;
use crate::types::typed::TypedValue;
use crate::types::Value;

pub(super) fn resolve(
    zone: &Value,
    operand: &Value,
    session: &TemporalSession,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<Zone>, QueryError> {
    resolve_with_observer(zone, operand, session, controls, || {})
}

fn resolve_with_observer(
    zone: &Value,
    operand: &Value,
    session: &TemporalSession,
    controls: Option<&QueryExecutionControls>,
    mut after_search_check: impl FnMut(),
) -> Result<Option<Zone>, QueryError> {
    let Value::String(name) = zone else {
        return Ok(None);
    };
    let probe = match operand {
        Value::Typed(TypedValue::Timestamp(local)) => temporal::utc_of(*local, &session.zone),
        Value::Typed(TypedValue::TimestampTz(instant)) => *instant,
        _ => return Ok(None),
    };
    for chunk in name.as_bytes().chunks(4096) {
        if let Some(controls) = controls {
            controls.check_interruption().map_err(QueryError::Cassie)?;
        }
        if !chunk.iter().all(u8::is_ascii_alphabetic) {
            return Ok(None);
        }
    }
    let global = session.abbreviations.lookup_with_index(name);
    if let (Some(controls), Some((index, east, _))) = (controls, global) {
        if controls
            .temporal_abbreviation_absent(session, index)
            .map_err(QueryError::Cassie)?
        {
            return Ok(Some(Zone::Fixed {
                east,
                name: String::new(),
            }));
        }
    }
    let mut check = || {
        after_search_check();
        controls.map_or(Ok(()), |controls| {
            controls.check_interruption().map_err(QueryError::Cassie)
        })
    };
    if let Some((east, _)) = session
        .zone
        .abbreviation_offset_with_checks(name, probe, &mut check)?
    {
        return Ok(Some(Zone::Fixed {
            east,
            name: String::new(),
        }));
    }
    if let Some((index, east, _)) = global {
        if let Some(controls) = controls {
            controls
                .mark_temporal_abbreviation_absent(session, index)
                .map_err(QueryError::Cassie)?;
        }
        return Ok(Some(Zone::Fixed {
            east,
            name: String::new(),
        }));
    }
    if let Some(zone) = lookup_zone(name) {
        return Ok(Some(zone));
    }
    Err(type_error(TypeInputError {
        sqlstate: "22023",
        message: format!("time zone \"{name}\" not recognized"),
    }))
}

#[cfg(test)]
mod tests {
    use super::resolve_with_observer;
    use crate::app::CassieError;
    use crate::config::CassieRuntimeLimits;
    use crate::executor::QueryError;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::extended::temporal::{
        calendar::{fields_to_timestamp, Fields},
        zone::lookup_zone,
        TemporalSession,
    };
    use crate::types::typed::TypedValue;
    use crate::types::Value;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn should_cancel_during_actual_session_zone_transition_search() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let session = TemporalSession {
            zone: lookup_zone("Europe/London").expect("London"),
            ..TemporalSession::default()
        };
        let local = fields_to_timestamp(
            &Fields {
                year: 2400,
                month: 1,
                day: 1,
                ..Fields::default()
            },
            None,
        )
        .expect("timestamp");
        let mut checks = 0;

        // Act
        let result = resolve_with_observer(
            &Value::String("unknownabbr".to_owned()),
            &Value::Typed(TypedValue::Timestamp(local)),
            &session,
            Some(&controls),
            || {
                checks += 1;
                if checks == 2 {
                    cancellation.cancel();
                }
            },
        );

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(checks, 2);
    }
    #[test]
    fn should_cache_only_a_complete_miss_for_the_same_session() {
        // Arrange
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            QueryCancellationHandle::new(),
        );
        let session = Arc::new(TemporalSession {
            zone: lookup_zone("America/New_York").expect("New York"),
            ..TemporalSession::default()
        });
        controls.install_temporal_session(Arc::clone(&session));
        let zone = Value::String("UTC".to_owned());
        let operand = Value::Typed(TypedValue::Timestamp(0));
        let mut checks = 0;

        // Act
        resolve_with_observer(&zone, &operand, &session, Some(&controls), || checks += 1)
            .expect("first resolution");
        let first_checks = checks;
        resolve_with_observer(&zone, &operand, &session, Some(&controls), || checks += 1)
            .expect("cached resolution");
        let replacement = Arc::new(TemporalSession::default());
        controls.install_temporal_session(Arc::clone(&replacement));

        // Assert
        assert!(first_checks > 1);
        assert_eq!(checks, first_checks);
        let (index, _, _) = session.abbreviations.lookup_with_index("UTC").expect("UTC");
        assert!(!controls
            .temporal_abbreviation_absent(&replacement, index)
            .expect("cache"));
        controls
            .mark_temporal_abbreviation_absent(&session, index)
            .expect("stale mark");
        assert!(!controls
            .temporal_abbreviation_absent(&replacement, index)
            .expect("cache"));
    }

    #[test]
    fn should_not_cache_a_cancelled_transition_scan() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let session = Arc::new(TemporalSession {
            zone: lookup_zone("America/New_York").expect("New York"),
            ..TemporalSession::default()
        });
        controls.install_temporal_session(Arc::clone(&session));
        let mut checks = 0;
        let (index, _, _) = session.abbreviations.lookup_with_index("UTC").expect("UTC");

        // Act
        let result = resolve_with_observer(
            &Value::String("UTC".to_owned()),
            &Value::Typed(TypedValue::Timestamp(0)),
            &session,
            Some(&controls),
            || {
                checks += 1;
                if checks == 2 {
                    cancellation.cancel();
                }
            },
        );

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(checks, 2);
        assert!(!controls.temporal_cache_contains_for_test(&session, index));
    }

    #[test]
    fn should_not_publish_an_old_sessions_miss_after_replacement() {
        // Arrange
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            QueryCancellationHandle::new(),
        );
        let old = Arc::new(TemporalSession {
            zone: lookup_zone("America/New_York").expect("New York"),
            ..TemporalSession::default()
        });
        let replacement = Arc::new(TemporalSession {
            zone: lookup_zone("Asia/Shanghai").expect("Shanghai"),
            ..TemporalSession::default()
        });
        controls.install_temporal_session(Arc::clone(&old));
        let mut checks = 0;
        let (index, _, _) = old.abbreviations.lookup_with_index("UTC").expect("UTC");

        // Act
        resolve_with_observer(
            &Value::String("UTC".to_owned()),
            &Value::Typed(TypedValue::Timestamp(0)),
            &old,
            Some(&controls),
            || {
                checks += 1;
                if checks == 2 {
                    controls.install_temporal_session(Arc::clone(&replacement));
                }
            },
        )
        .expect("old session resolution");

        // Assert
        assert!(checks >= 2);
        assert!(!controls
            .temporal_abbreviation_absent(&replacement, index)
            .expect("cache"));
    }
    #[test]
    fn should_use_custom_zone_rules_even_when_the_name_matches_a_known_zone() {
        // Arrange
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            QueryCancellationHandle::new(),
        );
        let session = Arc::new(TemporalSession {
            zone: crate::types::extended::temporal::zone::Zone::Rules {
                tz: jiff::tz::TimeZone::posix("UTC-8").expect("custom rules"),
                name: "America/New_York".to_owned(),
            },
            ..TemporalSession::default()
        });
        controls.install_temporal_session(Arc::clone(&session));

        // Act
        let actual = resolve_with_observer(
            &Value::String("UTC".to_owned()),
            &Value::Typed(TypedValue::Timestamp(0)),
            &session,
            Some(&controls),
            || {},
        )
        .expect("resolution");

        // Assert
        assert!(matches!(
            actual,
            Some(crate::types::extended::temporal::zone::Zone::Fixed { east: 28_800, .. })
        ));
    }
}
