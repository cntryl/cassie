use super::MapAttachment;
use crate::ast::{
    FieldDefinition, JsonBehavior, JsonTableColumn, JsonTableNested, JsonTableSpec,
    JsonTableValueColumn, MergeAction, MergeClause, MergeStatement, MutationSource, OrderExpr,
    ParsedStatement, PrepareStatement, QuerySource, QueryStatement, RowComparison,
    RowSubqueryAssignment, SelectItem, SelectSet, SelectStatement, SessionObjectStatement,
    SubqueryExpr, SubqueryKind, TableFunctionForm, UpdateStatement, UpdateTarget,
    WindowFunctionCall,
};
use crate::declarations::{ConstraintCheck, FieldConstraint};

impl MapAttachment for JsonBehavior<()> {
    type Rebound<B> = JsonBehavior<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Unspecified => JsonBehavior::<B>::Unspecified,
            Self::Null => JsonBehavior::<B>::Null,
            Self::Error => JsonBehavior::<B>::Error,
            Self::EmptyArray => JsonBehavior::<B>::EmptyArray,
            Self::EmptyObject => JsonBehavior::<B>::EmptyObject,
            Self::Default(value0) => JsonBehavior::<B>::Default(value0.rebind_attachment::<B>()),
        }
    }
}

impl MapAttachment for JsonTableColumn<()> {
    type Rebound<B> = JsonTableColumn<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Ordinality { name } => JsonTableColumn::<B>::Ordinality { name },
            Self::Value(value0) => JsonTableColumn::<B>::Value(value0.rebind_attachment::<B>()),
            Self::Exists(value0) => JsonTableColumn::<B>::Exists(value0),
            Self::Nested(value0) => JsonTableColumn::<B>::Nested(value0.rebind_attachment::<B>()),
        }
    }
}

impl MapAttachment for JsonTableNested<()> {
    type Rebound<B> = JsonTableNested<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        JsonTableNested {
            path: self.path,
            path_name: self.path_name,
            columns: self.columns.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for JsonTableSpec<()> {
    type Rebound<B> = JsonTableSpec<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        JsonTableSpec {
            path: self.path,
            path_name: self.path_name,
            passing: self.passing,
            columns: self.columns.rebind_attachment::<B>(),
            on_error: self.on_error,
        }
    }
}

impl MapAttachment for JsonTableValueColumn<()> {
    type Rebound<B> = JsonTableValueColumn<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        JsonTableValueColumn {
            name: self.name,
            data_type: self.data_type,
            format_json: self.format_json,
            path: self.path,
            wrapper: self.wrapper,
            quotes: self.quotes,
            on_empty: self.on_empty.rebind_attachment::<B>(),
            on_error: self.on_error.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for MergeAction<()> {
    type Rebound<B> = MergeAction<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Update {
                assignments,
                row_subqueries,
            } => MergeAction::<B>::Update {
                assignments: assignments.rebind_attachment::<B>(),
                row_subqueries: row_subqueries.rebind_attachment::<B>(),
            },
            Self::Delete => MergeAction::<B>::Delete,
            Self::DoNothing => MergeAction::<B>::DoNothing,
            Self::Insert {
                columns,
                values,
                overriding,
            } => MergeAction::<B>::Insert {
                columns,
                values: values.rebind_attachment::<B>(),
                overriding,
            },
        }
    }
}

impl MapAttachment for MergeClause<()> {
    type Rebound<B> = MergeClause<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        MergeClause {
            kind: self.kind,
            condition: self.condition.rebind_attachment::<B>(),
            action: self.action.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for MergeStatement<()> {
    type Rebound<B> = MergeStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        MergeStatement {
            ctes: self.ctes.rebind_attachment::<B>(),
            table: self.table,
            alias: self.alias,
            source: self.source.rebind_attachment::<B>(),
            on: self.on.rebind_attachment::<B>(),
            clauses: self.clauses.rebind_attachment::<B>(),
            returning: self.returning.rebind_attachment::<B>(),
            returning_aliases: self.returning_aliases,
        }
    }
}

impl MapAttachment for MutationSource<()> {
    type Rebound<B> = MutationSource<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        MutationSource {
            source: self.source.rebind_attachment::<B>(),
            query: self.query.rebind_attachment::<B>(),
            fields: self.fields,
        }
    }
}

impl MapAttachment for OrderExpr<()> {
    type Rebound<B> = OrderExpr<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        OrderExpr {
            expr: self.expr.rebind_attachment::<B>(),
            direction: self.direction,
            nulls: self.nulls,
        }
    }
}

