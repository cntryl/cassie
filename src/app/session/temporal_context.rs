//! The per-statement temporal view of a session: zone, styles and clocks.
use std::sync::Arc;

use super::CassieSession;
use crate::types::extended::temporal::session::{
    current_timestamp, parse_datestyle, DateOrder, DateOutput, IntervalStyle, TemporalSession,
};
use crate::types::extended::temporal::zone::{lookup_zone, AbbreviationSet, Zone};

impl CassieSession {
    /// Builds the temporal session a statement starting now evaluates under.
    /// `now()` is fixed for an explicit transaction; otherwise it is the
    /// statement start.
    pub(crate) fn temporal_session(&self) -> TemporalSession {
        let statement_start = current_timestamp();
        let transaction_start = if self.is_transaction_active() {
            let generation = self.transaction_generation();
            let mut clock = self.transaction_clock.lock();
            match *clock {
                Some((started, at)) if started == generation => at,
                _ => {
                    *clock = Some((generation, statement_start));
                    statement_start
                }
            }
        } else {
            statement_start
        };
        let setting = |name: &str| self.setting(name).unwrap_or_default();
        let zone = setting_zone(&setting("timezone")).unwrap_or_else(Zone::utc);
        let (date_output, date_order) =
            parse_datestyle(&setting("datestyle"), (DateOutput::Iso, DateOrder::Mdy))
                .unwrap_or((DateOutput::Iso, DateOrder::Mdy));
        TemporalSession {
            zone,
            date_output,
            date_order,
            interval_style: IntervalStyle::parse(&setting("intervalstyle")).unwrap_or_default(),
            abbreviations: AbbreviationSet::parse(&setting("timezone_abbreviations"))
                .unwrap_or_default(),
            transaction_start,
            statement_start,
        }
    }

    /// Installs this session's temporal view into a statement's controls.
    pub(crate) fn install_temporal_session(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) {
        controls.install_temporal_session(Arc::new(self.temporal_session()));
    }
}

/// Resolves a stored `TimeZone` setting: a zone name or a POSIX-style
/// fixed offset such as `<-08>+08`.
#[must_use]
pub(crate) fn setting_zone(name: &str) -> Option<Zone> {
    if let Some(rest) = name.strip_prefix('<') {
        let (abbreviation, posix) = rest.split_once('>')?;
        let east = fixed_offset(abbreviation)?;
        if fixed_offset(posix).map(|west| -west) != Some(east) {
            return None;
        }
        return Some(crate::types::extended::temporal::zone::fixed_zone(east));
    }
    lookup_zone(name)
}

fn fixed_offset(text: &str) -> Option<i32> {
    let (sign, body) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => (1, text),
    };
    let mut parts = body.split(':').map(|part| part.parse::<i32>().ok());
    let hours = parts.next()??;
    let minutes = parts.next().flatten().unwrap_or(0);
    let seconds = parts.next().flatten().unwrap_or(0);
    Some(sign * (hours * 3600 + minutes * 60 + seconds))
}
