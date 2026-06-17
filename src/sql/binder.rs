use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::mem;
use std::pin::Pin;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::sql::ast::{
    AlterTableOperation, AlterTableStatement, CreateIndexStatement, CreateSchemaStatement,
    DropIndexStatement, CteQuery, Expr, FunctionCall, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement,
};

type CteScope = HashMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
}

pub async fn bind(
    statement: ParsedStatement,
    catalog: &Catalog,
) -> Result<BoundStatement, CassieError> {
    let statement = bind_statement(statement, catalog, &HashMap::new()).await?;
    Ok(BoundStatement { statement })
}

fn bind_statement<'a>(
    statement: ParsedStatement,
    catalog: &'a Catalog,
    outer_scope: &'a CteScope,
) -> Pin<Box<dyn Future<Output = Result<ParsedStatement, CassieError>> + Send + 'a>> {
    Box::pin(async move {
        let raw_sql = statement.raw_sql.clone();
        match statement.statement {
            QueryStatement::Select(select) => {
                let select = bind_select(select, catalog, outer_scope).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::Select(select),
                })
            }
            QueryStatement::CreateTable(statement) => {
                let statement = bind_create_table(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CreateTable(statement),
                })
            }
            QueryStatement::DropTable(statement) => {
                let statement = bind_drop_table(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::DropTable(statement),
                })
            }
            QueryStatement::AlterTable(statement) => {
                let statement = bind_alter_table(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::AlterTable(statement),
                })
            }
            QueryStatement::CreateIndex(statement) => {
                let statement = bind_create_index(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CreateIndex(statement),
                })
            }
            QueryStatement::DropIndex(statement) => {
                let statement = bind_drop_index(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::DropIndex(statement),
                })
            }
            QueryStatement::CreateSchema(statement) => {
                let schema = statement.schema.trim().to_string();
                if schema.is_empty() {
                    return Err(CassieError::Planner("CREATE SCHEMA requires a name".into()));
                }

                if !statement.if_not_exists && catalog.namespace_exists(&schema).await {
                    return Err(CassieError::Planner(format!(
                        "namespace '{schema}' already exists"
                    )));
                }

                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CreateSchema(CreateSchemaStatement {
                        schema,
                        if_not_exists: statement.if_not_exists,
                    }),
                })
            }
        }
    })
}

async fn bind_create_table(
    mut statement: crate::sql::ast::CreateTableStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::CreateTableStatement, CassieError> {
    let name = statement.table.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires a table name".into(),
        ));
    }
    if !statement.if_not_exists && catalog.exists(&name).await {
        return Err(CassieError::Planner(format!(
            "collection '{name}' already exists"
        )));
    }

    let mut seen = HashSet::new();
    let mut primary_key_field: Option<String> = None;
    for field in &mut statement.fields {
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::Planner(
                "CREATE TABLE field names cannot be empty".into(),
            ));
        }

        if !seen.insert(field_name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "CREATE TABLE field '{field_name}' is defined more than once"
            )));
        }

        for constraint in &field.constraints {
            if constraint.primary_key {
                if let Some(previous) = &primary_key_field {
                    return Err(CassieError::Planner(format!(
                        "multiple primary keys defined on '{name}': '{previous}' and '{field_name}'"
                    )));
                }
                primary_key_field = Some(field_name.to_string());
            }
        }

        field.name = field_name.to_string();
    }

    statement.table = name;
    Ok(statement)
}

async fn bind_create_index(
    mut statement: CreateIndexStatement,
    catalog: &Catalog,
) -> Result<CreateIndexStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires a collection name".into(),
        ));
    }
    if !catalog.exists(&table).await {
        return Err(CassieError::CollectionNotFound(table));
    }

    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires an index name".into(),
        ));
    }

    let field = statement.field.trim().to_string();
    if field.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires an index field".into(),
        ));
    }

    let schema = catalog
        .get_schema(&table)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;
    if !schema.fields.iter().any(|entry| entry.name == field) {
        return Err(CassieError::Planner(format!(
            "index field '{field}' does not exist on collection '{table}'"
        )));
    }

    if !statement.if_not_exists && catalog.get_index(&table, &name).await.is_some() {
        return Err(CassieError::Planner(format!(
            "index '{name}' already exists on collection '{table}'"
        )));
    }

    statement.table = table;
    statement.name = name;
    statement.field = field;
    Ok(statement)
}

