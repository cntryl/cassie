use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::mem;
use std::pin::Pin;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::embeddings::DistanceMetric;
use crate::search::bm25;
use crate::sql::ast::{
    AlterTableOperation, AlterTableStatement, CallProcedureStatement, CreateFunctionStatement,
    CreateIndexStatement, CreateProcedureStatement, CreateSchemaStatement, CteQuery,
    DropFunctionStatement, DropIndexStatement, DropProcedureStatement, Expr, FunctionCall,
    InsertSource, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};
use crate::types::DataType;

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
            QueryStatement::CreateFunction(statement) => {
                let statement = bind_create_function(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CreateFunction(statement),
                })
            }
            QueryStatement::DropFunction(statement) => {
                let statement = bind_drop_function(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::DropFunction(statement),
                })
            }
            QueryStatement::CreateProcedure(statement) => {
                let statement = bind_create_procedure(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CreateProcedure(statement),
                })
            }
            QueryStatement::DropProcedure(statement) => {
                let statement = bind_drop_procedure(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::DropProcedure(statement),
                })
            }
            QueryStatement::CallProcedure(statement) => {
                let statement = bind_call_procedure(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::CallProcedure(statement),
                })
            }
            QueryStatement::Insert(statement) => {
                let statement = bind_insert(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::Insert(statement),
                })
            }
            QueryStatement::Update(statement) => {
                let statement = bind_update(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::Update(statement),
                })
            }
            QueryStatement::Delete(statement) => {
                let statement = bind_delete(statement, catalog).await?;
                Ok(ParsedStatement {
                    raw_sql,
                    statement: QueryStatement::Delete(statement),
                })
            }
            QueryStatement::Transaction(statement) => Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Transaction(statement),
            }),
        }
    })
}