impl MapAttachment for ParsedStatement<()> {
    type Rebound<B> = ParsedStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        ParsedStatement {
            raw_sql: self.raw_sql,
            statement: self.statement.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for PrepareStatement<()> {
    type Rebound<B> = PrepareStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        PrepareStatement {
            name: self.name,
            parameter_types: self.parameter_types,
            statement_sql: self.statement_sql,
            statement: self.statement.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for QuerySource<()> {
    type Rebound<B> = QuerySource<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Values { rows } => QuerySource::<B>::Values {
                rows: rows.rebind_attachment::<B>(),
            },
            Self::Collection(value0) => QuerySource::<B>::Collection(value0),
            Self::Cte(value0) => QuerySource::<B>::Cte(value0),
            Self::TableSample {
                source,
                method,
                rate,
                repeatable,
            } => QuerySource::<B>::TableSample {
                source: source.rebind_attachment::<B>(),
                method,
                rate: rate.rebind_attachment::<B>(),
                repeatable: repeatable.rebind_attachment::<B>(),
            },
            Self::Aliased {
                source,
                alias,
                column_aliases,
            } => QuerySource::<B>::Aliased {
                source: source.rebind_attachment::<B>(),
                alias,
                column_aliases,
            },
            Self::TableFunction {
                name,
                function,
                lateral,
                with_ordinality,
                form,
            } => QuerySource::<B>::TableFunction {
                name,
                function: function.rebind_attachment::<B>(),
                lateral,
                with_ordinality,
                form: form.rebind_attachment::<B>(),
            },
            Self::Subquery {
                alias,
                select,
                lateral,
            } => QuerySource::<B>::Subquery {
                alias,
                select: select.rebind_attachment::<B>(),
                lateral,
            },
            Self::Join {
                left,
                right,
                kind,
                on,
            } => QuerySource::<B>::Join {
                left: left.rebind_attachment::<B>(),
                right: right.rebind_attachment::<B>(),
                kind,
                on: on.rebind_attachment::<B>(),
            },
            Self::SingleRow => QuerySource::<B>::SingleRow,
        }
    }
}

impl MapAttachment for QueryStatement<()> {
    type Rebound<B> = QueryStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Explain(value0) => QueryStatement::<B>::Explain(value0.rebind_attachment::<B>()),
            Self::Select(value0) => QueryStatement::<B>::Select(value0.rebind_attachment::<B>()),
            Self::Show(value0) => QueryStatement::<B>::Show(value0),
            Self::Set(value0) => QueryStatement::<B>::Set(value0),
            Self::Copy(value0) => QueryStatement::<B>::Copy(value0),
            Self::Insert(value0) => QueryStatement::<B>::Insert(value0.rebind_attachment::<B>()),
            Self::Update(value0) => QueryStatement::<B>::Update(value0.rebind_attachment::<B>()),
            Self::Delete(value0) => QueryStatement::<B>::Delete(value0.rebind_attachment::<B>()),
            Self::Truncate(value0) => QueryStatement::<B>::Truncate(value0),
            Self::Merge(value0) => QueryStatement::<B>::Merge(value0.rebind_attachment::<B>()),
            Self::Transaction(value0) => QueryStatement::<B>::Transaction(value0),
            Self::SessionObject(value0) => {
                QueryStatement::<B>::SessionObject(value0.rebind_attachment::<B>())
            }
            Self::CreateTable(value0) => {
                QueryStatement::<B>::CreateTable(value0.rebind_attachment::<B>())
            }
            Self::DropTable(value0) => QueryStatement::<B>::DropTable(value0),
            Self::AlterTable(value0) => {
                QueryStatement::<B>::AlterTable(value0.rebind_attachment::<B>())
            }
            Self::CreateSequence(value0) => QueryStatement::<B>::CreateSequence(value0),
            Self::AlterSequence(value0) => QueryStatement::<B>::AlterSequence(value0),
            Self::DropSequence(value0) => QueryStatement::<B>::DropSequence(value0),
            Self::CreateDatabase(value0) => QueryStatement::<B>::CreateDatabase(value0),
            Self::DropDatabase(value0) => QueryStatement::<B>::DropDatabase(value0),
            Self::CreateSchema(value0) => QueryStatement::<B>::CreateSchema(value0),
            Self::CommentOn(value0) => QueryStatement::<B>::CommentOn(value0),
            Self::CreateView(value0) => QueryStatement::<B>::CreateView(value0),
            Self::DropView(value0) => QueryStatement::<B>::DropView(value0),
            Self::CreateRole(value0) => QueryStatement::<B>::CreateRole(value0),
            Self::AlterRole(value0) => QueryStatement::<B>::AlterRole(value0),
            Self::DropRole(value0) => QueryStatement::<B>::DropRole(value0),
            Self::GrantDatabaseConnect(value0) => QueryStatement::<B>::GrantDatabaseConnect(value0),
            Self::RevokeDatabaseConnect(value0) => {
                QueryStatement::<B>::RevokeDatabaseConnect(value0)
            }
            Self::CreateIndex(value0) => {
                QueryStatement::<B>::CreateIndex(value0.rebind_attachment::<B>())
            }
            Self::DropIndex(value0) => QueryStatement::<B>::DropIndex(value0),
            Self::AlterIndex(value0) => QueryStatement::<B>::AlterIndex(value0),
            Self::Reindex(value0) => QueryStatement::<B>::Reindex(value0),
            Self::CreateRollup(value0) => {
                QueryStatement::<B>::CreateRollup(value0.rebind_attachment::<B>())
            }
            Self::RefreshRollup(value0) => QueryStatement::<B>::RefreshRollup(value0),
            Self::DropRollup(value0) => QueryStatement::<B>::DropRollup(value0),
            Self::CreateMaterializedProjection(value0) => {
                QueryStatement::<B>::CreateMaterializedProjection(value0)
            }
            Self::RefreshMaterializedProjection(value0) => {
                QueryStatement::<B>::RefreshMaterializedProjection(value0)
            }
            Self::DropMaterializedProjection(value0) => {
                QueryStatement::<B>::DropMaterializedProjection(value0)
            }
            Self::CreateMaterializedView(value0) => {
                QueryStatement::<B>::CreateMaterializedView(value0)
            }
            Self::RefreshMaterializedView(value0) => {
                QueryStatement::<B>::RefreshMaterializedView(value0)
            }
            Self::DropMaterializedView(value0) => QueryStatement::<B>::DropMaterializedView(value0),
            Self::RenameMaterializedView(value0) => {
                QueryStatement::<B>::RenameMaterializedView(value0)
            }
            Self::AlterMaterializedProjection(value0) => {
                QueryStatement::<B>::AlterMaterializedProjection(value0)
            }
            Self::DropMaterializedProjectionVersion(value0) => {
                QueryStatement::<B>::DropMaterializedProjectionVersion(value0)
            }
            Self::VerifyProjection(value0) => QueryStatement::<B>::VerifyProjection(value0),
            Self::DiffProjection(value0) => QueryStatement::<B>::DiffProjection(value0),
            Self::CompareProjection(value0) => QueryStatement::<B>::CompareProjection(value0),
            Self::PlanRepairProjection(value0) => QueryStatement::<B>::PlanRepairProjection(value0),
            Self::RepairProjection(value0) => QueryStatement::<B>::RepairProjection(value0),
            Self::CreateRetentionPolicy(value0) => {
                QueryStatement::<B>::CreateRetentionPolicy(value0)
            }
            Self::AlterRetentionPolicy(value0) => QueryStatement::<B>::AlterRetentionPolicy(value0),
            Self::DropRetentionPolicy(value0) => QueryStatement::<B>::DropRetentionPolicy(value0),
            Self::EnforceRetentionPolicy(value0) => {
                QueryStatement::<B>::EnforceRetentionPolicy(value0)
            }
            Self::CreateFunction(value0) => QueryStatement::<B>::CreateFunction(value0),
            Self::DropFunction(value0) => QueryStatement::<B>::DropFunction(value0),
            Self::CreateProcedure(value0) => QueryStatement::<B>::CreateProcedure(value0),
            Self::DropProcedure(value0) => QueryStatement::<B>::DropProcedure(value0),
            Self::CallProcedure(value0) => {
                QueryStatement::<B>::CallProcedure(value0.rebind_attachment::<B>())
            }
            Self::Extension(value0) => QueryStatement::<B>::Extension(value0),
            Self::Type(value0) => QueryStatement::<B>::Type(value0),
            Self::PropertyGraph(value0) => QueryStatement::<B>::PropertyGraph(value0),
            Self::DropSchema(value0) => QueryStatement::<B>::DropSchema(value0),
            Self::AlterSchema(value0) => QueryStatement::<B>::AlterSchema(value0),
        }
    }
}

