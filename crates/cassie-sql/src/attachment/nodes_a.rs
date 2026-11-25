use super::MapAttachment;
use crate::ast::{
    AlterTableOperation, AlterTableStatement, ArraySubscript, CallProcedureStatement,
    CommonTableExpression, CreateIndexStatement, CreateRollupStatement, CreateTableAsClause,
    CreateTableStatement, CteCycle, CteQuery, DeclareCursorStatement, DeleteStatement,
    ExplainStatement, Expr, FunctionCall, GraphColumn, GraphEdgePattern, GraphPathPattern,
    GraphPathStep, GraphTable, GraphVertexPattern, InsertConflictAction, InsertConflictClause,
    InsertSource, InsertStatement,
};

impl MapAttachment for (String, Expr<()>) {
    type Rebound<B> = (String, Expr<B>);
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        (self.0, self.1.rebind_attachment::<B>())
    }
}

impl MapAttachment for (Expr<()>, Expr<()>) {
    type Rebound<B> = (Expr<B>, Expr<B>);
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        (
            self.0.rebind_attachment::<B>(),
            self.1.rebind_attachment::<B>(),
        )
    }
}

impl MapAttachment for AlterTableOperation<()> {
    type Rebound<B> = AlterTableOperation<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Multiple { operations } => AlterTableOperation::<B>::Multiple {
                operations: operations.rebind_attachment::<B>(),
            },
            Self::AddColumn {
                field,
                data_type,
                if_not_exists,
                constraints,
            } => AlterTableOperation::<B>::AddColumn {
                field,
                data_type,
                if_not_exists,
                constraints: constraints.rebind_attachment::<B>(),
            },
            Self::AddConstraint { constraints } => AlterTableOperation::<B>::AddConstraint {
                constraints: constraints.rebind_attachment::<B>(),
            },
            Self::AddUniqueConstraintUsingIndex {
                constraint_name,
                index_name,
            } => AlterTableOperation::<B>::AddUniqueConstraintUsingIndex {
                constraint_name,
                index_name,
            },
            Self::DropConstraint { name, if_exists } => {
                AlterTableOperation::<B>::DropConstraint { name, if_exists }
            }
            Self::DropColumn { field, cascade } => {
                AlterTableOperation::<B>::DropColumn { field, cascade }
            }
            Self::RenameColumn { from, to } => AlterTableOperation::<B>::RenameColumn { from, to },
            Self::RenameTo { table } => AlterTableOperation::<B>::RenameTo { table },
            Self::SetSchema { schema } => AlterTableOperation::<B>::SetSchema { schema },
            Self::SetAccessMethod { access_method } => {
                AlterTableOperation::<B>::SetAccessMethod { access_method }
            }
            Self::AlterColumnSetDefault {
                field,
                default_value,
                default_expression,
                default_sequence,
            } => AlterTableOperation::<B>::AlterColumnSetDefault {
                field,
                default_value,
                default_expression,
                default_sequence,
            },
            Self::AlterColumnDropDefault { field } => {
                AlterTableOperation::<B>::AlterColumnDropDefault { field }
            }
            Self::AlterColumnSetNotNull { field } => {
                AlterTableOperation::<B>::AlterColumnSetNotNull { field }
            }
            Self::AlterColumnStorage { field, setting } => {
                AlterTableOperation::<B>::AlterColumnStorage { field, setting }
            }
            Self::AlterColumnDropNotNull { field } => {
                AlterTableOperation::<B>::AlterColumnDropNotNull { field }
            }
            Self::AlterColumnIdentity { field, action } => {
                AlterTableOperation::<B>::AlterColumnIdentity { field, action }
            }
            Self::AlterColumnType {
                field,
                data_type,
                using,
            } => AlterTableOperation::<B>::AlterColumnType {
                field,
                data_type,
                using: using.rebind_attachment::<B>(),
            },
        }
    }
}