async fn bind_insert(
    mut statement: crate::sql::ast::InsertStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::InsertStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "INSERT requires a target table".into(),
        ));
    }
    if !catalog.exists(&table).await {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let mut seen_columns = HashSet::new();
    for column in statement.columns.iter_mut() {
        let column_name = column.trim().to_string();
        if column_name.is_empty() {
            return Err(CassieError::Planner(
                "INSERT column names cannot be empty".into(),
            ));
        }

        if !schema
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(&column_name))
        {
            return Err(CassieError::Planner(format!(
                "INSERT target column '{column_name}' does not exist in '{table}'"
            )));
        }

        if !seen_columns.insert(column_name.clone()) {
            return Err(CassieError::Planner(format!(
                "INSERT column '{column_name}' is duplicated"
            )));
        }

        *column = column_name;
    }

    if let InsertSource::Select(select) = statement.source {
        let source = bind_select(select, catalog, &HashMap::new()).await?;
        statement.source = InsertSource::Select(source);
    }

    for item in &statement.returning {
        match item {
            crate::sql::ast::SelectItem::Wildcard => {}
            crate::sql::ast::SelectItem::Column { name, .. } => {
                if name == "_id" {
                    continue;
                }

                if !schema
                    .fields
                    .iter()
                    .any(|field| field.name.eq_ignore_ascii_case(name))
                {
                    return Err(CassieError::Planner(format!(
                        "INSERT RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            crate::sql::ast::SelectItem::Function { function, .. } => {
                return Err(CassieError::Unsupported(format!(
                    "INSERT RETURNING function '{}' is not supported in this version",
                    function.name
                )));
            }
        }
    }

    statement.table = table;
    Ok(statement)
}

async fn bind_update(
    mut statement: crate::sql::ast::UpdateStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::UpdateStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "UPDATE requires a target table".into(),
        ));
    }
    if !catalog.exists(&table).await {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let mut seen = HashSet::new();
    for (field, _) in &mut statement.assignments {
        let normalized_field = field.trim().to_string();
        if normalized_field.is_empty() {
            return Err(CassieError::Planner(
                "UPDATE assignment names cannot be empty".into(),
            ));
        }
        if !schema
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&normalized_field))
        {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' does not exist in '{table}'"
            )));
        }

        if !seen.insert(normalized_field.clone()) {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' is duplicated"
            )));
        }

        *field = normalized_field;
    }

    for item in &statement.returning {
        match item {
            crate::sql::ast::SelectItem::Wildcard => {}
            crate::sql::ast::SelectItem::Column { name, .. } => {
                if name == "_id" {
                    continue;
                }

                if !schema
                    .fields
                    .iter()
                    .any(|field| field.name.eq_ignore_ascii_case(name))
                {
                    return Err(CassieError::Planner(format!(
                        "UPDATE RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            crate::sql::ast::SelectItem::Function { function, .. } => {
                return Err(CassieError::Unsupported(format!(
                    "UPDATE RETURNING function '{}' is not supported in this version",
                    function.name
                )));
            }
        }
    }

    statement.table = table;
    Ok(statement)
}

async fn bind_delete(
    mut statement: crate::sql::ast::DeleteStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::DeleteStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DELETE requires a target table".into(),
        ));
    }
    if !catalog.exists(&table).await {
        return Err(CassieError::CollectionNotFound(table));
    }
    let schema = catalog
        .get_schema(&table)
        .await
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    for item in &statement.returning {
        match item {
            crate::sql::ast::SelectItem::Wildcard => {}
            crate::sql::ast::SelectItem::Column { name, .. } => {
                if name == "_id" {
                    continue;
                }

                if !schema
                    .fields
                    .iter()
                    .any(|field| field.name.eq_ignore_ascii_case(name))
                {
                    return Err(CassieError::Planner(format!(
                        "DELETE RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            crate::sql::ast::SelectItem::Function { function, .. } => {
                return Err(CassieError::Unsupported(format!(
                    "DELETE RETURNING function '{}' is not supported in this version",
                    function.name
                )));
            }
        }
    }

    statement.table = table;
    Ok(statement)
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
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            ))
        })?;

    if statement.kind == crate::catalog::IndexKind::Vector {
        if let Some(existing_vector) = catalog.get_vector_index(&table, &field).await {
            let existing_index = catalog
                .get_index(&table, &name)
                .await
                .filter(|metadata| metadata.field == existing_vector.field)
                .filter(|metadata| metadata.kind == crate::catalog::IndexKind::Vector);

            if existing_index.is_none() {
                return Err(CassieError::Planner(format!(
                    "vector index on field '{}' already exists on collection '{}'",
                    existing_vector.field, table
                )));
            }
        }

        if !matches!(field_entry.data_type, DataType::Vector(_)) {
            return Err(CassieError::Planner(format!(
                "vector index '{name}' requires vector field '{field}'"
            )));
        }

        let source_field = statement
            .options
            .get("source_field")
            .map(std::string::String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CassieError::Planner("CREATE INDEX USING vector requires source_field".into())
            })?;

        let source_entry = schema
            .fields
            .iter()
            .find(|entry| entry.name == source_field)
            .ok_or_else(|| {
                CassieError::Planner(format!(
                    "source field '{source_field}' does not exist on collection '{table}'"
                ))
            })?;

        if !matches!(source_entry.data_type, DataType::Text | DataType::Json) {
            return Err(CassieError::Planner(format!(
                "source field '{source_field}' must be text/json for vector index"
            )));
        }

        let metric = parse_vector_metric(statement.options.get("metric").map(String::as_str))?;
        statement
            .options
            .insert("metric".to_string(), metric.as_str().to_string());
    }

    if statement.kind == crate::catalog::IndexKind::FullText {
        if !matches!(field_entry.data_type, DataType::Text) {
            return Err(CassieError::Planner(format!(
                "fulltext index '{name}' requires text field '{field}'"
            )));
        }

        let existing_fulltext_index =
            catalog
                .list_indexes(&table)
                .await
                .into_iter()
                .find(|metadata| {
                    metadata.kind == crate::catalog::IndexKind::FullText
                        && metadata.field.eq_ignore_ascii_case(&field)
                });
        if let Some(existing_fulltext_index) = existing_fulltext_index {
            let existing_index = catalog
                .get_index(&table, &name)
                .await
                .filter(|metadata| metadata.kind == crate::catalog::IndexKind::FullText)
                .filter(|metadata| {
                    metadata
                        .field
                        .eq_ignore_ascii_case(&existing_fulltext_index.field)
                });

            if existing_index.is_none() {
                return Err(CassieError::Planner(format!(
                    "fulltext index on field '{field}' already exists on collection '{table}'"
                )));
            }
        }

        let boost = parse_fulltext_index_float_option(
            "boost",
            statement
                .options
                .get("boost")
                .map(std::string::String::as_str),
            bm25::DEFAULT_FULLTEXT_BOOST,
            0.0,
            None,
        )?;

        let k1 = parse_fulltext_index_float_option(
            "k1",
            statement.options.get("k1").map(std::string::String::as_str),
            bm25::DEFAULT_BM25_K1,
            0.0,
            None,
        )?;

        let b = parse_fulltext_index_float_option(
            "b",
            statement.options.get("b").map(std::string::String::as_str),
            bm25::DEFAULT_BM25_B,
            0.0,
            Some(1.0),
        )?;

        for key in statement.options.keys() {
            if !matches!(key.as_str(), "boost" | "k1" | "b") {
                return Err(CassieError::Planner(format!(
                    "unsupported fulltext index option '{key}' for '{name}' on collection '{table}'"
                )));
            }
        }

        statement
            .options
            .insert("boost".to_string(), boost.to_string());
        statement.options.insert("k1".to_string(), k1.to_string());
        statement.options.insert("b".to_string(), b.to_string());
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

fn parse_vector_metric(raw_metric: Option<&str>) -> Result<DistanceMetric, CassieError> {
    let metric = raw_metric.unwrap_or("cosine");
    metric.parse().map_err(|_| {
        CassieError::Planner(format!(
            "unsupported vector metric '{metric}' (expected cosine, l2, or dot)"
        ))
    })
}

fn parse_fulltext_index_float_option(
    key: &str,
    value: Option<&str>,
    default: f64,
    min: f64,
    max: Option<f64>,
) -> Result<f64, CassieError> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Ok(default);
    }

    let parsed = value
        .parse::<f64>()
        .map_err(|_| CassieError::Planner(format!("invalid {key} value '{value}'")))?;

    if !parsed.is_finite() {
        return Err(CassieError::Planner(format!(
            "fulltext index option '{key}' must be finite"
        )));
    }

    let range_ok = if let Some(max) = max {
        parsed >= min && parsed <= max
    } else {
        parsed >= min
    };

    if !range_ok {
        return match max {
            Some(max) => Err(CassieError::Planner(format!(
                "fulltext index option '{key}' must be in [{min}, {max}]"
            ))),
            None => Err(CassieError::Planner(format!(
                "fulltext index option '{key}' must be at least {min}"
            ))),
        };
    }

    Ok(parsed)
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
        return Err(CassieError::Planner(
            "DROP INDEX requires an index name".into(),
        ));
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
    validate_functions(&select, catalog).await?;

    Ok(select)
}

