use serde::{Deserialize, Serialize};

use super::{IdentifierComponent, IdentifierPath};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommentOnObjectKind {
    Schema,
    Table,
    Column,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentOnStatement {
    pub kind: CommentOnObjectKind,
    pub object: IdentifierPath,
    pub column: Option<IdentifierComponent>,
    /// `None` clears an existing comment; `Some("")` is normalized to the same operation.
    pub description: Option<String>,
}
