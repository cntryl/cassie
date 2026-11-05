//! Monotonic minimum-reader admission using the existing schema-family marker.
use super::Midge;
use crate::app::CassieError;
use crate::midge::adapter::key_encoding;
use cntryl_midge::TransactionMode;

pub(super) const V020_LAYOUT_VERSION: &str = "cassie-midge-layout-v2-0.2.0";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Compatibility {
    V1,
    V020,
}

impl Compatibility {
    fn parse(marker: &[u8]) -> Option<Self> {
        match marker {
            value if value == key_encoding::LAYOUT_MARKER_VALUE => Some(Self::V1),
            value if value == V020_LAYOUT_VERSION.as_bytes() => Some(Self::V020),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::V1 => key_encoding::LAYOUT_VERSION,
            Self::V020 => V020_LAYOUT_VERSION,
        }
    }
}

impl Midge {
    pub(super) fn supports_storage_layout_marker(marker: &[u8]) -> bool {
        Compatibility::parse(marker).is_some()
    }

    #[must_use]
    pub(crate) fn supports_storage_layout_version(version: &str) -> bool {
        Self::supports_storage_layout_marker(version.as_bytes())
    }

    pub(crate) fn storage_layout_version(&self) -> Result<&'static str, CassieError> {
        self.publication.ensure_recovered()?;
        let schema = self.ensure_families_ready()?.schema.id();
        let read = self
            .engine
            .begin_tx(schema, TransactionMode::ReadOnly)
            .map_err(CassieError::from)?;
        let marker = read
            .get(&key_encoding::layout_marker_key())
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                Self::missing_layout_marker(crate::midge::adapter::SCHEMA_FAMILY_NAME)
            })?;
        Compatibility::parse(&marker)
            .map(Compatibility::as_str)
            .ok_or_else(|| Self::incompatible_layout_marker(&marker))
    }

    /// Acknowledges the minimum reader before a separate feature transaction publishes.
    /// A failure never authorizes that feature publication, even if the marker is visible.
    pub(crate) fn require_storage_layout_version(&self, version: &str) -> Result<(), CassieError> {
        self.publication.ensure_recovered()?;
        let required = Compatibility::parse(version.as_bytes()).ok_or_else(|| {
            CassieError::Unsupported(format!("unsupported required storage layout '{version}'"))
        })?;
        let schema = self.ensure_families_ready()?.schema.id();
        #[cfg(test)]
        tests::before_marker_lock();
        let _marker_guard = self.compatibility_gate.lock();
        self.publication.ensure_recovered()?;
        let mut transaction = self.begin_raw_adapter_tx(schema, TransactionMode::ReadWrite)?;
        let key = key_encoding::layout_marker_key();
        let marker = transaction
            .get(&key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                Self::missing_layout_marker(crate::midge::adapter::SCHEMA_FAMILY_NAME)
            })?;
        Compatibility::parse(&marker).ok_or_else(|| Self::incompatible_layout_marker(&marker))?;
        if required == Compatibility::V1 {
            return Ok(());
        }

        // Reassert even an existing v2 marker: a previous OutcomeUnknown may
        // have made these bytes visible without acknowledging their durability.
        // A nonempty commit reaches Midge's cloud durability finalizer.
        transaction
            .put(key, required.as_str().as_bytes().to_vec(), None)
            .map_err(CassieError::from)?;
        transaction
            .commit(self.write_policy.compatibility_upgrade())
            .map_err(CassieError::from)
    }
}

#[cfg(test)]
#[path = "storage_compatibility/tests.rs"]
mod tests;