async fn bind_create_function(
    mut statement: CreateFunctionStatement,
    catalog: &Catalog,
) -> Result<CreateFunctionStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a name".into(),
        ));
    }

    if crate::sql::functions::registry()
        .iter()
        .any(|function| function.name.eq_ignore_ascii_case(&name))
    {
        return Err(CassieError::Planner(format!(
            "cannot create function '{name}' because it conflicts with built-in function"
        )));
    }

    if !statement.if_not_exists && catalog.get_function(&name).await.is_some() {
        return Err(CassieError::Planner(format!(
            "function '{name}' already exists"
        )));
    }

    let mut seen = HashSet::new();
    for arg in &mut statement.args {
        let arg_name = arg.name.trim().to_string();
        if arg_name.is_empty() {
            return Err(CassieError::Planner(
                "function argument name cannot be empty".into(),
            ));
        }
        let key = arg_name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(CassieError::Planner(format!(
                "function '{name}' has duplicate argument '{arg_name}'"
            )));
        }
        arg.name = arg_name;
    }

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a body".into(),
        ));
    }
    let parsed_body = crate::sql::parser::parse_expression(&body).map_err(|error| {
        CassieError::Planner(format!("invalid function body for '{name}': {}", error.0))
    })?;

    if function_body_references(&parsed_body, &name) {
        return Err(CassieError::Planner(format!(
            "function '{name}' cannot call itself"
        )));
    }

    statement.name = name;
    statement.body = body;
    Ok(statement)
}

