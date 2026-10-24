//! Data-modifying `WITH` queries, after PostgreSQL's rules:
//!
//! - every data-modifying CTE runs exactly once, referenced or not, in order;
//! - all of them and the outer statement read one snapshot, taken before the
//!   first runs, so the outer statement sees their `RETURNING` rows but not
//!   their effects on the tables;
//! - the statement is atomic: a failure anywhere discards every change.
//!
//! Each data-modifying CTE is replaced by an inline relation of its
//! `RETURNING` rows, whose values travel as extra parameters so every value
//! keeps its exact carrier.
use super::{
    build_select_result, dml_command, execute_plan, Cassie, CassieSession, CommonTableExpression,
    CteContext, CteQuery, Expr, FunctionMeta, HashMap, LogicalCommand, PhysicalPlan, QueryError,
    QueryExecutionControls, QueryResult, QuerySource, QueryStatement, SelectItem, Value,
};
use crate::sql::ast::{InsertSource, ParsedStatement, SelectStatement};
use crate::sql::binder::cte_bodies::cte_is_modifying;

pub(super) struct StatementInputs<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) params: &'a [Value],
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) controls: &'a QueryExecutionControls,
}

/// Runs `plan` when it owns a data-modifying CTE; otherwise returns `None`.
pub(super) fn run(
    inputs: &StatementInputs<'_>,
    plan: &PhysicalPlan,
) -> Result<Option<QueryResult>, QueryError> {
    let modifying = owned_ctes(&plan.logical)
        .iter()
        .filter(|cte| cte_is_modifying(cte))
        .cloned()
        .collect::<Vec<_>>();
    if modifying.is_empty() {
        return Ok(None);
    }
    let owns_transaction = inputs
        .session
        .is_some_and(|session| !session.is_transaction_active());
    if let Some(session) = inputs.session.filter(|_| owns_transaction) {
        session.begin_implicit_transaction()?;
    }
    let result = execute(inputs, plan, &modifying);
    let Some(session) = inputs.session.filter(|_| owns_transaction) else {
        return result.map(Some);
    };
    match result {
        Ok(result) => {
            if let Err(error) = inputs.cassie.commit_transaction(session) {
                session.rollback_transaction();
                return Err(error.into());
            }
            Ok(Some(result))
        }
        Err(error) => {
            session.rollback_transaction();
            Err(error)
        }
    }
}

fn owned_ctes(plan: &super::LogicalPlan) -> &[CommonTableExpression] {
    match &plan.command {
        None => &plan.ctes,
        Some(LogicalCommand::Insert(statement)) => &statement.ctes,
        Some(LogicalCommand::Update(statement)) => &statement.ctes,
        Some(LogicalCommand::Delete(statement)) => &statement.ctes,
        Some(_) => &[],
    }
}

fn execute(
    inputs: &StatementInputs<'_>,
    plan: &PhysicalPlan,
    modifying: &[CommonTableExpression],
) -> Result<QueryResult, QueryError> {
    let snapshot = super::entrypoints::statement_read_controls(
        inputs.cassie,
        inputs.session,
        inputs.controls,
    )?;
    let mut params = inputs.params.to_vec();
    let mut relations = Vec::new();
    for cte in modifying {
        super::check_timeout(&snapshot)?;
        let CteQuery::Simple(statement) = &cte.query else {
            continue;
        };
        let returned = run_body(inputs, statement, &params, &snapshot)?;
        relations.push(relation(cte, &returned, &mut params));
    }
    let (_read, _overlay) = super::entrypoints::enter_statement_read(&snapshot);
    match &plan.logical.command {
        None => {
            let mut logical = plan.logical.clone();
            replace(&mut logical.ctes, &relations);
            let mut context = CteContext::new();
            let rows = execute_plan(
                inputs.cassie,
                inputs.session,
                &logical,
                &mut context,
                inputs.user_functions,
                &params,
                &snapshot,
            )?;
            build_select_result(
                inputs.cassie,
                inputs.session,
                plan,
                rows,
                inputs.user_functions,
                &snapshot,
            )
        }
        Some(command) => {
            let mut command = command.clone();
            inherit(&mut command, &relations);
            dml_command::execute_command(
                inputs.cassie,
                inputs.session,
                &command,
                &params,
                inputs.user_functions,
                &snapshot,
            )
        }
    }
}

fn run_body(
    inputs: &StatementInputs<'_>,
    statement: &ParsedStatement,
    params: &[Value],
    snapshot: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
        indexes: Vec::new(),
    })
    .map_err(QueryError::from)?;
    let Some(command) = plan.command.as_ref() else {
        return Err(QueryError::General(
            "data-modifying WITH query did not plan a command".into(),
        ));
    };
    dml_command::execute_command(
        inputs.cassie,
        inputs.session,
        command,
        params,
        inputs.user_functions,
        snapshot,
    )
}

