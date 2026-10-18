//! Validation of `TimeZone`, `DateStyle`, `IntervalStyle` and
//! `timezone_abbreviations`, canonicalized as PostgreSQL shows them.
use crate::app::CassieError;
use crate::types::extended::temporal::session::{
    datestyle_text, parse_datestyle, DateOrder, DateOutput, IntervalStyle,
};
use crate::types::extended::temporal::zone::{fixed_zone, lookup_zone, AbbreviationSet};

fn invalid(display: &str, value: &str) -> CassieError {
    CassieError::InvalidParameterValue(format!(
        "invalid value for parameter \"{display}\": \"{value}\""
    ))
}

/// Canonicalizes a temporal setting; `None` for other settings.
pub(super) fn canonical(
    name: &str,
    value: &str,
    current: &str,
) -> Option<Result<String, CassieError>> {
    let trimmed = value.trim();
    Some(match name {
        "timezone" => timezone(trimmed).ok_or_else(|| invalid("TimeZone", trimmed)),
        "datestyle" => {
            let current = parse_datestyle(current, (DateOutput::Iso, DateOrder::Mdy))
                .unwrap_or((DateOutput::Iso, DateOrder::Mdy));
            parse_datestyle(trimmed, current)
                .map(|(output, order)| datestyle_text(output, order))
                .map_err(|_| invalid("DateStyle", trimmed))
        }
        "intervalstyle" => IntervalStyle::parse(trimmed)
            .map(|style| style.name().to_string())
            .ok_or_else(|| invalid("IntervalStyle", trimmed)),
        "timezone_abbreviations" => AbbreviationSet::parse(trimmed)
            .map(|set| set.name().to_string())
            .ok_or_else(|| invalid("timezone_abbreviations", trimmed)),
        _ => return None,
    })
}

fn timezone(value: &str) -> Option<String> {
    if let Some(literal) = interval_literal(value) {
        let interval = crate::types::extended::temporal::parse_interval(
            &literal,
            -1,
            crate::types::extended::temporal::default_session(),
        )
        .ok()?;
        if interval.months != 0 || interval.days != 0 {
            return None;
        }
        let east = i32::try_from(interval.micros / 1_000_000).ok()?;
        return Some(fixed_zone(east).name().to_string());
    }
    if let Ok(hours) = value.parse::<f64>() {
        if !(-15.0..=15.0).contains(&hours) {
            return None;
        }
        let east =
            crate::types::extended::temporal::tokens::truncate_f64_i32((hours * 3600.0).round())?;
        return Some(fixed_zone(east).name().to_string());
    }
    if value.starts_with('<') {
        return crate::app::session::setting_zone(value).map(|zone| zone.name().to_string());
    }
    lookup_zone(value).map(|zone| zone.name().to_string())
}

/// The quoted literal of `INTERVAL '...' [qualifier]`.
fn interval_literal(value: &str) -> Option<String> {
    value
        .get(..8)
        .filter(|head| head.eq_ignore_ascii_case("interval"))?;
    let body = value[8..].trim_start();
    let body = body.strip_prefix('\'')?;
    let end = body.find('\'')?;
    Some(body[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn should_canonicalize_temporal_settings_like_postgresql() {
        // Arrange
        let inputs = [
            ("timezone", "america/new_york"),
            ("timezone", "INTERVAL '-08:00' HOUR TO MINUTE"),
            ("timezone", "-7"),
            ("datestyle", "SQL, DMY"),
            ("intervalstyle", "ISO_8601"),
        ];

        // Act
        let values =
            inputs.map(|(name, value)| canonical(name, value, "ISO, MDY").and_then(Result::ok));

        // Assert
        assert_eq!(
            values,
            [
                Some("America/New_York".to_string()),
                Some("<-08>+08".to_string()),
                Some("<-07>+07".to_string()),
                Some("SQL, DMY".to_string()),
                Some("iso_8601".to_string()),
            ]
        );
        assert!(canonical("timezone", "Mars/Olympus", "").is_some_and(|value| value.is_err()));
    }
}