async fn bind_drop_index(
    mut statement: DropIndexStatement,
    catalog: &Catalog,
) -> Result<DropIndexStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP INDEX requires a collection name".into(),
        ));
    }
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP INDEX requires an index name".into()));
    }

    if !catalog.exists(&table).await {
        if !statement.if_exists {
            return Err(CassieError::CollectionNotFound(table));
        }
        statement.table = table;
        statement.name = name;
        return Ok(statement);
    }

    if !statement.if_exists && catalog.get_index(&table, &name).await.is_none() {
        return Err(CassieError::Planner(format!(
            "index '{name}' does not exist on collection '{table}'"
        )));
    }

    statement.table = table;
    statement.name = name;
    Ok(statement)
}

async fn bind_drop_table(
    mut statement: crate::sql::ast::DropTableStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::DropTableStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP TABLE requires a table name".into(),
        ));
    }
    if !statement.if_exists && !catalog.exists(&table).await {
        return Err(CassieError::CollectionNotFound(table));
    }
    statement.table = table;
    Ok(statement)
}

async fn bind_alter_table(
    mut statement: AlterTableStatement,
    catalog: &Catalog,
) -> Result<AlterTableStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE requires a table name".into(),
        ));
    }

    let schema = catalog
        .get_schema(&table)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let existing_fields = schema
        .fields
        .into_iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    validate_alter_schema(&table, &statement.operation, &existing_fields)?;

    statement.table = table;
    Ok(statement)
}

fn validate_alter_schema(
    table: &str,
    operation: &AlterTableOperation,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::AddColumn {
            field,
            data_type: _,
        } => {
            let name = field.trim();
            if name.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE ADD COLUMN requires a field name".into(),
                ));
            }

            if existing_fields.contains(&name.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "cannot add existing column '{name}' on collection '{table}'"
                )));
            }
        }
        AlterTableOperation::DropColumn { field } => {
            let name = field.trim();
            if name.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE DROP COLUMN requires a field name".into(),
                ));
            }
            if name.eq_ignore_ascii_case("id") {
                return Err(CassieError::Planner(
                    "ALTER TABLE DROP COLUMN cannot remove reserved field 'id'".into(),
                ));
            }
            if !existing_fields.contains(&name.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "ALTER TABLE '{table}' has no field '{name}'"
                )));
            }
        }
        AlterTableOperation::RenameTo { table: target } => {
            let target = target.trim();
            if target.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME TO requires a table name".into(),
                ));
            }
            if table.eq_ignore_ascii_case(target) {
                return Err(CassieError::Planner(
                    "ALTER TABLE cannot rename collection to same name".into(),
                ));
            }
        }
    }

    Ok(())
}

