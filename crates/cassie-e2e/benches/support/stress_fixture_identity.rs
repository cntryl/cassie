//! One fixture identity per benchmark owner and scale.

use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct FixtureIdentityTracker {
    identities: BTreeMap<(String, String), String>,
}

impl FixtureIdentityTracker {
    /// Records one measured fixture identity for an owner and scale.
    ///
    /// Reopening the same persisted fixture is represented by reusing its identity.
    ///
    /// # Errors
    ///
    /// Returns an error when another identity was already measured for the owner and scale.
    pub fn register(
        &mut self,
        owner: &str,
        fixture_scale: &str,
        identity: &str,
    ) -> Result<(), String> {
        let key = (owner.to_string(), fixture_scale.to_string());
        if let Some(existing) = self.identities.get(&key) {
            if existing != identity {
                return Err(format!(
                    "owner {owner} scale {fixture_scale} reused different fixture identities: \
                     '{existing}' then '{identity}'"
                ));
            }
            return Ok(());
        }
        self.identities.insert(key, identity.to_string());
        Ok(())
    }
}