/// `SELECT * FROM (VALUES ($n, ...), ...) AS name(columns)` over the rows.
fn relation(
    cte: &CommonTableExpression,
    returned: &QueryResult,
    params: &mut Vec<Value>,
) -> CommonTableExpression {
    let columns = returned
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(returned.rows().len().max(1));
    for row in returned.rows() {
        rows.push(
            row.iter()
                .map(|value| {
                    params.push(value.clone());
                    Expr::Param(params.len() - 1)
                })
                .collect::<Vec<_>>(),
        );
    }
    let empty = rows.is_empty();
    if empty {
        rows.push(vec![Expr::Null; columns.len().max(1)]);
    }
    let select = SelectStatement {
        source: QuerySource::Aliased {
            source: Box::new(QuerySource::Values { rows }),
            alias: cte.name.clone(),
            column_aliases: columns.clone(),
        },
        ctes: Vec::new(),
        recursive: false,
        distinct: false,
        distinct_on: Vec::new(),
        projection: vec![SelectItem::Wildcard],
        filter: empty.then_some(Expr::BoolLiteral(false)),
        group_by: Vec::new(),
        having: None,
        order: Vec::new(),
        limit: None,
        offset: None,
        set: None,
    };
    CommonTableExpression {
        name: cte.name.clone(),
        aliases: if cte.aliases.is_empty() {
            columns
        } else {
            cte.aliases.clone()
        },
        query: CteQuery::Simple(Box::new(ParsedStatement {
            raw_sql: String::new(),
            statement: QueryStatement::Select(Box::new(select)),
        })),
        materialization: crate::sql::ast::CteMaterialization::Materialized,
        search: None,
        cycle: None,
    }
}

fn replace(ctes: &mut [CommonTableExpression], relations: &[CommonTableExpression]) {
    for cte in ctes {
        if let Some(relation) = relations
            .iter()
            .find(|relation| relation.name.eq_ignore_ascii_case(&cte.name))
        {
            *cte = relation.clone();
        }
    }
}

/// Replaces the data-modifying definitions each nested query inherited.
fn inherit(command: &mut LogicalCommand, relations: &[CommonTableExpression]) {
    match command {
        LogicalCommand::Insert(insert) => {
            insert.ctes.clear();
            match &mut insert.source {
                InsertSource::Select(select) => select_ctes(select, relations),
                InsertSource::Values(rows) => {
                    for expr in rows.iter_mut().flatten() {
                        nested(expr, relations);
                    }
                }
            }
            items(&mut insert.returning, relations);
        }
        LogicalCommand::Update(update) => {
            update.ctes.clear();
            for (_, expr) in &mut update.assignments {
                nested(expr, relations);
            }
            if let Some(expr) = &mut update.filter {
                nested(expr, relations);
            }
            for assignment in &mut update.row_subqueries {
                statement(&mut assignment.query, relations);
            }
            if let Some(select) = update.from.as_mut().and_then(|from| from.query.as_mut()) {
                select_ctes(select, relations);
            }
            items(&mut update.returning, relations);
        }
        LogicalCommand::Delete(delete) => {
            delete.ctes.clear();
            if let Some(expr) = &mut delete.filter {
                nested(expr, relations);
            }
            if let Some(select) = delete.using.as_mut().and_then(|using| using.query.as_mut()) {
                select_ctes(select, relations);
            }
            items(&mut delete.returning, relations);
        }
        _ => {}
    }
}

fn select_ctes(select: &mut SelectStatement, relations: &[CommonTableExpression]) {
    replace(&mut select.ctes, relations);
}

fn statement(query: &mut ParsedStatement, relations: &[CommonTableExpression]) {
    if let QueryStatement::Select(select) = &mut query.statement {
        select_ctes(select, relations);
    }
}

fn nested(expr: &mut Expr, relations: &[CommonTableExpression]) {
    if let Some(query) = expr.subquery_statement_mut() {
        statement(query, relations);
    }
    let _ = expr.try_visit_children_mut(|child| {
        nested(child, relations);
        Ok::<_, ()>(())
    });
}

fn items(items: &mut [SelectItem], relations: &[CommonTableExpression]) {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => nested(expr, relations),
            SelectItem::Function { function, .. } => {
                for arg in function.expressions_mut() {
                    nested(arg, relations);
                }
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. } => {}
        }
    }
}