async fn bind_select(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
) -> Result<SelectStatement, CassieError> {
    let mut scope = outer_scope.clone();
    let mut local_names = HashSet::new();
    let mut select = select;
    let ctes = mem::take(&mut select.ctes);

    let mut bound_ctes = Vec::with_capacity(ctes.len());
    for cte in ctes {
        let cte_name = cte.name.trim();
        if cte_name.is_empty() {
            return Err(CassieError::Planner("CTE name cannot be empty".into()));
        }
        let cte_name_lc = cte_name.to_ascii_lowercase();
        if !local_names.insert(cte_name_lc.clone()) {
            return Err(CassieError::Planner(format!(
                "duplicate CTE name '{cte_name}'"
            )));
        }

        let query = match cte.query {
            CteQuery::Simple(next) => {
                CteQuery::Simple(Box::new(bind_statement(*next, catalog, &scope).await?))
            }
            CteQuery::Recursive { base, recursive } => {
                if cte.aliases.is_empty() {
                    return Err(CassieError::Planner(format!(
                        "recursive CTE '{cte_name}' requires column aliases"
                    )));
                }

                let mut recursive_scope = scope.clone();
                recursive_scope.insert(cte_name_lc.clone(), cte.aliases.clone());

                let bound_base = bind_statement(*base, catalog, &recursive_scope).await?;
                let bound_recursive = bind_statement(*recursive, catalog, &recursive_scope).await?;

                if !recursive_cte_references_self(&bound_recursive, cte_name) {
                    return Err(CassieError::Planner(format!(
                        "recursive CTE '{cte_name}' must reference itself in recursive term"
                    )));
                }

                CteQuery::Recursive {
                    base: Box::new(bound_base),
                    recursive: Box::new(bound_recursive),
                }
            }
        };

        let visible_fields = cte_output_fields(&query).await?;
        let aliases = if cte.aliases.is_empty() {
            visible_fields
        } else {
            if visible_fields.len() != cte.aliases.len() {
                return Err(CassieError::Planner(format!(
                    "CTE '{cte_name}' alias count does not match output columns"
                )));
            }

            cte.aliases
                .iter()
                .map(|alias| alias.to_ascii_lowercase())
                .collect()
        };
        scope.insert(cte_name_lc, aliases);

        bound_ctes.push(crate::sql::ast::CommonTableExpression {
            name: cte.name,
            aliases: cte.aliases,
            query,
        });
    }

    let source_name = match &select.source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.to_string(),
    };
    let source_name_lc = source_name.to_ascii_lowercase();
    let source = if scope.contains_key(&source_name_lc) {
        QuerySource::Cte(source_name)
    } else {
        if !catalog.exists(&source_name).await {
            return Err(CassieError::CollectionNotFound(source_name));
        }
        QuerySource::Collection(source_name)
    };

    let known_fields = source_fields(catalog, &source, &scope).await?;
    select.source = source;
    select.ctes = bound_ctes;

    let projection_aliases = collect_projection_aliases(&select);
    validate_projection_references(&select.projection, &known_fields)?;
    validate_expression_references(
        select.filter.as_ref(),
        &known_fields,
        &projection_aliases,
        false,
    )?;
    validate_order_by_references(&select.order, &known_fields, &projection_aliases)?;
    validate_functions(&select)?;

    Ok(select)
}

async fn cte_output_fields(cte_query: &CteQuery) -> Result<Vec<String>, CassieError> {
    let query = match cte_query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    let QueryStatement::Select(select) = &query.statement else {
        return Err(CassieError::Planner(
            "CTE body must be a SELECT statement".into(),
        ));
    };
    if select.projection.iter().any(matches_wildcard) {
        return Ok(vec!["*".into()]);
    }

    Ok(projected_column_names(&select.projection))
}

fn projected_column_names(projection: &[SelectItem]) -> Vec<String> {
    projection
        .iter()
        .map(|item| match item {
            SelectItem::Wildcard => "*".to_string(),
            SelectItem::Column {
                name: _,
                alias: Some(alias),
                ..
            } => alias.to_ascii_lowercase(),
            SelectItem::Column { name, alias: None } => name.to_ascii_lowercase(),
            SelectItem::Function { function, alias } => alias
                .as_deref()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
        })
        .collect()
}

fn matches_wildcard(item: &SelectItem) -> bool {
    matches!(item, SelectItem::Wildcard)
}

async fn source_fields(
    catalog: &Catalog,
    source: &QuerySource,
    scope: &CteScope,
) -> Result<HashSet<String>, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            let schema = catalog
                .get_schema(name)
                .await
                .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
            Ok(schema
                .fields
                .iter()
                .map(|field| field.name.to_ascii_lowercase())
                .collect())
        }
        QuerySource::Cte(name) => scope
            .get(&name.to_ascii_lowercase())
            .cloned()
            .map(|fields| fields.into_iter().collect())
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone())),
    }
}