async fn bind_drop_function(
    mut statement: DropFunctionStatement,
    catalog: &Catalog,
) -> Result<DropFunctionStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP FUNCTION requires a name".into()));
    }

    if !statement.if_exists && catalog.get_function(&name).await.is_none() {
        return Err(CassieError::Planner(format!(
            "function '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

async fn bind_create_procedure(
    mut statement: CreateProcedureStatement,
    catalog: &Catalog,
) -> Result<CreateProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_not_exists && catalog.get_procedure(&name).await.is_some() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' already exists"
        )));
    }

    let mut seen = HashSet::new();
    for arg in &mut statement.args {
        let arg_name = arg.name.trim().to_string();
        if arg_name.is_empty() {
            return Err(CassieError::Planner(
                "procedure argument name cannot be empty".into(),
            ));
        }
        let key = arg_name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(CassieError::Planner(format!(
                "procedure '{name}' has duplicate argument '{arg_name}'"
            )));
        }
        arg.name = arg_name;
    }

    statement.name = name;
    Ok(statement)
}

async fn bind_drop_procedure(
    mut statement: DropProcedureStatement,
    catalog: &Catalog,
) -> Result<DropProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_exists && catalog.get_procedure(&name).await.is_none() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

async fn bind_call_procedure(
    statement: CallProcedureStatement,
    catalog: &Catalog,
) -> Result<CallProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("CALL requires a name".into()));
    }

    let Some(metadata) = catalog.get_procedure(&name).await else {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' does not exist"
        )));
    };

    if statement.args.len() != metadata.args.len() {
        return Err(CassieError::Planner(format!(
            "procedure '{}' expects {} args, got {}",
            name,
            metadata.args.len(),
            statement.args.len()
        )));
    }

    let mut bound = statement;
    bound.name = name;
    Ok(bound)
}

fn function_body_references(expr: &Expr, function_name: &str) -> bool {
    let normalized = function_name.to_ascii_lowercase();
    match expr {
        Expr::Function(function) => {
            function.name.eq_ignore_ascii_case(&normalized)
                || function
                    .args
                    .iter()
                    .any(|arg| function_body_references(arg, function_name))
        }
        Expr::Binary { left, right, .. } => {
            function_body_references(left, function_name)
                || function_body_references(right, function_name)
        }
        Expr::IsNull { expr, .. } => function_body_references(expr, function_name),
        Expr::InList { expr, values, .. } => {
            function_body_references(expr, function_name)
                || values
                    .iter()
                    .any(|value| function_body_references(value, function_name))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            function_body_references(expr, function_name)
                || function_body_references(low, function_name)
                || function_body_references(high, function_name)
        }
        Expr::Cast { expr, .. } => function_body_references(expr, function_name),
        Expr::Exists(_) => false,
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Param(_)
        | Expr::Column(_)
        | Expr::Null => false,
    }
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

async fn validate_functions(
    statement: &SelectStatement,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut signatures = crate::sql::functions::registry()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function.arity))
        .collect::<HashMap<_, _>>();

    for function in catalog.list_functions().await {
        signatures.insert(function.name.to_ascii_lowercase(), function.args.len());
    }

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
        Expr::IsNull { expr, .. } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::InList { expr, values, .. } => {
            validate_expression(
                expr,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            for value in values {
                validate_expression(
                    value,
                    known_fields,
                    projection_aliases,
                    allow_projection_alias,
                )?;
            }
            Ok(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_expression(
                expr,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                low,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                high,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )
        }
        Expr::Cast { expr, .. } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::Exists(_) => Ok(()),
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
    if let Expr::IsNull { expr, .. } = expr {
        collect_expr(expr, out);
    }
    if let Expr::InList { expr, values, .. } = expr {
        collect_expr(expr, out);
        for value in values {
            collect_expr(value, out);
        }
    }
    if let Expr::Between {
        expr, low, high, ..
    } = expr
    {
        collect_expr(expr, out);
        collect_expr(low, out);
        collect_expr(high, out);
    }
    if let Expr::Cast { expr, .. } = expr {
        collect_expr(expr, out);
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
