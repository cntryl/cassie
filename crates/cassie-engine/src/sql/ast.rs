//! Engine-specialized aliases for the syntax crate's AST.
//!
//! Parsing and persisted syntax use the default `()` attachment. Bound and
//! executing engine expressions carry their lazy subquery outcomes here.

pub use cassie_sql::ast::*;

pub use super::subquery_outcome::{SubqueryExprWidthError, SubqueryRows};
pub use super::SubqueryOutcome;

pub type AlterTableOperation = cassie_sql::ast::AlterTableOperation<SubqueryOutcome>;
pub type AlterTableStatement = cassie_sql::ast::AlterTableStatement<SubqueryOutcome>;
pub type ArraySubscript = cassie_sql::ast::ArraySubscript<SubqueryOutcome>;
pub type CallProcedureStatement = cassie_sql::ast::CallProcedureStatement<SubqueryOutcome>;
pub type CommonTableExpression = cassie_sql::ast::CommonTableExpression<SubqueryOutcome>;
pub type CreateIndexStatement = cassie_sql::ast::CreateIndexStatement<SubqueryOutcome>;
pub type CreateRollupStatement = cassie_sql::ast::CreateRollupStatement<SubqueryOutcome>;
pub type CreateTableAsClause = cassie_sql::ast::CreateTableAsClause<SubqueryOutcome>;
pub type CreateTableStatement = cassie_sql::ast::CreateTableStatement<SubqueryOutcome>;
pub type CteCycle = cassie_sql::ast::CteCycle<SubqueryOutcome>;
pub type CteQuery = cassie_sql::ast::CteQuery<SubqueryOutcome>;
pub type DeclareCursorStatement = cassie_sql::ast::DeclareCursorStatement<SubqueryOutcome>;
pub type DeleteStatement = cassie_sql::ast::DeleteStatement<SubqueryOutcome>;
pub type Expr = cassie_sql::ast::Expr<SubqueryOutcome>;
pub type ExplainStatement = cassie_sql::ast::ExplainStatement<SubqueryOutcome>;
pub type FieldDefinition = cassie_sql::ast::FieldDefinition<SubqueryOutcome>;
pub type FunctionCall = cassie_sql::ast::FunctionCall<SubqueryOutcome>;
pub type GraphColumn = cassie_sql::ast::GraphColumn<SubqueryOutcome>;
pub type GraphEdgePattern = cassie_sql::ast::GraphEdgePattern<SubqueryOutcome>;
pub type GraphPathPattern = cassie_sql::ast::GraphPathPattern<SubqueryOutcome>;
pub type GraphPathStep = cassie_sql::ast::GraphPathStep<SubqueryOutcome>;
pub type GraphTable = cassie_sql::ast::GraphTable<SubqueryOutcome>;
pub type GraphVertexPattern = cassie_sql::ast::GraphVertexPattern<SubqueryOutcome>;
pub type InsertConflictAction = cassie_sql::ast::InsertConflictAction<SubqueryOutcome>;
pub type InsertConflictClause = cassie_sql::ast::InsertConflictClause<SubqueryOutcome>;
pub type InsertSource = cassie_sql::ast::InsertSource<SubqueryOutcome>;
pub type InsertStatement = cassie_sql::ast::InsertStatement<SubqueryOutcome>;
pub type JsonBehavior = cassie_sql::ast::JsonBehavior<SubqueryOutcome>;
pub type JsonTableColumn = cassie_sql::ast::JsonTableColumn<SubqueryOutcome>;
pub type JsonTableNested = cassie_sql::ast::JsonTableNested<SubqueryOutcome>;
pub type JsonTableSpec = cassie_sql::ast::JsonTableSpec<SubqueryOutcome>;
pub type JsonTableValueColumn = cassie_sql::ast::JsonTableValueColumn<SubqueryOutcome>;
pub type MergeAction = cassie_sql::ast::MergeAction<SubqueryOutcome>;
pub type MergeClause = cassie_sql::ast::MergeClause<SubqueryOutcome>;
pub type MergeStatement = cassie_sql::ast::MergeStatement<SubqueryOutcome>;
pub type MutationSource = cassie_sql::ast::MutationSource<SubqueryOutcome>;
pub type OrderExpr = cassie_sql::ast::OrderExpr<SubqueryOutcome>;
pub type ParsedStatement = cassie_sql::ast::ParsedStatement<SubqueryOutcome>;
pub type PrepareStatement = cassie_sql::ast::PrepareStatement<SubqueryOutcome>;
pub type QuerySource = cassie_sql::ast::QuerySource<SubqueryOutcome>;
pub type QueryStatement = cassie_sql::ast::QueryStatement<SubqueryOutcome>;
pub type RowComparison = cassie_sql::ast::RowComparison<SubqueryOutcome>;
pub type RowSubqueryAssignment = cassie_sql::ast::RowSubqueryAssignment<SubqueryOutcome>;
pub type SelectItem = cassie_sql::ast::SelectItem<SubqueryOutcome>;
pub type SelectSet = cassie_sql::ast::SelectSet<SubqueryOutcome>;
pub type SelectStatement = cassie_sql::ast::SelectStatement<SubqueryOutcome>;
pub type SessionObjectStatement = cassie_sql::ast::SessionObjectStatement<SubqueryOutcome>;
pub type SubqueryExpr = cassie_sql::ast::SubqueryExpr<SubqueryOutcome>;
pub type SubqueryKind = cassie_sql::ast::SubqueryKind<SubqueryOutcome>;
pub type TableFunctionForm = cassie_sql::ast::TableFunctionForm<SubqueryOutcome>;
pub type UpdateStatement = cassie_sql::ast::UpdateStatement<SubqueryOutcome>;
pub type UpdateTarget = cassie_sql::ast::UpdateTarget<SubqueryOutcome>;
pub type WindowFunctionCall = cassie_sql::ast::WindowFunctionCall<SubqueryOutcome>;