impl MapAttachment for RowComparison<()> {
    type Rebound<B> = RowComparison<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        RowComparison {
            left: self.left.rebind_attachment::<B>(),
            op: self.op,
            right: self.right.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for RowSubqueryAssignment<()> {
    type Rebound<B> = RowSubqueryAssignment<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        RowSubqueryAssignment {
            columns: self.columns,
            query: self.query.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for SelectItem<()> {
    type Rebound<B> = SelectItem<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Wildcard => SelectItem::<B>::Wildcard,
            Self::Column { name, alias } => SelectItem::<B>::Column { name, alias },
            Self::Function { function, alias } => SelectItem::<B>::Function {
                function: function.rebind_attachment::<B>(),
                alias,
            },
            Self::Expr { expr, alias } => SelectItem::<B>::Expr {
                expr: expr.rebind_attachment::<B>(),
                alias,
            },
            Self::WindowFunction { function, alias } => SelectItem::<B>::WindowFunction {
                function: function.rebind_attachment::<B>(),
                alias,
            },
        }
    }
}

impl MapAttachment for SelectSet<()> {
    type Rebound<B> = SelectSet<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        SelectSet {
            operator: self.operator,
            right: self.right.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for SelectStatement<()> {
    type Rebound<B> = SelectStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        SelectStatement {
            source: self.source.rebind_attachment::<B>(),
            ctes: self.ctes.rebind_attachment::<B>(),
            recursive: self.recursive,
            distinct: self.distinct,
            distinct_on: self.distinct_on.rebind_attachment::<B>(),
            projection: self.projection.rebind_attachment::<B>(),
            filter: self.filter.rebind_attachment::<B>(),
            group_by: self.group_by.rebind_attachment::<B>(),
            grouping_subtotal: self.grouping_subtotal,
            having: self.having.rebind_attachment::<B>(),
            order: self.order.rebind_attachment::<B>(),
            limit: self.limit.rebind_attachment::<B>(),
            offset: self.offset.rebind_attachment::<B>(),
            set: self.set.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for SessionObjectStatement<()> {
    type Rebound<B> = SessionObjectStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Discard(value0) => SessionObjectStatement::<B>::Discard(value0),
            Self::Prepare(value0) => {
                SessionObjectStatement::<B>::Prepare(value0.rebind_attachment::<B>())
            }
            Self::Execute(value0) => SessionObjectStatement::<B>::Execute(value0),
            Self::Deallocate(value0) => SessionObjectStatement::<B>::Deallocate(value0),
            Self::DeclareCursor(value0) => {
                SessionObjectStatement::<B>::DeclareCursor(value0.rebind_attachment::<B>())
            }
            Self::Fetch(value0) => SessionObjectStatement::<B>::Fetch(value0),
            Self::CloseCursor(value0) => SessionObjectStatement::<B>::CloseCursor(value0),
        }
    }
}

impl MapAttachment for SubqueryExpr<()> {
    type Rebound<B> = SubqueryExpr<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        SubqueryExpr {
            kind: self.kind.rebind_attachment::<B>(),
            query: self.query.rebind_attachment::<B>(),
            data_type: self.data_type,
            outcome: None,
        }
    }
}

impl MapAttachment for SubqueryKind<()> {
    type Rebound<B> = SubqueryKind<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Scalar => SubqueryKind::<B>::Scalar,
            Self::Array => SubqueryKind::<B>::Array,
            Self::Row { left, op } => SubqueryKind::<B>::Row {
                left: left.rebind_attachment::<B>(),
                op,
            },
            Self::Quantified {
                left,
                op,
                all,
                membership,
                negated,
            } => SubqueryKind::<B>::Quantified {
                left: left.rebind_attachment::<B>(),
                op,
                all,
                membership,
                negated,
            },
        }
    }
}

impl MapAttachment for TableFunctionForm<()> {
    type Rebound<B> = TableFunctionForm<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Columns(value0) => TableFunctionForm::<B>::Columns(value0),
            Self::JsonTable(value0) => {
                TableFunctionForm::<B>::JsonTable(value0.rebind_attachment::<B>())
            }
            Self::GraphTable(value0) => {
                TableFunctionForm::<B>::GraphTable(value0.rebind_attachment::<B>())
            }
            Self::GraphElements(value0) => TableFunctionForm::<B>::GraphElements(value0),
            Self::Typed(value0) => TableFunctionForm::<B>::Typed(value0),
        }
    }
}