fn collect_projection_aliases(select: &SelectStatement) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for item in &select.projection {
        match item {
            SelectItem::Column {
                alias: Some(alias), ..
            }
            | SelectItem::Function {
                alias: Some(alias), ..
            } => {
                aliases.insert(alias.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    aliases
}

fn validate_functions(statement: &SelectStatement) -> Result<(), CassieError> {
    let signatures = crate::sql::functions::registry()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function.arity))
        .collect::<HashMap<_, _>>();

    let mut seen = Vec::new();
    collect_functions(statement, &mut seen);

    for function in seen {
        let Some(arity) = signatures.get(&function.name.to_ascii_lowercase()) else {
            return Err(CassieError::Planner(format!(
                "unsupported function '{}'",
                function.name
            )));
        };
        if function.args.len() != *arity {
            return Err(CassieError::Planner(format!(
                "function '{}' expects {} args, got {}",
                function.name,
                arity,
                function.args.len()
            )));
        }
    }

    Ok(())
}

fn validate_projection_references(
    projection: &[SelectItem],
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in projection {
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                validate_expression(
                    &Expr::Column(name.clone()),
                    known_fields,
                    &HashSet::new(),
                    false,
                )?;
            }
            SelectItem::Function { function, .. } => {
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_expression_references(
    expression: Option<&Expr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    let Some(expression) = expression else {
        return Ok(());
    };
    validate_expression(
        expression,
        known_fields,
        projection_aliases,
        allow_projection_alias,
    )
}

fn validate_order_by_references(
    order: &[crate::sql::ast::OrderExpr],
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in order {
        validate_expression(&item.expr, known_fields, projection_aliases, true)?;
    }
    Ok(())
}

fn validate_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            let name = name.to_ascii_lowercase();
            if known_fields.contains("*") || name == "id" || known_fields.contains(&name) {
                return Ok(());
            }

            if allow_projection_alias && projection_aliases.contains(&name) {
                return Ok(());
            }

            Err(CassieError::Planner(format!(
                "unresolvable column reference '{}'; known fields or aliases required",
                name
            )))
        }
        Expr::Binary { left, right, .. } => {
            validate_expression(
                left,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                right,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )
        }
        Expr::Function(function) => {
            for arg in &function.args {
                validate_expression(
                    arg,
                    known_fields,
                    projection_aliases,
                    allow_projection_alias,
                )?;
            }
            Ok(())
        }
        Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => Ok(()),
    }
}

fn collect_functions(statement: &SelectStatement, out: &mut Vec<FunctionCall>) {
    for item in &statement.projection {
        collect_item(item, out);
    }
    if let Some(expr) = &statement.filter {
        collect_expr(expr, out);
    }
    for order in &statement.order {
        collect_expr(&order.expr, out);
    }
    for cte in &statement.ctes {
        match &cte.query {
            CteQuery::Simple(statement) => {
                if let QueryStatement::Select(select) = &statement.statement {
                    collect_functions(select, out);
                }
            }
            CteQuery::Recursive { base, recursive } => {
                if let QueryStatement::Select(select) = &base.statement {
                    collect_functions(select, out);
                }
                if let QueryStatement::Select(select) = &recursive.statement {
                    collect_functions(select, out);
                }
            }
        }
    }
}

fn collect_item(item: &SelectItem, out: &mut Vec<FunctionCall>) {
    if let SelectItem::Function { function, .. } = item {
        out.push(function.clone());
        for arg in &function.args {
            collect_expr(arg, out);
        }
    }
}

fn collect_expr(expr: &Expr, out: &mut Vec<FunctionCall>) {
    if let Expr::Function(function) = expr {
        out.push(function.clone());
        for arg in &function.args {
            collect_expr(arg, out);
        }
    }
    if let Expr::Binary { left, right, .. } = expr {
        collect_expr(left, out);
        collect_expr(right, out);
    }
}

fn recursive_cte_references_self(statement: &ParsedStatement, cte_name: &str) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => match &select.source {
            QuerySource::Cte(name) | QuerySource::Collection(name) => {
                name.eq_ignore_ascii_case(cte_name)
            }
        },
        _ => false,
    }
}
