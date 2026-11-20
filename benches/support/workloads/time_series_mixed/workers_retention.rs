//! Scheduled retention driving and progress checks for the mixed workload.

use super::*;

pub(super) fn enforce_retention(cassie: &Cassie, session: &CassieSession) -> MaintenanceResult {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(180);
    let mut completed_batches = 0_u64;
    loop {
        if started.elapsed() >= Duration::from_secs(180) {
            return MaintenanceResult {
                name: "retention_enforcement".to_owned(),
                elapsed: started.elapsed(),
                completed_batches,
                completed: false,
            };
        }
        cassie
            .execute_sql(
                session,
                &format!(
                    "ENFORCE RETENTION POLICY {RETENTION_POLICY} DUE AT '{}'",
                    model::RETENTION_AT
                ),
                Vec::new(),
            )
            .expect("execute one scheduled retention batch");
        completed_batches = completed_batches.saturating_add(1);
        if !retention_pending_for(cassie, session) {
            return MaintenanceResult {
                name: "retention_enforcement".to_owned(),
                elapsed: started.elapsed(),
                completed_batches,
                completed: true,
            };
        }
        if Instant::now() >= deadline {
            return MaintenanceResult {
                name: "retention_enforcement".to_owned(),
                elapsed: started.elapsed(),
                completed_batches,
                completed: false,
            };
        }
    }
}

pub(super) fn retention_pending(fixture: &Fixture) -> Result<bool, CassieError> {
    let result = fixture.execute(&retention_pending_sql())?;
    Ok(pending_cutoff_is_set(&result))
}

fn retention_pending_for(cassie: &Cassie, session: &CassieSession) -> bool {
    let result = cassie
        .execute_sql(session, &retention_pending_sql(), Vec::new())
        .expect("read retention pending state");
    pending_cutoff_is_set(result.rows())
}

/// The catalog stores the policy under its database-qualified name.
fn retention_pending_sql() -> String {
    format!(
        "SELECT pending_cutoff FROM pg_catalog.pg_retention_policies \
         WHERE policy_name LIKE '%.{RETENTION_POLICY}'"
    )
}

/// A missing policy row must never read as "retention finished".
fn pending_cutoff_is_set(rows: &[Vec<Value>]) -> bool {
    let row = rows
        .first()
        .expect("the retention policy is listed in pg_retention_policies");
    !row[0].is_null()
}