impl MapAttachment for AlterTableStatement<()> {
    type Rebound<B> = AlterTableStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        AlterTableStatement {
            table: self.table,
            if_exists: self.if_exists,
            operation: self.operation.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for ArraySubscript<()> {
    type Rebound<B> = ArraySubscript<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Index(value0) => ArraySubscript::<B>::Index(value0.rebind_attachment::<B>()),
            Self::Slice { lower, upper } => ArraySubscript::<B>::Slice {
                lower: lower.rebind_attachment::<B>(),
                upper: upper.rebind_attachment::<B>(),
            },
        }
    }
}

impl MapAttachment for CallProcedureStatement<()> {
    type Rebound<B> = CallProcedureStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CallProcedureStatement {
            name: self.name,
            args: self.args.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for CommonTableExpression<()> {
    type Rebound<B> = CommonTableExpression<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CommonTableExpression {
            name: self.name,
            aliases: self.aliases,
            query: self.query.rebind_attachment::<B>(),
            materialization: self.materialization,
            search: self.search,
            cycle: self.cycle.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for CreateIndexStatement<()> {
    type Rebound<B> = CreateIndexStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CreateIndexStatement {
            name: self.name,
            table: self.table,
            fields: self.fields,
            expressions: self.expressions.rebind_attachment::<B>(),
            include_fields: self.include_fields,
            predicate: self.predicate.rebind_attachment::<B>(),
            if_not_exists: self.if_not_exists,
            unique: self.unique,
            kind: self.kind,
            options: self.options,
        }
    }
}

impl MapAttachment for CreateRollupStatement<()> {
    type Rebound<B> = CreateRollupStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CreateRollupStatement {
            name: self.name,
            source: self.source,
            bucket: self.bucket.rebind_attachment::<B>(),
            group_by: self.group_by.rebind_attachment::<B>(),
            aggregates: self.aggregates.rebind_attachment::<B>(),
            filter: self.filter.rebind_attachment::<B>(),
            if_not_exists: self.if_not_exists,
        }
    }
}

impl MapAttachment for CreateTableAsClause<()> {
    type Rebound<B> = CreateTableAsClause<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CreateTableAsClause {
            query: self.query.rebind_attachment::<B>(),
            column_names: self.column_names,
            with_data: self.with_data,
        }
    }
}

impl MapAttachment for CreateTableStatement<()> {
    type Rebound<B> = CreateTableStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CreateTableStatement {
            table: self.table,
            fields: self.fields.rebind_attachment::<B>(),
            if_not_exists: self.if_not_exists,
            storage_mode: self.storage_mode,
            as_query: self.as_query.rebind_attachment::<B>(),
            like: self.like,
        }
    }
}

impl MapAttachment for CteCycle<()> {
    type Rebound<B> = CteCycle<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        CteCycle {
            by: self.by,
            mark_column: self.mark_column,
            mark_value: self.mark_value.rebind_attachment::<B>(),
            default_value: self.default_value.rebind_attachment::<B>(),
            path_column: self.path_column,
        }
    }
}

impl MapAttachment for CteQuery<()> {
    type Rebound<B> = CteQuery<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Simple(value0) => CteQuery::<B>::Simple(value0.rebind_attachment::<B>()),
            Self::Recursive {
                operator,
                base,
                recursive,
            } => CteQuery::<B>::Recursive {
                operator,
                base: base.rebind_attachment::<B>(),
                recursive: recursive.rebind_attachment::<B>(),
            },
        }
    }
}

