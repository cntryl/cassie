use std::collections::BTreeMap;

use crate::performance_benchmarks::{FixtureClass, PerformanceBenchmarkScenario};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationUnit {
    Batch,
    Cancel,
    Candidate,
    Comparison,
    Connection,
    Distance,
    Document,
    Event,
    Fetch,
    Key,
    Lookup,
    Message,
    Operation,
    Parameter,
    Plan,
    Predicate,
    Probe,
    Query,
    Request,
    ResultRow,
    Row,
    Score,
    SourceRow,
    Startup,
    Statement,
    Text,
    TopKMaintenance,
    Workflow,
}

impl OperationUnit {
    pub const ALL: [Self; 28] = [
        Self::Batch,
        Self::Cancel,
        Self::Candidate,
        Self::Comparison,
        Self::Connection,
        Self::Distance,
        Self::Document,
        Self::Event,
        Self::Fetch,
        Self::Key,
        Self::Lookup,
        Self::Message,
        Self::Operation,
        Self::Parameter,
        Self::Plan,
        Self::Predicate,
        Self::Probe,
        Self::Query,
        Self::Request,
        Self::ResultRow,
        Self::Row,
        Self::Score,
        Self::SourceRow,
        Self::Startup,
        Self::Statement,
        Self::Text,
        Self::TopKMaintenance,
        Self::Workflow,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Cancel => "cancel",
            Self::Candidate => "candidate",
            Self::Comparison => "comparison",
            Self::Connection => "connection",
            Self::Distance => "distance",
            Self::Document => "document",
            Self::Event => "event",
            Self::Fetch => "fetch",
            Self::Key => "key",
            Self::Lookup => "lookup",
            Self::Message => "message",
            Self::Operation => "operation",
            Self::Parameter => "parameter",
            Self::Plan => "plan",
            Self::Predicate => "predicate",
            Self::Probe => "probe",
            Self::Query => "query",
            Self::Request => "request",
            Self::ResultRow => "result_row",
            Self::Row => "row",
            Self::Score => "score",
            Self::SourceRow => "source_row",
            Self::Startup => "startup",
            Self::Statement => "statement",
            Self::Text => "text",
            Self::TopKMaintenance => "top_k_maintenance",
            Self::Workflow => "workflow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureDeclaration {
    class: FixtureClass,
    rows: usize,
    identity: String,
}

impl FixtureDeclaration {
    #[must_use]
    pub fn new(class: FixtureClass, rows: usize, identity: impl Into<String>) -> Self {
        Self {
            class,
            rows,
            identity: identity.into(),
        }
    }

    #[must_use]
    pub const fn class(&self) -> FixtureClass {
        self.class
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCaseDeclaration {
    fixture: FixtureDeclaration,
    operation_unit: OperationUnit,
}

impl RuntimeCaseDeclaration {
    #[must_use]
    pub const fn new(fixture: FixtureDeclaration, operation_unit: OperationUnit) -> Self {
        Self {
            fixture,
            operation_unit,
        }
    }

    #[must_use]
    pub const fn fixture(&self) -> &FixtureDeclaration {
        &self.fixture
    }

    #[must_use]
    pub const fn operation_unit(&self) -> OperationUnit {
        self.operation_unit
    }
}

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

/// Validates the runtime declaration against the scenario registry.
///
/// # Errors
///
/// Returns an error when the declaration is absent or disagrees with the registry.
pub fn validate_runtime_case_contract(
    scenario: &PerformanceBenchmarkScenario,
    declaration: Option<&RuntimeCaseDeclaration>,
) -> Result<(), String> {
    let declaration = declaration.ok_or_else(|| {
        format!(
            "scenario {} has no runtime fixture and operation declaration",
            scenario.scenario_id
        )
    })?;
    let fixture = declaration.fixture();
    if fixture.identity().trim().is_empty() {
        return Err(format!(
            "scenario {} has an empty runtime fixture identity",
            scenario.scenario_id
        ));
    }
    if fixture.class() != scenario.fixture_class {
        return Err(format!(
            "scenario {} runtime fixture class {:?} does not match registered {:?}",
            scenario.scenario_id,
            fixture.class(),
            scenario.fixture_class
        ));
    }
    if fixture.rows() != scenario.fixture_rows {
        return Err(format!(
            "scenario {} runtime fixture rows {} do not match registered {}",
            scenario.scenario_id,
            fixture.rows(),
            scenario.fixture_rows
        ));
    }
    if declaration.operation_unit().as_str() != scenario.operation_unit {
        return Err(format!(
            "scenario {} runtime operation unit '{}' does not match registered '{}'",
            scenario.scenario_id,
            declaration.operation_unit().as_str(),
            scenario.operation_unit
        ));
    }
    Ok(())
}
