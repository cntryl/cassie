use serde::{Deserialize, Serialize};

/// One selected built-in extension lifecycle operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtensionAction {
    Create {
        name: String,
        if_not_exists: bool,
        schema: Option<String>,
        version: Option<String>,
    },
    SetSchema {
        name: String,
        schema: String,
    },
    Update {
        name: String,
        version: Option<String>,
    },
    Drop {
        name: String,
        if_exists: bool,
        cascade: bool,
    },
}

/// A finite `CREATE`, `ALTER` or `DROP EXTENSION` statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionStatement {
    pub action: ExtensionAction,
}

impl ExtensionStatement {
    #[must_use]
    pub fn name(&self) -> &str {
        match &self.action {
            ExtensionAction::Create { name, .. }
            | ExtensionAction::SetSchema { name, .. }
            | ExtensionAction::Update { name, .. }
            | ExtensionAction::Drop { name, .. } => name,
        }
    }
}