impl MapAttachment for DeclareCursorStatement<()> {
    type Rebound<B> = DeclareCursorStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        DeclareCursorStatement {
            name: self.name,
            binary: self.binary,
            scroll: self.scroll,
            hold: self.hold,
            query_sql: self.query_sql,
            query: self.query.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for DeleteStatement<()> {
    type Rebound<B> = DeleteStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        DeleteStatement {
            table: self.table,
            filter: self.filter.rebind_attachment::<B>(),
            returning: self.returning.rebind_attachment::<B>(),
            alias: self.alias,
            using: self.using.rebind_attachment::<B>(),
            returning_aliases: self.returning_aliases,
            ctes: self.ctes.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for ExplainStatement<()> {
    type Rebound<B> = ExplainStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        ExplainStatement {
            analyze: self.analyze,
            statement: self.statement.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for Expr<()> {
    type Rebound<B> = Expr<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        rebind_expr(self)
    }
}

fn rebind_expr<B>(expression: Expr<()>) -> Expr<B> {
    match expression {
        Expr::Column(value) => Expr::Column(value),
        Expr::Param(value) => Expr::Param(value),
        Expr::StringLiteral(value) => Expr::StringLiteral(value),
        Expr::NumberLiteral(value) => Expr::NumberLiteral(value),
        Expr::IntegerLiteral(value) => Expr::IntegerLiteral(value),
        Expr::NumericLiteral(value) => Expr::NumericLiteral(value),
        Expr::BoolLiteral(value) => Expr::BoolLiteral(value),
        Expr::Null => Expr::Null,
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => rebind_case(operand, branches, else_expr),
        Expr::Binary { left, op, right } => Expr::<B>::Binary {
            left: left.rebind_attachment::<B>(),
            op,
            right: right.rebind_attachment::<B>(),
        },
        Expr::TruthTest {
            expr,
            truth,
            negated,
        } => Expr::<B>::TruthTest {
            expr: expr.rebind_attachment::<B>(),
            truth,
            negated,
        },
        Expr::IsNull { expr, negated } => Expr::<B>::IsNull {
            expr: expr.rebind_attachment::<B>(),
            negated,
        },
        Expr::InList {
            expr,
            values,
            negated,
        } => Expr::<B>::InList {
            expr: expr.rebind_attachment::<B>(),
            values: values.rebind_attachment::<B>(),
            negated,
        },
        Expr::Like {
            expr,
            pattern,
            escape,
            case_insensitive,
        } => Expr::<B>::Like {
            expr: expr.rebind_attachment::<B>(),
            pattern: pattern.rebind_attachment::<B>(),
            escape: escape.rebind_attachment::<B>(),
            case_insensitive,
        },
        Expr::Between {
            expr,
            low,
            high,
            negated,
            symmetric,
        } => Expr::<B>::Between {
            expr: expr.rebind_attachment::<B>(),
            low: low.rebind_attachment::<B>(),
            high: high.rebind_attachment::<B>(),
            negated,
            symmetric,
        },
        Expr::Not { expr } => Expr::<B>::Not {
            expr: expr.rebind_attachment::<B>(),
        },
        Expr::Cast {
            expr,
            data_type,
            source_type,
            identity,
        } => Expr::<B>::Cast {
            expr: expr.rebind_attachment::<B>(),
            data_type,
            source_type,
            identity,
        },
        Expr::Collate { expr, collation } => Expr::<B>::Collate {
            expr: expr.rebind_attachment::<B>(),
            collation,
        },
        Expr::ArraySubscript { array, subscripts } => Expr::<B>::ArraySubscript {
            array: array.rebind_attachment::<B>(),
            subscripts: subscripts.rebind_attachment::<B>(),
        },
        Expr::UnaryNumeric { expr, op } => Expr::<B>::UnaryNumeric {
            expr: expr.rebind_attachment::<B>(),
            op,
        },
        Expr::IntegerNegate { expr, width } => Expr::<B>::IntegerNegate {
            expr: expr.rebind_attachment::<B>(),
            width,
        },
        Expr::RowCompare(value0) => Expr::<B>::RowCompare(value0.rebind_attachment::<B>()),
        Expr::Exists(value0) => Expr::<B>::Exists(value0.rebind_attachment::<B>()),
        Expr::Subquery(value0) => Expr::<B>::Subquery(value0.rebind_attachment::<B>()),
        Expr::Function(value0) => Expr::<B>::Function(value0.rebind_attachment::<B>()),
    }
}

fn rebind_case<B>(
    operand: Option<Box<Expr<()>>>,
    branches: Vec<(Expr<()>, Expr<()>)>,
    else_expr: Option<Box<Expr<()>>>,
) -> Expr<B> {
    Expr::Case {
        operand: operand.rebind_attachment::<B>(),
        branches: branches.rebind_attachment::<B>(),
        else_expr: else_expr.rebind_attachment::<B>(),
    }
}

impl MapAttachment for FunctionCall<()> {
    type Rebound<B> = FunctionCall<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        FunctionCall {
            name: self.name,
            args: self.args.rebind_attachment::<B>(),
            quantifier: self.quantifier,
            filter: self.filter.rebind_attachment::<B>(),
            order_by: self.order_by.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for GraphColumn<()> {
    type Rebound<B> = GraphColumn<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphColumn {
            expr: self.expr.rebind_attachment::<B>(),
            alias: self.alias,
        }
    }
}

impl MapAttachment for GraphEdgePattern<()> {
    type Rebound<B> = GraphEdgePattern<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphEdgePattern {
            variable: self.variable,
            labels: self.labels,
            filter: self.filter.rebind_attachment::<B>(),
            direction: self.direction,
            quantifier: self.quantifier,
        }
    }
}

impl MapAttachment for GraphPathPattern<()> {
    type Rebound<B> = GraphPathPattern<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphPathPattern {
            start: self.start.rebind_attachment::<B>(),
            steps: self.steps.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for GraphPathStep<()> {
    type Rebound<B> = GraphPathStep<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphPathStep {
            edge: self.edge.rebind_attachment::<B>(),
            vertex: self.vertex.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for GraphTable<()> {
    type Rebound<B> = GraphTable<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphTable {
            graph: self.graph,
            path_variable: self.path_variable,
            path: self.path.rebind_attachment::<B>(),
            extra_paths: self.extra_paths.rebind_attachment::<B>(),
            selector: self.selector,
            filter: self.filter.rebind_attachment::<B>(),
            columns: self.columns.rebind_attachment::<B>(),
            binding: self.binding,
        }
    }
}

impl MapAttachment for GraphVertexPattern<()> {
    type Rebound<B> = GraphVertexPattern<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        GraphVertexPattern {
            variable: self.variable,
            labels: self.labels,
            filter: self.filter.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for InsertConflictAction<()> {
    type Rebound<B> = InsertConflictAction<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::DoNothing => InsertConflictAction::<B>::DoNothing,
            Self::DoUpdate {
                assignments,
                filter,
            } => InsertConflictAction::<B>::DoUpdate {
                assignments: assignments.rebind_attachment::<B>(),
                filter: filter.rebind_attachment::<B>(),
            },
        }
    }
}

impl MapAttachment for InsertConflictClause<()> {
    type Rebound<B> = InsertConflictClause<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        InsertConflictClause {
            target_fields: self.target_fields,
            action: self.action.rebind_attachment::<B>(),
            constraint: self.constraint,
            target_expressions: self.target_expressions.rebind_attachment::<B>(),
            target_predicate: self.target_predicate.rebind_attachment::<B>(),
            update_row_subqueries: self.update_row_subqueries.rebind_attachment::<B>(),
        }
    }
}

impl MapAttachment for InsertSource<()> {
    type Rebound<B> = InsertSource<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        match self {
            Self::Values(value0) => InsertSource::<B>::Values(value0.rebind_attachment::<B>()),
            Self::Select(value0) => InsertSource::<B>::Select(value0.rebind_attachment::<B>()),
        }
    }
}

impl MapAttachment for InsertStatement<()> {
    type Rebound<B> = InsertStatement<B>;
    fn rebind_attachment<B>(self) -> Self::Rebound<B> {
        InsertStatement {
            table: self.table,
            columns: self.columns,
            source: self.source.rebind_attachment::<B>(),
            on_conflict: self.on_conflict.rebind_attachment::<B>(),
            returning: self.returning.rebind_attachment::<B>(),
            overriding: self.overriding,
            alias: self.alias,
            returning_aliases: self.returning_aliases,
            ctes: self.ctes.rebind_attachment::<B>(),
        }
    }
}
