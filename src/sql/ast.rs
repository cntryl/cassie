use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::{FieldConstraint, IndexKind};
use crate::types::DataType;

#[path = "ast_schema.rs"]
mod ast_schema;
pub use ast_schema::{AlterTableOperation, AlterTableStatement};

#[path = "ast_query.rs"]
mod ast_query;
pub use ast_query::{
    BinaryOp, Bm25Params, CommonTableExpression, CteQuery, Expr, FunctionCall, JoinKind,
    NullsOrder, OrderExpr, QuerySource, SelectItem, SelectSet, SelectStatement, SetOperator,
    SortDirection, WindowFrame, WindowFrameBound, WindowFrameExclusion, WindowFrameUnit,
    WindowFunctionCall,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedStatement {
    pub raw_sql: String,
    pub statement: QueryStatement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionArg {
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Volatility {
    Immutable,
    Stable,
    Volatile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryStatement {
    Explain(ExplainStatement),
    Select(SelectStatement),
    Show(ShowStatement),
    Set(SetStatement),
    Copy(CopyStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
    Transaction(TransactionStatement),
    CreateTable(CreateTableStatement),
    CreateGraph(CreateGraphStatement),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement),
    CreateSequence(CreateSequenceStatement),
    DropSequence(DropSequenceStatement),
    CreateDatabase(CreateDatabaseStatement),
    DropDatabase(DropDatabaseStatement),
    CreateSchema(CreateSchemaStatement),
    CreateView(CreateViewStatement),
    DropView(DropViewStatement),
    CreateRole(CreateRoleStatement),
    AlterRole(AlterRoleStatement),
    DropRole(DropRoleStatement),
    GrantDatabaseConnect(DatabaseConnectPrivilegeStatement),
    RevokeDatabaseConnect(DatabaseConnectPrivilegeStatement),
    CreateIndex(CreateIndexStatement),
    DropIndex(DropIndexStatement),
    CreateRollup(CreateRollupStatement),
    RefreshRollup(RefreshRollupStatement),
    DropRollup(DropRollupStatement),
    CreateMaterializedProjection(CreateMaterializedProjectionStatement),
    RefreshMaterializedProjection(RefreshMaterializedProjectionStatement),
    DropMaterializedProjection(DropMaterializedProjectionStatement),
    AlterMaterializedProjection(AlterMaterializedProjectionStatement),
    DropMaterializedProjectionVersion(DropMaterializedProjectionVersionStatement),
    VerifyProjection(VerifyProjectionStatement),
    DiffProjection(DiffProjectionStatement),
    CompareProjection(CompareProjectionStatement),
    PlanRepairProjection(PlanRepairProjectionStatement),
    RepairProjection(RepairProjectionStatement),
    CreateRetentionPolicy(CreateRetentionPolicyStatement),
    AlterRetentionPolicy(AlterRetentionPolicyStatement),
    DropRetentionPolicy(DropRetentionPolicyStatement),
    EnforceRetentionPolicy(EnforceRetentionPolicyStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CallProcedure(CallProcedureStatement),
    DropSchema(DropSchemaStatement),
    AlterSchema(AlterSchemaStatement),
}

#[path = "statement_routing.rs"]
mod statement_routing;
pub use statement_routing::{
    CatalogStatement, CatalogStatementRef, ProjectionStatement, ProjectionStatementRef,
    RetentionStatement, RetentionStatementRef, RuntimeStatement, RuntimeStatementRef,
    StatementFamily, StatementRoute, StatementRouteRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyFormat {
    Csv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyStatement {
    pub table: String,
    pub columns: Vec<String>,
    pub format: CopyFormat,
    pub header: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainStatement {
    pub analyze: bool,
    pub statement: Box<ParsedStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionStatement {
    pub action: TransactionAction,
    pub isolation: Option<TransactionIsolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAction {
    Begin,
    Commit,
    Rollback,
    Savepoint { name: String },
    RollbackTo { name: String },
    Release { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionIsolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Select(Box<SelectStatement>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertConflictClause {
    pub target_fields: Vec<String>,
    pub action: InsertConflictAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsertConflictAction {
    DoNothing,
    DoUpdate {
        assignments: Vec<(String, Expr)>,
        filter: Option<Expr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertStatement {
    pub table: String,
    pub columns: Vec<String>,
    pub source: InsertSource,
    pub on_conflict: Option<InsertConflictClause>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatement {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteStatement {
    pub table: String,
    pub filter: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTableStatement {
    pub table: String,
    pub fields: Vec<FieldDefinition>,
    pub if_not_exists: bool,
    pub storage_mode: crate::catalog::CollectionStorageMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGraphStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub node_fields: Vec<FieldDefinition>,
    pub edge_fields: Vec<FieldDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateIndexStatement {
    pub name: String,
    pub table: String,
    pub fields: Vec<String>,
    pub expressions: Vec<Expr>,
    pub include_fields: Vec<String>,
    pub predicate: Option<Expr>,
    pub if_not_exists: bool,
    pub unique: bool,
    pub kind: IndexKind,
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRollupStatement {
    pub name: String,
    pub source: String,
    pub bucket: FunctionCall,
    pub group_by: Vec<Expr>,
    pub aggregates: Vec<SelectItem>,
    pub filter: Option<Expr>,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRollupStatement {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRollupStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMaterializedProjectionStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub options: BTreeMap<String, String>,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshMaterializedProjectionStatement {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropMaterializedProjectionStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterMaterializedProjectionStatement {
    pub name: String,
    pub operation: AlterMaterializedProjectionOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlterMaterializedProjectionOperation {
    BuildVersion,
    ActivateVersion {
        version_id: String,
        unsafe_override: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropMaterializedProjectionVersionStatement {
    pub name: String,
    pub version_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyProjectionStatement {
    pub name: String,
    pub version_id: Option<String>,
    pub mode: ProjectionVerificationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionDiffTarget {
    pub name: String,
    pub version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffProjectionStatement {
    pub left: ProjectionDiffTarget,
    pub right: ProjectionDiffTarget,
    pub limit: Option<usize>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareProjectionStatement {
    pub target: ProjectionDiffTarget,
    pub manifest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRepairProjectionStatement {
    pub target: ProjectionDiffTarget,
    pub scope: ProjectionRepairScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairProjectionStatement {
    pub target: ProjectionDiffTarget,
    pub scope: ProjectionRepairScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionRepairScope {
    Row,
    Range,
    Index,
    ProjectionVersion,
    FullRebuild,
}

impl ProjectionRepairScope {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Range => "range",
            Self::Index => "index",
            Self::ProjectionVersion => "projection_version",
            Self::FullRebuild => "full_rebuild",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionVerificationMode {
    MetadataOnly,
    HashesOnly,
    IndexesOnly,
    Full,
}

impl ProjectionVerificationMode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MetadataOnly => "metadata_only",
            Self::HashesOnly => "hashes_only",
            Self::IndexesOnly => "indexes_only",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRetentionPolicyStatement {
    pub name: String,
    pub collection: String,
    pub timestamp_field: String,
    pub retention_duration: String,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterRetentionPolicyStatement {
    pub name: String,
    pub retention_duration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRetentionPolicyStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceRetentionPolicyStatement {
    pub name: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFunctionStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub return_type: DataType,
    pub volatility: Volatility,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropFunctionStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProcedureStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub args: Vec<FunctionArg>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropProcedureStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallProcedureStatement {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowStatement {
    pub variable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetStatement {
    pub variable: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropIndexStatement {
    pub name: String,
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropTableStatement {
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSequenceStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub data_type: DataType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropSequenceStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSchemaStatement {
    pub schema: String,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDatabaseStatement {
    pub name: String,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropDatabaseStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropSchemaStatement {
    pub schema: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterSchemaStatement {
    pub schema: String,
    pub operation: AlterSchemaOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlterSchemaOperation {
    RenameTo { schema: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateViewStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropViewStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoleStatement {
    pub name: String,
    pub if_not_exists: bool,
    pub login: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlterRoleStatement {
    pub name: String,
    pub login: Option<bool>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRoleStatement {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConnectPrivilegeStatement {
    pub database: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<FieldConstraint>,
}
