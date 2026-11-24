//! Projection DML statements: `TRUNCATE`, `MERGE`, joined mutation sources
//! and tuple assignments.

use serde::{Deserialize, Serialize};

use super::{
    CommonTableExpression, Expr, IdentifierPath, InsertOverriding, ParsedStatement, QuerySource,
    SelectItem, SelectStatement,
};

/// The row entry that carries `merge_action()` for `MERGE ... RETURNING`.
pub const MERGE_ACTION_COLUMN: &str = "__cassie_merge_action";

/// The parser marker for column defaults in mutation assignments and INSERT cells.
pub const DEFAULT_ASSIGNMENT_MARKER: &str = "__cassie_column_default";

impl<A> Expr<A> {
    /// Whether this expression is a parsed column DEFAULT marker.
    #[must_use]
    pub fn is_default_assignment(&self) -> bool {
        matches!(self, Self::Function(function) if function.name == DEFAULT_ASSIGNMENT_MARKER)
    }
}

/// `TRUNCATE [TABLE] [ONLY] name [, ...] [RESTART | CONTINUE IDENTITY] [CASCADE | RESTRICT]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruncateStatement {
    pub tables: Vec<IdentifierPath>,
    pub restart_identity: bool,
    pub cascade: bool,
}

/// `SET (a, b) = (SELECT ...)`: one row of a scalar subquery assigned to
/// several target columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowSubqueryAssignment<A = ()> {
    pub columns: Vec<String>,
    pub query: Box<ParsedStatement<A>>,
}

/// The aliases `RETURNING WITH (OLD AS o, NEW AS n)` gives the row images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturningAliases {
    pub old: String,
    pub new: String,
}

impl Default for ReturningAliases {
    fn default() -> Self {
        Self {
            old: "old".to_string(),
            new: "new".to_string(),
        }
    }
}

/// A source joined to a mutation target (`UPDATE ... FROM`, `DELETE ... USING`,
/// `MERGE ... USING`).
///
/// The binder fills `query` with a SELECT that projects every source column
/// under its qualified name, so execution never re-binds the source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationSource<A = ()> {
    pub source: QuerySource<A>,
    #[serde(default)]
    pub query: Option<Box<SelectStatement<A>>>,
    /// Qualified output columns of `query`, in projection order.
    #[serde(default)]
    pub fields: Vec<MutationSourceField>,
}

/// One qualified source column (`alias.column`) and its type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationSourceField {
    pub qualifier: String,
    pub column: String,
    pub data_type: cassie_core::types::DataType,
}

impl MutationSourceField {
    #[must_use]
    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.qualifier, self.column)
    }
}

impl<A> MutationSource<A> {
    #[must_use]
    pub const fn new(source: QuerySource) -> Self {
        Self {
            source,
            query: None,
            fields: Vec::new(),
        }
    }
}

/// Which rows a `WHEN` clause of `MERGE` applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeMatch {
    Matched,
    NotMatchedByTarget,
    NotMatchedBySource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeAction<A = ()> {
    Update {
        assignments: Vec<(String, Expr<A>)>,
        #[serde(default)]
        row_subqueries: Vec<RowSubqueryAssignment<A>>,
    },
    Delete,
    DoNothing,
    Insert {
        columns: Vec<String>,
        /// `None` is `DEFAULT VALUES`.
        values: Option<Vec<Expr<A>>>,
        overriding: Option<InsertOverriding>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeClause<A = ()> {
    pub kind: MergeMatch,
    pub condition: Option<Expr<A>>,
    pub action: MergeAction<A>,
}

/// `[WITH ...] MERGE INTO target [AS alias] USING source ON condition WHEN ...`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStatement<A = ()> {
    #[serde(default)]
    pub ctes: Vec<CommonTableExpression<A>>,
    pub table: IdentifierPath,
    pub alias: Option<String>,
    pub source: MutationSource<A>,
    pub on: Expr<A>,
    pub clauses: Vec<MergeClause<A>>,
    pub returning: Vec<SelectItem<A>>,
    #[serde(default)]
    pub returning_aliases: Option<ReturningAliases>,
}
