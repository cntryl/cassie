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
use std::mem::size_of;

use super::{
    build_select_result, dml_command, execute_plan, Cassie, CassieSession, CommonTableExpression,
    CteContext, CteQuery, Expr, FunctionMeta, HashMap, LogicalCommand, PhysicalPlan, QueryError,
    QueryExecutionControls, QueryResult, QuerySource, QueryStatement, SelectItem, Value,
};
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::{InsertSource, OrderExpr, ParsedStatement, SelectStatement};
use crate::sql::binder::cte_bodies::cte_is_modifying;

pub(super) struct StatementInputs<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) params: &'a [Value],
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) authority: Option<&'a crate::app::FailureAuthority>,
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
        session.begin_implicit_transaction_for_operation(inputs.authority)?;
        #[cfg(test)]
        CassieSession::observe_owned_transaction_start_for_test();
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
    let mut relation_memory = AccountedVec::<QueryMemoryReservation>::try_new(&snapshot)?;
    let mut params = AccountedVec::try_new(&snapshot)?;
    for value in inputs.params {
        snapshot.check_interruption()?;
        params.try_push_clone(value, value_clone_bytes(value)?)?;
    }
    let mut relations = Vec::new();
    for cte in modifying {
        super::check_timeout(&snapshot)?;
        let CteQuery::Simple(statement) = &cte.query else {
            continue;
        };
        let returned = run_body(inputs, statement, params.as_slice(), &snapshot)?;
        relations.push(relation(
            cte,
            &returned,
            &mut params,
            &snapshot,
            &mut relation_memory,
        )?);
    }
    let (_read, _overlay) = super::entrypoints::enter_statement_read(&snapshot);
    match &plan.logical.command {
        None => {
            let mut logical = plan.logical.clone();
            replace(
                &mut logical.ctes,
                &relations,
                &snapshot,
                &mut relation_memory,
            )?;
            let mut context = CteContext::new();
            let rows = execute_plan(
                inputs.cassie,
                inputs.session,
                &logical,
                &mut context,
                inputs.user_functions,
                params.as_slice(),
                &snapshot,
            )?;
            build_select_result(
                inputs.cassie,
                inputs.session,
                plan,
                super::owned_rows::OwnedRows::untracked(rows),
                inputs.user_functions,
                &snapshot,
            )
        }
        Some(command) => {
            let mut command = command.clone();
            inherit(&mut command, &relations, &snapshot, &mut relation_memory)?;
            dml_command::execute_command_for_operation(
                inputs.cassie,
                inputs.session,
                &command,
                params.as_slice(),
                inputs.user_functions,
                (&snapshot, inputs.authority),
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
    let controls = snapshot.for_uncapped_input();
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
    dml_command::execute_command_for_operation(
        inputs.cassie,
        inputs.session,
        command,
        params,
        inputs.user_functions,
        (&controls, inputs.authority),
    )
}

/// `SELECT * FROM (VALUES ($n, ...), ...) AS name(columns)` over the rows.
fn relation(
    cte: &CommonTableExpression,
    returned: &QueryResult,
    params: &mut AccountedVec<Value>,
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<CommonTableExpression, QueryError> {
    let relation_memory_bytes = relation_storage_bytes(cte, returned)?;
    let relation_reservation = controls.reserve_query_memory(relation_memory_bytes)?;
    relation_memory.try_push_with(0, || relation_reservation)?;
    controls.check_interruption()?;
    let columns = returned
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(returned.rows().len().max(1));
    for row in returned.rows() {
        controls.check_interruption()?;
        let mut expressions = Vec::with_capacity(row.len());
        for value in row {
            controls.check_interruption()?;
            let index = params.len();
            params.try_push_clone(value, value_clone_bytes(value)?)?;
            expressions.push(Expr::Param(index));
        }
        rows.push(expressions);
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
        grouping_subtotal: false,
        having: None,
        order: Vec::new(),
        limit: None,
        offset: None,
        set: None,
    };
    Ok(CommonTableExpression {
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
    })
}

fn relation_storage_bytes(
    cte: &CommonTableExpression,
    returned: &QueryResult,
) -> Result<usize, QueryError> {
    let columns = returned.columns();
    let rows = returned.rows();
    let row_count = rows.len().max(1);
    let expression_count = if rows.is_empty() {
        columns.len().max(1)
    } else {
        rows.iter()
            .try_fold(0usize, |count, row| add(count, row.len()))?
    };
    let mut bytes = add(
        add(
            size_of::<CommonTableExpression>(),
            size_of::<ParsedStatement>(),
        )?,
        add(size_of::<SelectStatement>(), size_of::<QuerySource>())?,
    )?;
    bytes = add(bytes, mul(row_count, size_of::<Vec<Expr>>())?)?;
    bytes = add(bytes, mul(expression_count, size_of::<Expr>())?)?;
    bytes = add(bytes, size_of::<SelectItem>())?;
    bytes = add(bytes, mul(columns.len(), size_of::<String>())?)?;
    bytes = add(
        bytes,
        mul(
            if cte.aliases.is_empty() {
                columns.len()
            } else {
                cte.aliases.len()
            },
            size_of::<String>(),
        )?,
    )?;
    let column_name_bytes = columns
        .iter()
        .try_fold(0usize, |total, column| add(total, column.name.capacity()))?;
    bytes = add(
        bytes,
        mul(
            column_name_bytes,
            if cte.aliases.is_empty() { 2 } else { 1 },
        )?,
    )?;
    bytes = add(bytes, mul(cte.name.capacity(), 2)?)?;
    if !cte.aliases.is_empty() {
        bytes = add(
            bytes,
            cte.aliases
                .iter()
                .try_fold(0usize, |total, alias| add(total, alias.capacity()))?,
        )?;
    }
    Ok(bytes)
}

fn relation_clone_memory_bytes(cte: &CommonTableExpression) -> Result<usize, QueryError> {
    let mut bytes = size_of::<CommonTableExpression>();
    bytes = add(bytes, cte.name.capacity())?;
    bytes = add(bytes, mul(cte.aliases.capacity(), size_of::<String>())?)?;
    bytes = add(
        bytes,
        cte.aliases
            .iter()
            .try_fold(0usize, |total, alias| add(total, alias.capacity()))?,
    )?;
    let CteQuery::Simple(statement) = &cte.query else {
        return Err(QueryError::General(
            "unexpected recursive query in modifying CTE relation".into(),
        ));
    };
    bytes = add(bytes, size_of::<ParsedStatement>())?;
    bytes = add(bytes, statement.raw_sql.capacity())?;
    let QueryStatement::Select(select) = &statement.statement else {
        return Err(QueryError::General(
            "unexpected non-SELECT query in modifying CTE relation".into(),
        ));
    };
    bytes = add(bytes, size_of::<SelectStatement>())?;
    bytes = add(
        bytes,
        mul(select.ctes.capacity(), size_of::<CommonTableExpression>())?,
    )?;
    bytes = add(
        bytes,
        mul(select.distinct_on.capacity(), size_of::<Expr>())?,
    )?;
    bytes = add(
        bytes,
        mul(select.projection.capacity(), size_of::<SelectItem>())?,
    )?;
    bytes = add(bytes, mul(select.group_by.capacity(), size_of::<Expr>())?)?;
    bytes = add(bytes, mul(select.order.capacity(), size_of::<OrderExpr>())?)?;
    let QuerySource::Aliased {
        source,
        alias,
        column_aliases,
    } = &select.source
    else {
        return Err(QueryError::General(
            "unexpected source in modifying CTE relation".into(),
        ));
    };
    bytes = add(bytes, alias.capacity())?;
    bytes = add(bytes, mul(column_aliases.capacity(), size_of::<String>())?)?;
    bytes = add(
        bytes,
        column_aliases
            .iter()
            .try_fold(0usize, |total, name| add(total, name.capacity()))?,
    )?;
    let QuerySource::Values { rows } = source.as_ref() else {
        return Err(QueryError::General(
            "unexpected aliased source in modifying CTE relation".into(),
        ));
    };
    bytes = add(bytes, size_of::<QuerySource>())?;
    bytes = add(bytes, mul(rows.capacity(), size_of::<Vec<Expr>>())?)?;
    rows.iter()
        .try_fold(bytes, |total, row| {
            add(total, mul(row.capacity(), size_of::<Expr>())?)
        })
        .map_err(Into::into)
}

fn replace(
    ctes: &mut [CommonTableExpression],
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    for cte in ctes {
        if let Some(relation) = relations.iter().find(|relation| relation.name == cte.name) {
            let reservation =
                controls.reserve_query_memory(relation_clone_memory_bytes(relation)?)?;
            relation_memory.try_push_with(0, || reservation)?;
            controls.check_interruption()?;
            *cte = relation.clone();
        }
    }
    Ok(())
}

/// Replaces the data-modifying definitions each nested query inherited.
fn inherit(
    command: &mut LogicalCommand,
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    match command {
        LogicalCommand::Insert(insert) => {
            insert.ctes.clear();
            match &mut insert.source {
                InsertSource::Select(select) => {
                    select_ctes(select, relations, controls, relation_memory)?;
                }
                InsertSource::Values(rows) => {
                    for expr in rows.iter_mut().flatten() {
                        nested(expr, relations, controls, relation_memory)?;
                    }
                }
            }
            items(&mut insert.returning, relations, controls, relation_memory)?;
        }
        LogicalCommand::Update(update) => {
            update.ctes.clear();
            for (_, expr) in &mut update.assignments {
                nested(expr, relations, controls, relation_memory)?;
            }
            if let Some(expr) = &mut update.filter {
                nested(expr, relations, controls, relation_memory)?;
            }
            for assignment in &mut update.row_subqueries {
                statement(&mut assignment.query, relations, controls, relation_memory)?;
            }
            if let Some(select) = update.from.as_mut().and_then(|from| from.query.as_mut()) {
                select_ctes(select, relations, controls, relation_memory)?;
            }
            items(&mut update.returning, relations, controls, relation_memory)?;
        }
        LogicalCommand::Delete(delete) => {
            delete.ctes.clear();
            if let Some(expr) = &mut delete.filter {
                nested(expr, relations, controls, relation_memory)?;
            }
            if let Some(select) = delete.using.as_mut().and_then(|using| using.query.as_mut()) {
                select_ctes(select, relations, controls, relation_memory)?;
            }
            items(&mut delete.returning, relations, controls, relation_memory)?;
        }
        _ => {}
    }
    Ok(())
}

fn select_ctes(
    select: &mut SelectStatement,
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    replace(&mut select.ctes, relations, controls, relation_memory)
}

fn statement(
    query: &mut ParsedStatement,
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    if let QueryStatement::Select(select) = &mut query.statement {
        select_ctes(select, relations, controls, relation_memory)?;
    }
    Ok(())
}

fn nested(
    expr: &mut Expr,
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    if let Some(query) = expr.subquery_statement_mut() {
        statement(query, relations, controls, relation_memory)?;
    }
    expr.try_visit_children_mut(|child| nested(child, relations, controls, relation_memory))
}

fn items(
    items: &mut [SelectItem],
    relations: &[CommonTableExpression],
    controls: &QueryExecutionControls,
    relation_memory: &mut AccountedVec<QueryMemoryReservation>,
) -> Result<(), QueryError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => nested(expr, relations, controls, relation_memory)?,
            SelectItem::Function { function, .. } => {
                for arg in function.expressions_mut() {
                    nested(arg, relations, controls, relation_memory)?;
                }
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod retained_relation_tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;
    use crate::executor::execution::ColumnMeta;
    use crate::runtime::QueryExecutionControls;
    use std::time::Instant;

    fn controls_with_budget(bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: bytes,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    fn relation_clone_layout_bytes(cte: &CommonTableExpression) -> usize {
        let CteQuery::Simple(statement) = &cte.query else {
            panic!("fixture relation is a simple query");
        };
        let QueryStatement::Select(select) = &statement.statement else {
            panic!("fixture relation is a SELECT");
        };
        let QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } = &select.source
        else {
            panic!("fixture relation has an aliased source");
        };
        let QuerySource::Values { rows } = source.as_ref() else {
            panic!("fixture relation has a values source");
        };

        size_of::<CommonTableExpression>()
            + cte.name.capacity()
            + cte.aliases.capacity() * size_of::<String>()
            + cte.aliases.iter().map(String::capacity).sum::<usize>()
            + size_of::<ParsedStatement>()
            + statement.raw_sql.capacity()
            + size_of::<SelectStatement>()
            + select.ctes.capacity() * size_of::<CommonTableExpression>()
            + select.distinct_on.capacity() * size_of::<Expr>()
            + select.projection.capacity() * size_of::<SelectItem>()
            + select.group_by.capacity() * size_of::<Expr>()
            + select.order.capacity() * size_of::<OrderExpr>()
            + size_of::<QuerySource>()
            + alias.capacity()
            + column_aliases.capacity() * size_of::<String>()
            + column_aliases.iter().map(String::capacity).sum::<usize>()
            + rows.capacity() * size_of::<Vec<Expr>>()
            + rows
                .iter()
                .map(|row| row.capacity() * size_of::<Expr>())
                .sum::<usize>()
    }

    #[test]
    fn should_admit_each_inherited_relation_clone_before_constructing_it() {
        // Arrange
        let result_controls = controls_with_budget(64 * 1024);
        let mut builder = QueryResult::try_builder(&result_controls).expect("result builder");
        builder
            .try_push_column(&ColumnMeta::text("payload"))
            .expect("result column");
        builder
            .try_push_row(&[Value::String("x".repeat(4096))])
            .expect("result row");
        let returned = builder.finish().expect("admitted result");
        let cte = CommonTableExpression {
            name: "inserted".to_owned(),
            aliases: Vec::new(),
            query: CteQuery::Simple(Box::new(
                crate::sql::parse_statement("SELECT 1").expect("placeholder CTE"),
            )),
            materialization: crate::sql::ast::CteMaterialization::Materialized,
            search: None,
            cycle: None,
        };
        let source_controls = controls_with_budget(64 * 1024);
        let mut params = AccountedVec::try_new(&source_controls).expect("parameter owner");
        let mut relation_memory = AccountedVec::try_new(&source_controls).expect("relation owner");
        let relation = relation(
            &cte,
            &returned,
            &mut params,
            &source_controls,
            &mut relation_memory,
        )
        .expect("source relation");
        let clone_bytes = relation_clone_memory_bytes(&relation).expect("clone estimate");
        let controls = controls_with_budget(clone_bytes + size_of::<QueryMemoryReservation>());
        let mut clone_memory = AccountedVec::try_new(&controls).expect("clone lease owner");
        let mut ctes = vec![cte.clone(), cte];

        // Act
        let result = replace(&mut ctes, &[relation], &controls, &mut clone_memory);

        // Assert
        assert!(result.is_err(), "the second relation clone must be denied");
        assert_eq!(ctes[0].aliases, ["payload"]);
        assert!(
            ctes[1].aliases.is_empty(),
            "denial precedes the second clone"
        );
        drop(clone_memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_admit_one_cte_relation_clone_at_its_exact_memory_budget() {
        // Arrange
        let result_controls = controls_with_budget(64 * 1024);
        let mut builder = QueryResult::try_builder(&result_controls).expect("result builder");
        builder
            .try_push_column(&ColumnMeta::text("payload"))
            .expect("result column");
        builder
            .try_push_row(&[Value::String("x".repeat(4096))])
            .expect("result row");
        let returned = builder.finish().expect("admitted result");
        let cte = CommonTableExpression {
            name: "inserted".to_owned(),
            aliases: Vec::new(),
            query: CteQuery::Simple(Box::new(
                crate::sql::parse_statement("SELECT 1").expect("placeholder CTE"),
            )),
            materialization: crate::sql::ast::CteMaterialization::Materialized,
            search: None,
            cycle: None,
        };
        let prototype_controls = controls_with_budget(64 * 1024);
        let mut prototype_params = AccountedVec::try_new(&prototype_controls).expect("params");
        let mut prototype_memory =
            AccountedVec::try_new(&prototype_controls).expect("relation memory");
        let prototype = relation(
            &cte,
            &returned,
            &mut prototype_params,
            &prototype_controls,
            &mut prototype_memory,
        )
        .expect("prototype relation");
        let value = &returned.rows()[0][0];
        let budget = relation_storage_bytes(&cte, &returned).expect("original estimate")
            + relation_clone_layout_bytes(&prototype)
            + size_of::<Value>()
            + value_clone_bytes(value).expect("parameter heap")
            + 2 * size_of::<QueryMemoryReservation>();
        let controls = controls_with_budget(budget);
        let mut params = AccountedVec::try_new(&controls).expect("parameter owner");
        let mut relation_memory = AccountedVec::try_new(&controls).expect("relation owner");
        let mut ctes = vec![cte.clone()];

        // Act
        let relation = relation(
            &cte,
            &returned,
            &mut params,
            &controls,
            &mut relation_memory,
        )
        .expect("original relation fits");
        replace(&mut ctes, &[relation], &controls, &mut relation_memory)
            .expect("one actual clone fits");

        // Assert
        assert_eq!(ctes[0].aliases, ["payload"]);
        assert_eq!(controls.current_query_memory_bytes(), budget);
        drop(relation_memory);
        drop(params);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
