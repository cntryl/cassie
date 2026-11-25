use serde::{Deserialize, Serialize};

use super::{
    AlterIndexStatement, AlterMaterializedProjectionStatement, AlterRetentionPolicyStatement,
    AlterRoleStatement, AlterSchemaStatement, AlterSequenceStatement, AlterTableStatement,
    CallProcedureStatement, CommentOnStatement, CompareProjectionStatement, CopyStatement,
    CreateDatabaseStatement, CreateFunctionStatement, CreateIndexStatement,
    CreateMaterializedProjectionStatement, CreateMaterializedViewStatement,
    CreateProcedureStatement, CreateRetentionPolicyStatement, CreateRoleStatement,
    CreateRollupStatement, CreateSchemaStatement, CreateSequenceStatement, CreateTableStatement,
    CreateViewStatement, DatabaseConnectPrivilegeStatement, DeleteStatement,
    DiffProjectionStatement, DropDatabaseStatement, DropFunctionStatement, DropIndexStatement,
    DropMaterializedProjectionStatement, DropMaterializedProjectionVersionStatement,
    DropMaterializedViewStatement, DropProcedureStatement, DropRetentionPolicyStatement,
    DropRoleStatement, DropRollupStatement, DropSchemaStatement, DropSequenceStatement,
    DropTableStatement, DropViewStatement, EnforceRetentionPolicyStatement, ExplainStatement,
    ExtensionStatement, InsertStatement, MergeStatement, PlanRepairProjectionStatement,
    PropertyGraphStatement, QueryStatement, RefreshMaterializedProjectionStatement,
    RefreshMaterializedViewStatement, RefreshRollupStatement, ReindexStatement,
    RenameMaterializedViewStatement, RepairProjectionStatement, SelectStatement,
    SessionObjectStatement, SetStatement, ShowStatement, TransactionStatement, TruncateStatement,
    TypeStatement, UpdateStatement, VerifyProjectionStatement,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementFamily {
    Runtime,
    Catalog,
    Projection,
    Retention,
}

#[derive(Debug)]
pub enum StatementRouteRef<'a, A = ()> {
    Runtime(RuntimeStatementRef<'a, A>),
    Catalog(CatalogStatementRef<'a, A>),
    Projection(ProjectionStatementRef<'a, A>),
    Retention(RetentionStatementRef<'a>),
}

#[derive(Debug)]
pub enum RuntimeStatementRef<'a, A = ()> {
    Explain(&'a ExplainStatement<A>),
    Select(&'a SelectStatement<A>),
    Show(&'a ShowStatement),
    Set(&'a SetStatement),
    Copy(&'a CopyStatement),
    Insert(&'a InsertStatement<A>),
    Update(&'a UpdateStatement<A>),
    Delete(&'a DeleteStatement<A>),
    Truncate(&'a TruncateStatement),
    Merge(&'a MergeStatement<A>),
    Transaction(&'a TransactionStatement),
    SessionObject(&'a SessionObjectStatement<A>),
}

#[derive(Debug)]
pub enum CatalogStatementRef<'a, A = ()> {
    CommentOn(&'a CommentOnStatement),
    CreateTable(&'a CreateTableStatement<A>),
    DropTable(&'a DropTableStatement),
    AlterTable(&'a AlterTableStatement<A>),
    CreateSequence(&'a CreateSequenceStatement),
    AlterSequence(&'a AlterSequenceStatement),
    DropSequence(&'a DropSequenceStatement),
    CreateDatabase(&'a CreateDatabaseStatement),
    DropDatabase(&'a DropDatabaseStatement),
    CreateSchema(&'a CreateSchemaStatement),
    DropSchema(&'a DropSchemaStatement),
    AlterSchema(&'a AlterSchemaStatement),
    CreateView(&'a CreateViewStatement),
    DropView(&'a DropViewStatement),
    CreateRole(&'a CreateRoleStatement),
    AlterRole(&'a AlterRoleStatement),
    DropRole(&'a DropRoleStatement),
    GrantDatabaseConnect(&'a DatabaseConnectPrivilegeStatement),
    RevokeDatabaseConnect(&'a DatabaseConnectPrivilegeStatement),
    CreateFunction(&'a CreateFunctionStatement),
    DropFunction(&'a DropFunctionStatement),
    CreateProcedure(&'a CreateProcedureStatement),
    DropProcedure(&'a DropProcedureStatement),
    CallProcedure(&'a CallProcedureStatement<A>),
    Extension(&'a ExtensionStatement),
    Type(&'a TypeStatement),
    PropertyGraph(&'a PropertyGraphStatement),
    CreateIndex(&'a CreateIndexStatement<A>),
    DropIndex(&'a DropIndexStatement),
    AlterIndex(&'a AlterIndexStatement),
    Reindex(&'a ReindexStatement),
}

#[derive(Debug)]
pub enum ProjectionStatementRef<'a, A = ()> {
    CreateRollup(&'a CreateRollupStatement<A>),
    RefreshRollup(&'a RefreshRollupStatement),
    DropRollup(&'a DropRollupStatement),
    CreateMaterializedProjection(&'a CreateMaterializedProjectionStatement),
    RefreshMaterializedProjection(&'a RefreshMaterializedProjectionStatement),
    DropMaterializedProjection(&'a DropMaterializedProjectionStatement),
    CreateMaterializedView(&'a CreateMaterializedViewStatement),
    RefreshMaterializedView(&'a RefreshMaterializedViewStatement),
    DropMaterializedView(&'a DropMaterializedViewStatement),
    RenameMaterializedView(&'a RenameMaterializedViewStatement),
    AlterMaterializedProjection(&'a AlterMaterializedProjectionStatement),
    DropMaterializedProjectionVersion(&'a DropMaterializedProjectionVersionStatement),
    VerifyProjection(&'a VerifyProjectionStatement),
    DiffProjection(&'a DiffProjectionStatement),
    CompareProjection(&'a CompareProjectionStatement),
    PlanRepairProjection(&'a PlanRepairProjectionStatement),
    RepairProjection(&'a RepairProjectionStatement),
}

#[derive(Debug, Clone, Copy)]
pub enum RetentionStatementRef<'a> {
    CreateRetentionPolicy(&'a CreateRetentionPolicyStatement),
    AlterRetentionPolicy(&'a AlterRetentionPolicyStatement),
    DropRetentionPolicy(&'a DropRetentionPolicyStatement),
    EnforceRetentionPolicy(&'a EnforceRetentionPolicyStatement),
}

pub enum StatementRoute<A = ()> {
    Runtime(Box<RuntimeStatement<A>>),
    Catalog(CatalogStatement<A>),
    Projection(ProjectionStatement<A>),
    Retention(RetentionStatement),
}

pub enum RuntimeStatement<A = ()> {
    Explain(ExplainStatement<A>),
    Select(Box<SelectStatement<A>>),
    Show(ShowStatement),
    Set(SetStatement),
    Copy(CopyStatement),
    Insert(InsertStatement<A>),
    Update(UpdateStatement<A>),
    Delete(DeleteStatement<A>),
    Truncate(TruncateStatement),
    Merge(Box<MergeStatement<A>>),
    Transaction(TransactionStatement),
    SessionObject(SessionObjectStatement<A>),
}

pub enum CatalogStatement<A = ()> {
    CommentOn(CommentOnStatement),
    CreateTable(CreateTableStatement<A>),
    DropTable(DropTableStatement),
    AlterTable(AlterTableStatement<A>),
    CreateSequence(CreateSequenceStatement),
    AlterSequence(AlterSequenceStatement),
    DropSequence(DropSequenceStatement),
    CreateDatabase(CreateDatabaseStatement),
    DropDatabase(DropDatabaseStatement),
    CreateSchema(CreateSchemaStatement),
    DropSchema(DropSchemaStatement),
    AlterSchema(AlterSchemaStatement),
    CreateView(CreateViewStatement),
    DropView(DropViewStatement),
    CreateRole(CreateRoleStatement),
    AlterRole(AlterRoleStatement),
    DropRole(DropRoleStatement),
    GrantDatabaseConnect(DatabaseConnectPrivilegeStatement),
    RevokeDatabaseConnect(DatabaseConnectPrivilegeStatement),
    CreateFunction(CreateFunctionStatement),
    DropFunction(DropFunctionStatement),
    CreateProcedure(CreateProcedureStatement),
    DropProcedure(DropProcedureStatement),
    CallProcedure(CallProcedureStatement<A>),
    Extension(ExtensionStatement),
    Type(TypeStatement),
    PropertyGraph(PropertyGraphStatement),
    CreateIndex(CreateIndexStatement<A>),
    DropIndex(DropIndexStatement),
    AlterIndex(AlterIndexStatement),
    Reindex(ReindexStatement),
}

pub enum ProjectionStatement<A = ()> {
    CreateRollup(CreateRollupStatement<A>),
    RefreshRollup(RefreshRollupStatement),
    DropRollup(DropRollupStatement),
    CreateMaterializedProjection(CreateMaterializedProjectionStatement),
    RefreshMaterializedProjection(RefreshMaterializedProjectionStatement),
    DropMaterializedProjection(DropMaterializedProjectionStatement),
    CreateMaterializedView(CreateMaterializedViewStatement),
    RefreshMaterializedView(RefreshMaterializedViewStatement),
    DropMaterializedView(DropMaterializedViewStatement),
    RenameMaterializedView(RenameMaterializedViewStatement),
    AlterMaterializedProjection(AlterMaterializedProjectionStatement),
    DropMaterializedProjectionVersion(DropMaterializedProjectionVersionStatement),
    VerifyProjection(VerifyProjectionStatement),
    DiffProjection(DiffProjectionStatement),
    CompareProjection(CompareProjectionStatement),
    PlanRepairProjection(PlanRepairProjectionStatement),
    RepairProjection(RepairProjectionStatement),
}

pub enum RetentionStatement {
    CreateRetentionPolicy(CreateRetentionPolicyStatement),
    AlterRetentionPolicy(AlterRetentionPolicyStatement),
    DropRetentionPolicy(DropRetentionPolicyStatement),
    EnforceRetentionPolicy(EnforceRetentionPolicyStatement),
}

impl<A> Copy for StatementRouteRef<'_, A> {}

impl<A> Clone for StatementRouteRef<'_, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for RuntimeStatementRef<'_, A> {}

impl<A> Clone for RuntimeStatementRef<'_, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for CatalogStatementRef<'_, A> {}

impl<A> Clone for CatalogStatementRef<'_, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for ProjectionStatementRef<'_, A> {}

impl<A> Clone for ProjectionStatementRef<'_, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> QueryStatement<A> {
    #[must_use]
    pub const fn family(&self) -> StatementFamily {
        match self {
            Self::Explain(_)
            | Self::Select(_)
            | Self::Show(_)
            | Self::Set(_)
            | Self::Copy(_)
            | Self::Insert(_)
            | Self::Update(_)
            | Self::Delete(_)
            | Self::Truncate(_)
            | Self::Merge(_)
            | Self::Transaction(_)
            | Self::SessionObject(_) => StatementFamily::Runtime,
            Self::CreateTable(_)
            | Self::DropTable(_)
            | Self::AlterTable(_)
            | Self::CreateSequence(_)
            | Self::AlterSequence(_)
            | Self::DropSequence(_)
            | Self::CreateDatabase(_)
            | Self::DropDatabase(_)
            | Self::CreateSchema(_)
            | Self::CommentOn(_)
            | Self::DropSchema(_)
            | Self::AlterSchema(_)
            | Self::CreateView(_)
            | Self::DropView(_)
            | Self::CreateRole(_)
            | Self::AlterRole(_)
            | Self::DropRole(_)
            | Self::GrantDatabaseConnect(_)
            | Self::RevokeDatabaseConnect(_)
            | Self::CreateFunction(_)
            | Self::DropFunction(_)
            | Self::CreateProcedure(_)
            | Self::DropProcedure(_)
            | Self::CallProcedure(_)
            | Self::Extension(_)
            | Self::Type(_)
            | Self::PropertyGraph(_)
            | Self::CreateIndex(_)
            | Self::DropIndex(_)
            | Self::AlterIndex(_)
            | Self::Reindex(_) => StatementFamily::Catalog,
            Self::CreateRollup(_)
            | Self::RefreshRollup(_)
            | Self::DropRollup(_)
            | Self::CreateMaterializedProjection(_)
            | Self::RefreshMaterializedProjection(_)
            | Self::DropMaterializedProjection(_)
            | Self::CreateMaterializedView(_)
            | Self::RefreshMaterializedView(_)
            | Self::DropMaterializedView(_)
            | Self::RenameMaterializedView(_)
            | Self::AlterMaterializedProjection(_)
            | Self::DropMaterializedProjectionVersion(_)
            | Self::VerifyProjection(_)
            | Self::DiffProjection(_)
            | Self::CompareProjection(_)
            | Self::PlanRepairProjection(_)
            | Self::RepairProjection(_) => StatementFamily::Projection,
            Self::CreateRetentionPolicy(_)
            | Self::AlterRetentionPolicy(_)
            | Self::DropRetentionPolicy(_)
            | Self::EnforceRetentionPolicy(_) => StatementFamily::Retention,
        }
    }

    #[must_use]
    pub fn route(&self) -> StatementRouteRef<'_, A> {
        match self.family() {
            StatementFamily::Runtime => StatementRouteRef::Runtime(self.runtime_route_ref()),
            StatementFamily::Catalog => StatementRouteRef::Catalog(self.catalog_route_ref()),
            StatementFamily::Projection => {
                StatementRouteRef::Projection(self.projection_route_ref())
            }
            StatementFamily::Retention => StatementRouteRef::Retention(self.retention_route_ref()),
        }
    }

    #[must_use]
    pub fn into_route(self) -> StatementRoute<A> {
        match self.family() {
            StatementFamily::Runtime => {
                StatementRoute::Runtime(Box::new(self.into_runtime_route()))
            }
            StatementFamily::Catalog => StatementRoute::Catalog(self.into_catalog_route()),
            StatementFamily::Projection => StatementRoute::Projection(self.into_projection_route()),
            StatementFamily::Retention => StatementRoute::Retention(self.into_retention_route()),
        }
    }

    fn runtime_route_ref(&self) -> RuntimeStatementRef<'_, A> {
        match self {
            Self::Explain(statement) => RuntimeStatementRef::Explain(statement),
            Self::Select(statement) => RuntimeStatementRef::Select(statement),
            Self::Show(statement) => RuntimeStatementRef::Show(statement),
            Self::Set(statement) => RuntimeStatementRef::Set(statement),
            Self::Copy(statement) => RuntimeStatementRef::Copy(statement),
            Self::Insert(statement) => RuntimeStatementRef::Insert(statement),
            Self::Update(statement) => RuntimeStatementRef::Update(statement),
            Self::Delete(statement) => RuntimeStatementRef::Delete(statement),
            Self::Truncate(statement) => RuntimeStatementRef::Truncate(statement),
            Self::Merge(statement) => RuntimeStatementRef::Merge(statement),
            Self::Transaction(statement) => RuntimeStatementRef::Transaction(statement),
            Self::SessionObject(statement) => RuntimeStatementRef::SessionObject(statement),
            _ => unreachable!("statement family should match runtime route"),
        }
    }

    fn catalog_route_ref(&self) -> CatalogStatementRef<'_, A> {
        match self {
            Self::CommentOn(statement) => CatalogStatementRef::CommentOn(statement),
            Self::CreateTable(statement) => CatalogStatementRef::CreateTable(statement),
            Self::DropTable(statement) => CatalogStatementRef::DropTable(statement),
            Self::AlterTable(statement) => CatalogStatementRef::AlterTable(statement),
            Self::CreateSequence(statement) => CatalogStatementRef::CreateSequence(statement),
            Self::AlterSequence(statement) => CatalogStatementRef::AlterSequence(statement),
            Self::DropSequence(statement) => CatalogStatementRef::DropSequence(statement),
            Self::CreateDatabase(statement) => CatalogStatementRef::CreateDatabase(statement),
            Self::DropDatabase(statement) => CatalogStatementRef::DropDatabase(statement),
            Self::CreateSchema(statement) => CatalogStatementRef::CreateSchema(statement),
            Self::DropSchema(statement) => CatalogStatementRef::DropSchema(statement),
            Self::AlterSchema(statement) => CatalogStatementRef::AlterSchema(statement),
            Self::CreateView(statement) => CatalogStatementRef::CreateView(statement),
            Self::DropView(statement) => CatalogStatementRef::DropView(statement),
            Self::CreateRole(statement) => CatalogStatementRef::CreateRole(statement),
            Self::AlterRole(statement) => CatalogStatementRef::AlterRole(statement),
            Self::DropRole(statement) => CatalogStatementRef::DropRole(statement),
            Self::GrantDatabaseConnect(statement) => {
                CatalogStatementRef::GrantDatabaseConnect(statement)
            }
            Self::RevokeDatabaseConnect(statement) => {
                CatalogStatementRef::RevokeDatabaseConnect(statement)
            }
            Self::CreateFunction(statement) => CatalogStatementRef::CreateFunction(statement),
            Self::DropFunction(statement) => CatalogStatementRef::DropFunction(statement),
            Self::CreateProcedure(statement) => CatalogStatementRef::CreateProcedure(statement),
            Self::DropProcedure(statement) => CatalogStatementRef::DropProcedure(statement),
            Self::CallProcedure(statement) => CatalogStatementRef::CallProcedure(statement),
            Self::Extension(statement) => CatalogStatementRef::Extension(statement),
            Self::Type(statement) => CatalogStatementRef::Type(statement),
            Self::PropertyGraph(statement) => CatalogStatementRef::PropertyGraph(statement),
            Self::CreateIndex(statement) => CatalogStatementRef::CreateIndex(statement),
            Self::DropIndex(statement) => CatalogStatementRef::DropIndex(statement),
            Self::AlterIndex(statement) => CatalogStatementRef::AlterIndex(statement),
            Self::Reindex(statement) => CatalogStatementRef::Reindex(statement),
            _ => unreachable!("statement family should match catalog route"),
        }
    }

    fn projection_route_ref(&self) -> ProjectionStatementRef<'_, A> {
        match self {
            Self::CreateRollup(statement) => ProjectionStatementRef::CreateRollup(statement),
            Self::RefreshRollup(statement) => ProjectionStatementRef::RefreshRollup(statement),
            Self::DropRollup(statement) => ProjectionStatementRef::DropRollup(statement),
            Self::CreateMaterializedProjection(statement) => {
                ProjectionStatementRef::CreateMaterializedProjection(statement)
            }
            Self::RefreshMaterializedProjection(statement) => {
                ProjectionStatementRef::RefreshMaterializedProjection(statement)
            }
            Self::DropMaterializedProjection(statement) => {
                ProjectionStatementRef::DropMaterializedProjection(statement)
            }
            Self::CreateMaterializedView(statement) => {
                ProjectionStatementRef::CreateMaterializedView(statement)
            }
            Self::RefreshMaterializedView(statement) => {
                ProjectionStatementRef::RefreshMaterializedView(statement)
            }
            Self::DropMaterializedView(statement) => {
                ProjectionStatementRef::DropMaterializedView(statement)
            }
            Self::RenameMaterializedView(statement) => {
                ProjectionStatementRef::RenameMaterializedView(statement)
            }
            Self::AlterMaterializedProjection(statement) => {
                ProjectionStatementRef::AlterMaterializedProjection(statement)
            }
            Self::DropMaterializedProjectionVersion(statement) => {
                ProjectionStatementRef::DropMaterializedProjectionVersion(statement)
            }
            Self::VerifyProjection(statement) => {
                ProjectionStatementRef::VerifyProjection(statement)
            }
            Self::DiffProjection(statement) => ProjectionStatementRef::DiffProjection(statement),
            Self::CompareProjection(statement) => {
                ProjectionStatementRef::CompareProjection(statement)
            }
            Self::PlanRepairProjection(statement) => {
                ProjectionStatementRef::PlanRepairProjection(statement)
            }
            Self::RepairProjection(statement) => {
                ProjectionStatementRef::RepairProjection(statement)
            }
            _ => unreachable!("statement family should match projection route"),
        }
    }

    fn retention_route_ref(&self) -> RetentionStatementRef<'_> {
        match self {
            Self::CreateRetentionPolicy(statement) => {
                RetentionStatementRef::CreateRetentionPolicy(statement)
            }
            Self::AlterRetentionPolicy(statement) => {
                RetentionStatementRef::AlterRetentionPolicy(statement)
            }
            Self::DropRetentionPolicy(statement) => {
                RetentionStatementRef::DropRetentionPolicy(statement)
            }
            Self::EnforceRetentionPolicy(statement) => {
                RetentionStatementRef::EnforceRetentionPolicy(statement)
            }
            _ => unreachable!("statement family should match retention route"),
        }
    }

    fn into_runtime_route(self) -> RuntimeStatement<A> {
        match self {
            Self::Explain(statement) => RuntimeStatement::Explain(statement),
            Self::Select(statement) => RuntimeStatement::Select(statement),
            Self::Show(statement) => RuntimeStatement::Show(statement),
            Self::Set(statement) => RuntimeStatement::Set(statement),
            Self::Copy(statement) => RuntimeStatement::Copy(statement),
            Self::Insert(statement) => RuntimeStatement::Insert(statement),
            Self::Update(statement) => RuntimeStatement::Update(statement),
            Self::Delete(statement) => RuntimeStatement::Delete(statement),
            Self::Truncate(statement) => RuntimeStatement::Truncate(statement),
            Self::Merge(statement) => RuntimeStatement::Merge(statement),
            Self::Transaction(statement) => RuntimeStatement::Transaction(statement),
            Self::SessionObject(statement) => RuntimeStatement::SessionObject(statement),
            _ => unreachable!("statement family should match runtime route"),
        }
    }

    fn into_catalog_route(self) -> CatalogStatement<A> {
        match self {
            Self::CommentOn(statement) => CatalogStatement::CommentOn(statement),
            Self::CreateTable(statement) => CatalogStatement::CreateTable(statement),
            Self::DropTable(statement) => CatalogStatement::DropTable(statement),
            Self::AlterTable(statement) => CatalogStatement::AlterTable(statement),
            Self::CreateSequence(statement) => CatalogStatement::CreateSequence(statement),
            Self::AlterSequence(statement) => CatalogStatement::AlterSequence(statement),
            Self::DropSequence(statement) => CatalogStatement::DropSequence(statement),
            Self::CreateDatabase(statement) => CatalogStatement::CreateDatabase(statement),
            Self::DropDatabase(statement) => CatalogStatement::DropDatabase(statement),
            Self::CreateSchema(statement) => CatalogStatement::CreateSchema(statement),
            Self::DropSchema(statement) => CatalogStatement::DropSchema(statement),
            Self::AlterSchema(statement) => CatalogStatement::AlterSchema(statement),
            Self::CreateView(statement) => CatalogStatement::CreateView(statement),
            Self::DropView(statement) => CatalogStatement::DropView(statement),
            Self::CreateRole(statement) => CatalogStatement::CreateRole(statement),
            Self::AlterRole(statement) => CatalogStatement::AlterRole(statement),
            Self::DropRole(statement) => CatalogStatement::DropRole(statement),
            Self::GrantDatabaseConnect(statement) => {
                CatalogStatement::GrantDatabaseConnect(statement)
            }
            Self::RevokeDatabaseConnect(statement) => {
                CatalogStatement::RevokeDatabaseConnect(statement)
            }
            Self::CreateFunction(statement) => CatalogStatement::CreateFunction(statement),
            Self::DropFunction(statement) => CatalogStatement::DropFunction(statement),
            Self::CreateProcedure(statement) => CatalogStatement::CreateProcedure(statement),
            Self::DropProcedure(statement) => CatalogStatement::DropProcedure(statement),
            Self::CallProcedure(statement) => CatalogStatement::CallProcedure(statement),
            Self::Extension(statement) => CatalogStatement::Extension(statement),
            Self::Type(statement) => CatalogStatement::Type(statement),
            Self::PropertyGraph(statement) => CatalogStatement::PropertyGraph(statement),
            Self::CreateIndex(statement) => CatalogStatement::CreateIndex(statement),
            Self::DropIndex(statement) => CatalogStatement::DropIndex(statement),
            Self::AlterIndex(statement) => CatalogStatement::AlterIndex(statement),
            Self::Reindex(statement) => CatalogStatement::Reindex(statement),
            _ => unreachable!("statement family should match catalog route"),
        }
    }

    fn into_projection_route(self) -> ProjectionStatement<A> {
        match self {
            Self::CreateRollup(statement) => ProjectionStatement::CreateRollup(statement),
            Self::RefreshRollup(statement) => ProjectionStatement::RefreshRollup(statement),
            Self::DropRollup(statement) => ProjectionStatement::DropRollup(statement),
            Self::CreateMaterializedProjection(statement) => {
                ProjectionStatement::CreateMaterializedProjection(statement)
            }
            Self::RefreshMaterializedProjection(statement) => {
                ProjectionStatement::RefreshMaterializedProjection(statement)
            }
            Self::DropMaterializedProjection(statement) => {
                ProjectionStatement::DropMaterializedProjection(statement)
            }
            Self::CreateMaterializedView(statement) => {
                ProjectionStatement::CreateMaterializedView(statement)
            }
            Self::RefreshMaterializedView(statement) => {
                ProjectionStatement::RefreshMaterializedView(statement)
            }
            Self::DropMaterializedView(statement) => {
                ProjectionStatement::DropMaterializedView(statement)
            }
            Self::RenameMaterializedView(statement) => {
                ProjectionStatement::RenameMaterializedView(statement)
            }
            Self::AlterMaterializedProjection(statement) => {
                ProjectionStatement::AlterMaterializedProjection(statement)
            }
            Self::DropMaterializedProjectionVersion(statement) => {
                ProjectionStatement::DropMaterializedProjectionVersion(statement)
            }
            Self::VerifyProjection(statement) => ProjectionStatement::VerifyProjection(statement),
            Self::DiffProjection(statement) => ProjectionStatement::DiffProjection(statement),
            Self::CompareProjection(statement) => ProjectionStatement::CompareProjection(statement),
            Self::PlanRepairProjection(statement) => {
                ProjectionStatement::PlanRepairProjection(statement)
            }
            Self::RepairProjection(statement) => ProjectionStatement::RepairProjection(statement),
            _ => unreachable!("statement family should match projection route"),
        }
    }

    fn into_retention_route(self) -> RetentionStatement {
        match self {
            Self::CreateRetentionPolicy(statement) => {
                RetentionStatement::CreateRetentionPolicy(statement)
            }
            Self::AlterRetentionPolicy(statement) => {
                RetentionStatement::AlterRetentionPolicy(statement)
            }
            Self::DropRetentionPolicy(statement) => {
                RetentionStatement::DropRetentionPolicy(statement)
            }
            Self::EnforceRetentionPolicy(statement) => {
                RetentionStatement::EnforceRetentionPolicy(statement)
            }
            _ => unreachable!("statement family should match retention route"),
        }
    }
}