impl MapAttachment for UpdateStatement<()> {
    type Rebound<B> = UpdateStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        UpdateStatement {
            table: self.table,
            assignments: self.assignments.rebind_attachment::<B>(),
            array_targets: self.array_targets.rebind_attachment::<B>(),
            filter: self.filter.rebind_attachment::<B>(),
            returning: self.returning.rebind_attachment::<B>(),
            alias: self.alias,
            from: self.from.rebind_attachment::<B>(),
            row_subqueries: self.row_subqueries.rebind_attachment::<B>(),
            returning_aliases: self.returning_aliases,
            ctes: self.ctes.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for UpdateTarget<()> {
    type Rebound<B> = UpdateTarget<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        UpdateTarget {
            assignment_index: self.assignment_index,
            subscripts: self.subscripts.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for WindowFunctionCall<()> {
    type Rebound<B> = WindowFunctionCall<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        WindowFunctionCall {
            name: self.name,
            args: self.args.rebind_attachment::<B>(),
            filter: self.filter.rebind_attachment::<B>(),
            partition_by: self.partition_by.rebind_attachment::<B>(),
            order_by: self.order_by.rebind_attachment::<B>(),
            frame: self.frame,
        }
    }
}

impl MapAttachment for FieldDefinition<()> {
    type Rebound<B> = FieldDefinition<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        FieldDefinition {
            name: self.name,
            data_type: self.data_type,
            constraints: self.constraints.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for ConstraintCheck<()> {
    type Rebound<B> = ConstraintCheck<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        ConstraintCheck {
            field: self.field,
            operator: self.operator,
            value: self.value,
            bound_expression: self.bound_expression.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for FieldConstraint<()> {
    type Rebound<B> = FieldConstraint<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        FieldConstraint {
            definition_semantics_version: self.definition_semantics_version,
            field: self.field,
            primary_key_name: self.primary_key_name,
            primary_key_ordinal: self.primary_key_ordinal,
            unique_name: self.unique_name,
            unique_ordinal: self.unique_ordinal,
            constraint_index_name: self.constraint_index_name,
            constraint_index_include_fields: self.constraint_index_include_fields,
            constraint_index_options: self.constraint_index_options,
            check_name: self.check_name,
            foreign_key_name: self.foreign_key_name,
            foreign_key_ordinal: self.foreign_key_ordinal,
            foreign_key_on_delete: self.foreign_key_on_delete,
            foreign_key_on_update: self.foreign_key_on_update,
            not_null: self.not_null,
            not_null_ownership: self.not_null_ownership,
            unique: self.unique,
            unique_null_policy: self.unique_null_policy,
            primary_key: self.primary_key,
            default_value: self.default_value,
            default_expression: self.default_expression,
            default_sequence: self.default_sequence,
            default_sequence_owned: self.default_sequence_owned,
            check: self.check.rebind_attachment::<B>(),
            references_table: self.references_table,
            references_field: self.references_field,
            identity: self.identity,
            identity_options: self.identity_options,
            storage: self.storage,
            compression: self.compression,
        }
    }
}
