//! Native option readback and bounded runtime memory diagnostics. This module
//! observes the published Midge API; it does not own another storage layer.

use super::{CassieError, Midge};
use cntryl_midge::{MemoryBudget, OpenOptions, Storage};
use std::time::Duration;

/// Actual built engine allocations. Numeric fields are byte counts; these are
/// configuration readbacks, not measured process memory or physical I/O.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NativeMemoryConfiguration {
    pub mode: &'static str,
    pub total: usize,
    pub memtable_limit: usize,
    pub flush_threshold: usize,
    pub transaction_pool: usize,
}

impl NativeMemoryConfiguration {
    pub(super) fn from_options(options: &OpenOptions) -> Self {
        Self {
            mode: match options.storage() {
                Storage::InMemory => "memory",
                Storage::Local { .. } => "local",
                Storage::Cloud { .. } => "cloud",
                Storage::CloudSimulated { .. } => "cloud-simulated",
            },
            total: options.memory_budget_bytes(),
            memtable_limit: options.memtable_size_limit(),
            flush_threshold: options.memtable_flush_threshold(),
            transaction_pool: options.transaction_memory_pool_size(),
        }
    }
}

/// Observations returned by Midge's runtime metrics facade.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NativeMemoryRuntime {
    pub total_memtable_bytes: usize,
    pub active_memtables: usize,
    pub immutable_memtables: usize,
    pub write_stalled: bool,
    pub active_snapshots: usize,
    pub current_sequence: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NativeMemoryDiagnostics {
    pub configuration: NativeMemoryConfiguration,
    pub runtime: NativeMemoryRuntime,
}

pub(super) fn in_memory_options(
    budget: Option<usize>,
    memtable: Option<usize>,
) -> Result<OpenOptions, CassieError> {
    let mut builder = OpenOptions::in_memory();
    if let Some(bytes) = budget {
        builder = builder.memory_budget(MemoryBudget::Bytes(bytes));
    }
    if let Some(bytes) = memtable {
        builder = builder.with_memtable_size_limit(bytes);
    }
    builder.build().map_err(CassieError::from)
}

impl Midge {
    /// Actual options captured immediately before opening this engine.
    #[must_use]
    pub fn native_memory_configuration(&self) -> &NativeMemoryConfiguration {
        &self.native_memory_configuration
    }

    /// Reads native runtime observations without writing or scanning SQL data.
    /// The published runtime request has a five-second hang guard.
    ///
    /// # Errors
    ///
    /// Returns the native metrics error or timeout; no observation is fabricated.
    pub fn native_memory_diagnostics(&self) -> Result<NativeMemoryDiagnostics, CassieError> {
        let runtime = self
            .engine
            .metrics()
            .get_runtime_metrics_with_timeout(Duration::from_secs(5))
            .map_err(CassieError::from)?;
        Ok(NativeMemoryDiagnostics {
            configuration: self.native_memory_configuration.clone(),
            runtime: NativeMemoryRuntime {
                total_memtable_bytes: runtime.total_memtable_bytes,
                active_memtables: runtime.active_memtables,
                immutable_memtables: runtime.immutable_memtables,
                write_stalled: runtime.write_stalled,
                active_snapshots: runtime.active_snapshots,
                current_sequence: runtime.current_sequence,
            },
        })
    }
}

#[cfg(test)]
mod tests;
