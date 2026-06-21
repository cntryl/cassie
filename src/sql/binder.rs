use std::collections::{HashMap, HashSet};
use std::mem;

use crate::app::CassieError;
use crate::catalog::{is_reserved_namespace, virtual_views, Catalog, CollectionSchema, IndexMeta};
use crate::embeddings::DistanceMetric;
use crate::search::bm25;
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CallProcedureStatement, CommonTableExpression, CreateFunctionStatement, CreateIndexStatement,
    CreateProcedureStatement, CreateSchemaStatement, CreateViewStatement, CteQuery,
    DropFunctionStatement, DropIndexStatement, DropProcedureStatement, DropSchemaStatement,
    DropViewStatement, Expr, FunctionCall, InsertSource, OrderExpr, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectSet, SelectStatement,
};
use crate::types::{DataType, FieldSchema, Schema};

type CteScope = HashMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub statement: ParsedStatement,
    pub indexes: Vec<IndexMeta>,
}

pub fn bind(statement: ParsedStatement, catalog: &Catalog) -> Result<BoundStatement, CassieError> {
    let statement = bind_statement(statement, catalog, &HashMap::new())?;
    let indexes = bound_indexes(&statement, catalog);
    Ok(BoundStatement { statement, indexes })
}

fn bound_indexes(statement: &ParsedStatement, catalog: &Catalog) -> Vec<IndexMeta> {
    let Some(collection) = bound_statement_collection(statement) else {
        return Vec::new();
    };
    catalog.list_indexes(&collection)
}

fn bound_statement_collection(statement: &ParsedStatement) -> Option<String> {
    match &statement.statement {
        QueryStatement::Select(select) => source_collection(&select.source),
        QueryStatement::Explain(statement) => bound_statement_collection(&statement.statement),
        _ => None,
    }
}

fn source_collection(source: &QuerySource) -> Option<String> {
    match source {
        QuerySource::Collection(collection) => Some(collection.clone()),
        QuerySource::Subquery { select, .. } => source_collection(&select.source),
        QuerySource::Join { left, .. } => source_collection(left),
        QuerySource::Cte(_) | QuerySource::SingleRow => None,
    }
}

fn bind_statement(
    statement: ParsedStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
) -> Result<ParsedStatement, CassieError> {
    let raw_sql = statement.raw_sql.clone();
    match statement.statement {
        QueryStatement::Select(select) => {
            let select = bind_select(select, catalog, outer_scope)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Select(select),
            })
        }
        QueryStatement::Explain(statement) => {
            let inner = bind_statement(*statement.statement, catalog, outer_scope)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Explain(crate::sql::ast::ExplainStatement {
                    analyze: statement.analyze,
                    statement: Box::new(inner),
                }),
            })
        }
        QueryStatement::Show(statement) => {
            let mut clone = statement.clone();
            clone.variable = clone.variable.trim().to_string();
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Show(clone),
            })
        }
        QueryStatement::Set(statement) => {
            let mut clone = statement.clone();
            clone.variable = clone.variable.trim().to_string();
            clone.value = clone.value.map(|value| value.trim().to_string());
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Set(clone),
            })
        }
        QueryStatement::CreateTable(statement) => {
            let statement = bind_create_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateTable(statement),
            })
        }
        QueryStatement::DropTable(statement) => {
            let statement = bind_drop_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropTable(statement),
            })
        }
        QueryStatement::AlterTable(statement) => {
            let statement = bind_alter_table(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::AlterTable(statement),
            })
        }
        QueryStatement::CreateIndex(statement) => {
            let statement = bind_create_index(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateIndex(statement),
            })
        }
        QueryStatement::DropIndex(statement) => {
            let statement = bind_drop_index(statement, catalog)?;
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

            if is_reserved_namespace(&schema) {
                return Err(CassieError::Unsupported(format!(
                    "namespace '{schema}' is reserved"
                )));
            }
            if !statement.if_not_exists && catalog.namespace_exists(&schema) {
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
        QueryStatement::DropSchema(statement) => {
            let statement = bind_drop_schema(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropSchema(statement),
            })
        }
        QueryStatement::AlterSchema(statement) => {
            let statement = bind_alter_schema(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::AlterSchema(statement),
            })
        }
        QueryStatement::CreateView(statement) => {
            let statement = bind_create_view(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateView(statement),
            })
        }
        QueryStatement::DropView(statement) => {
            let statement = bind_drop_view(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropView(statement),
            })
        }
        QueryStatement::CreateRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::CreateRole(statement),
        }),
        QueryStatement::AlterRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::AlterRole(statement),
        }),
        QueryStatement::DropRole(statement) => Ok(ParsedStatement {
            raw_sql,
            statement: QueryStatement::DropRole(statement),
        }),
        QueryStatement::CreateFunction(statement) => {
            let statement = bind_create_function(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateFunction(statement),
            })
        }
        QueryStatement::DropFunction(statement) => {
            let statement = bind_drop_function(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropFunction(statement),
            })
        }
        QueryStatement::CreateProcedure(statement) => {
            let statement = bind_create_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CreateProcedure(statement),
            })
        }
        QueryStatement::DropProcedure(statement) => {
            let statement = bind_drop_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::DropProcedure(statement),
            })
        }
        QueryStatement::CallProcedure(statement) => {
            let statement = bind_call_procedure(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::CallProcedure(statement),
            })
        }
        QueryStatement::Insert(statement) => {
            let statement = bind_insert(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Insert(statement),
            })
        }
        QueryStatement::Update(statement) => {
            let statement = bind_update(statement, catalog)?;
            Ok(ParsedStatement {
                raw_sql,
                statement: QueryStatement::Update(statement),
            })
        }
        QueryStatement::Delete(statement) => {
            let statement = bind_delete(statement, catalog)?;
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
}

fn bind_insert(
    mut statement: crate::sql::ast::InsertStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::InsertStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "INSERT requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
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
        let source = bind_select(*select, catalog, &HashMap::new())?;
        statement.source = InsertSource::Select(Box::new(source));
    }

    validate_returning_items(&statement.returning, &schema, &table, "INSERT", catalog)?;

    statement.table = table;
    Ok(statement)
}

fn bind_update(
    mut statement: crate::sql::ast::UpdateStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::UpdateStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "UPDATE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
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

    validate_returning_items(&statement.returning, &schema, &table, "UPDATE", catalog)?;

    statement.table = table;
    Ok(statement)
}

fn bind_delete(
    mut statement: crate::sql::ast::DeleteStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::DeleteStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DELETE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    validate_returning_items(&statement.returning, &schema, &table, "DELETE", catalog)?;

    statement.table = table;
    Ok(statement)
}

fn validate_returning_items(
    returning: &[SelectItem],
    schema: &CollectionSchema,
    table: &str,
    operation: &str,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    known_fields.insert("_id".to_string());

    let mut functions = Vec::new();
    for item in returning {
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                if name == "_id" {
                    continue;
                }

                if !schema
                    .fields
                    .iter()
                    .any(|field| field.name.eq_ignore_ascii_case(name))
                {
                    return Err(CassieError::Planner(format!(
                        "{operation} RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            SelectItem::Function { function, .. } => {
                validate_expression(
                    &Expr::Function(function.clone()),
                    &known_fields,
                    &HashSet::new(),
                    false,
                )?;
                collect_item(item, &mut functions);
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression(expr, &known_fields, &HashSet::new(), false)?;
            }
            SelectItem::WindowFunction { .. } => {
                return Err(CassieError::Planner(format!(
                    "{operation} RETURNING does not support window functions"
                )));
            }
        }
    }

    validate_function_calls(functions, catalog)
}

fn bind_create_table(
    mut statement: crate::sql::ast::CreateTableStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::CreateTableStatement, CassieError> {
    let name = statement.table.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires a table name".into(),
        ));
    }
    if !statement.if_not_exists
        && (catalog.relation_exists(&name) || virtual_views::schema(&name).is_some())
    {
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

fn bind_create_view(
    mut statement: CreateViewStatement,
    catalog: &Catalog,
) -> Result<CreateViewStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("CREATE VIEW requires a name".into()));
    }
    if !statement.if_not_exists
        && (catalog.relation_exists(&name) || virtual_views::schema(&name).is_some())
    {
        return Err(CassieError::Planner(format!(
            "relation '{name}' already exists"
        )));
    }

    let parsed = crate::sql::parser::parse_statement(&statement.query)
        .map_err(|error| CassieError::Parse(error.0))?;
    let raw_sql = parsed.raw_sql.clone();
    let QueryStatement::Select(select) = parsed.statement else {
        return Err(CassieError::Planner(
            "CREATE VIEW requires a SELECT query body".into(),
        ));
    };

    let bound = bind_select(select, catalog, &HashMap::new())?;
    if select_contains_parameters(&bound) {
        return Err(CassieError::Planner(
            "CREATE VIEW cannot contain bind parameters".into(),
        ));
    }

    let _schema = infer_select_schema(&bound, catalog)?;

    statement.name = name;
    statement.query = raw_sql;
    Ok(statement)
}

fn bind_drop_view(
    mut statement: DropViewStatement,
    catalog: &Catalog,
) -> Result<DropViewStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP VIEW requires a name".into()));
    }

    if catalog.get_view(&name).is_none() {
        if virtual_views::schema(&name).is_some() || catalog.exists(&name) {
            return Err(CassieError::Planner(format!(
                "relation '{name}' is not a view"
            )));
        }
        if !statement.if_exists {
            return Err(CassieError::Planner(format!(
                "view '{name}' does not exist"
            )));
        }
    }

    statement.name = name;
    Ok(statement)
}

fn bind_create_index(
    mut statement: CreateIndexStatement,
    catalog: &Catalog,
) -> Result<CreateIndexStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires a collection name".into(),
        ));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires an index name".into(),
        ));
    }

    let fields = statement
        .fields
        .iter()
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let expressions = statement.expressions.clone();
    if fields.is_empty() && expressions.is_empty() {
        return Err(CassieError::Planner(
            "CREATE INDEX requires at least one indexed field".into(),
        ));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    if !matches!(statement.kind, crate::catalog::IndexKind::Scalar)
        && fields.len() + expressions.len() > 1
    {
        return Err(CassieError::Planner(
            "composite indexes are only supported for scalar index methods".into(),
        ));
    }
    if !expressions.is_empty() && !matches!(statement.kind, crate::catalog::IndexKind::Scalar) {
        return Err(CassieError::Planner(
            "expression indexes are only supported for scalar indexes".into(),
        ));
    }

    let include_fields = statement
        .include_fields
        .iter()
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if !include_fields.is_empty() && !matches!(statement.kind, crate::catalog::IndexKind::Scalar) {
        return Err(CassieError::Planner(
            "INCLUDE columns are only supported for scalar indexes".into(),
        ));
    }

    for field in &fields {
        let exists = schema.fields.iter().any(|entry| entry.name == *field);
        if !exists {
            return Err(CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            )));
        }
    }
    let known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    for expression in &expressions {
        validate_index_expression(expression, &known_fields)?;
    }
    let mut seen_include_fields = std::collections::BTreeSet::new();
    let key_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for field in &include_fields {
        let normalized = field.to_ascii_lowercase();
        if !seen_include_fields.insert(normalized.clone()) {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' is duplicated"
            )));
        }
        if key_fields.contains(&normalized) {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' duplicates an index key field"
            )));
        }
        let exists = schema.fields.iter().any(|entry| entry.name == *field);
        if !exists {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{field}' does not exist on collection '{table}'"
            )));
        }
    }

    if statement.kind == crate::catalog::IndexKind::Vector {
        let field = &fields[0];
        let field_entry = schema
            .fields
            .iter()
            .find(|entry| entry.name == *field)
            .ok_or_else(|| {
                CassieError::Planner(format!(
                    "index field '{field}' does not exist on collection '{table}'"
                ))
            })?;

        if let Some(existing_vector) = catalog.get_vector_index(&table, field) {
            let existing_index = catalog
                .get_index(&table, &name)
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
        let field = &fields[0];
        let field_entry = schema
            .fields
            .iter()
            .find(|entry| entry.name == *field)
            .ok_or_else(|| {
                CassieError::Planner(format!(
                    "index field '{field}' does not exist on collection '{table}'"
                ))
            })?;

        if !matches!(field_entry.data_type, DataType::Text) {
            return Err(CassieError::Planner(format!(
                "fulltext index '{name}' requires text field '{field}'"
            )));
        }

        let existing_fulltext_index = catalog.list_indexes(&table).into_iter().find(|metadata| {
            metadata.kind == crate::catalog::IndexKind::FullText
                && metadata.field.eq_ignore_ascii_case(field)
        });
        if let Some(existing_fulltext_index) = existing_fulltext_index {
            let existing_index = catalog
                .get_index(&table, &name)
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
        let analyzer =
            crate::search::analyzer::AnalyzerConfig::from_index_options(&statement.options)
                .map_err(CassieError::Planner)?;

        for key in statement.options.keys() {
            if !matches!(
                key.as_str(),
                "boost"
                    | "k1"
                    | "b"
                    | "analyzer"
                    | "case_folding"
                    | "stop_words"
                    | "stemming"
                    | "accent_folding"
            ) {
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
        statement
            .options
            .insert("analyzer".to_string(), analyzer.name);
        statement.options.insert(
            "case_folding".to_string(),
            analyzer.case_folding.to_string(),
        );
        statement
            .options
            .insert("stop_words".to_string(), analyzer.stop_words);
        statement
            .options
            .insert("stemming".to_string(), analyzer.stemming);
        statement.options.insert(
            "accent_folding".to_string(),
            analyzer.accent_folding.to_string(),
        );
    }

    if !statement.if_not_exists && catalog.get_index(&table, &name).is_some() {
        return Err(CassieError::Planner(format!(
            "index '{name}' already exists on collection '{table}'"
        )));
    }

    statement.table = table;
    statement.name = name;
    statement.fields = fields;
    statement.expressions = expressions;
    statement.include_fields = include_fields;
    Ok(statement)
}

fn validate_index_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            if known_fields.contains(name) {
                Ok(())
            } else {
                Err(CassieError::Planner(format!(
                    "index expression references unknown field '{name}'"
                )))
            }
        }
        Expr::Param(_) => Err(CassieError::Planner(
            "index expressions cannot reference query parameters".into(),
        )),
        Expr::Exists(_) => Err(CassieError::Planner(
            "index expressions cannot contain subqueries".into(),
        )),
        Expr::Function(function) => {
            let normalized = function.name.to_ascii_lowercase();
            if crate::sql::functions::is_aggregate_function(&normalized) {
                return Err(CassieError::Planner(format!(
                    "aggregate function '{}' is not allowed in index expressions",
                    function.name
                )));
            }
            let Some(definition) = crate::sql::functions::registry()
                .into_iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(&normalized))
            else {
                return Err(CassieError::Planner(format!(
                    "function '{}' is not allowed in index expressions",
                    function.name
                )));
            };
            if !matches!(
                definition.name,
                "length" | "lower" | "upper" | "substring" | "trim" | "concat" | "coalesce" | "abs"
            ) {
                return Err(CassieError::Planner(format!(
                    "function '{}' is not immutable for index expressions",
                    function.name
                )));
            }
            if !definition.arity.matches(function.args.len()) {
                return Err(CassieError::Planner(format!(
                    "function '{}' expects {}, got {}",
                    function.name,
                    definition.arity.describe(),
                    function.args.len()
                )));
            }
            for arg in &function.args {
                validate_index_expression(arg, known_fields)?;
            }
            Ok(())
        }
        Expr::Binary { left, right, .. } => {
            validate_index_expression(left, known_fields)?;
            validate_index_expression(right, known_fields)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            validate_index_expression(expr, known_fields)
        }
        Expr::InList { expr, values, .. } => {
            validate_index_expression(expr, known_fields)?;
            for value in values {
                validate_index_expression(value, known_fields)?;
            }
            Ok(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_index_expression(expr, known_fields)?;
            validate_index_expression(low, known_fields)?;
            validate_index_expression(high, known_fields)
        }
        Expr::StringLiteral(_) | Expr::NumberLiteral(_) | Expr::BoolLiteral(_) | Expr::Null => {
            Ok(())
        }
    }
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

fn bind_drop_index(
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

    if !catalog.exists(&table) {
        if !statement.if_exists {
            return Err(CassieError::CollectionNotFound(table));
        }
        statement.table = table;
        statement.name = name;
        return Ok(statement);
    }

    if !statement.if_exists && catalog.get_index(&table, &name).is_none() {
        return Err(CassieError::Planner(format!(
            "index '{name}' does not exist on collection '{table}'"
        )));
    }

    statement.table = table;
    statement.name = name;
    Ok(statement)
}

fn bind_drop_schema(
    mut statement: DropSchemaStatement,
    catalog: &Catalog,
) -> Result<DropSchemaStatement, CassieError> {
    let schema = statement.schema.trim().to_string();
    if schema.is_empty() {
        return Err(CassieError::Planner(
            "DROP SCHEMA requires a schema name".into(),
        ));
    }
    if is_reserved_namespace(&schema) {
        return Err(CassieError::Unsupported(format!(
            "namespace '{schema}' is reserved"
        )));
    }
    if !statement.if_exists && !catalog.namespace_exists(&schema) {
        return Err(CassieError::NotFound(format!(
            "namespace '{schema}' does not exist"
        )));
    }

    statement.schema = schema;
    Ok(statement)
}

fn bind_alter_schema(
    mut statement: AlterSchemaStatement,
    catalog: &Catalog,
) -> Result<AlterSchemaStatement, CassieError> {
    let schema = statement.schema.trim().to_string();
    if schema.is_empty() {
        return Err(CassieError::Planner(
            "ALTER SCHEMA requires a schema name".into(),
        ));
    }
    if is_reserved_namespace(&schema) {
        return Err(CassieError::Unsupported(format!(
            "namespace '{schema}' is reserved"
        )));
    }
    if !catalog.namespace_exists(&schema) {
        return Err(CassieError::NotFound(format!(
            "namespace '{schema}' does not exist"
        )));
    }

    match &mut statement.operation {
        AlterSchemaOperation::RenameTo { schema: target } => {
            let next = target.trim().to_string();
            if next.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER SCHEMA RENAME TO requires a schema name".into(),
                ));
            }
            if is_reserved_namespace(&next) {
                return Err(CassieError::Unsupported(format!(
                    "namespace '{next}' is reserved"
                )));
            }
            if schema.eq_ignore_ascii_case(&next) {
                return Err(CassieError::Planner(
                    "ALTER SCHEMA cannot rename namespace to same name".into(),
                ));
            }
            if catalog.namespace_exists(&next) {
                return Err(CassieError::Planner(format!(
                    "namespace '{next}' already exists"
                )));
            }
            *target = next;
        }
    }

    statement.schema = schema;
    Ok(statement)
}

fn bind_drop_table(
    mut statement: crate::sql::ast::DropTableStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::DropTableStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DROP TABLE requires a table name".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Planner(format!(
            "relation '{table}' is a view"
        )));
    }
    if !statement.if_exists && !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    statement.table = table;
    Ok(statement)
}

fn bind_alter_table(
    mut statement: AlterTableStatement,
    catalog: &Catalog,
) -> Result<AlterTableStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "ALTER TABLE requires a table name".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Planner(format!(
            "relation '{table}' is a view"
        )));
    }

    let schema = catalog
        .get_schema(&table)
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
        AlterTableOperation::RenameColumn { from, to } => {
            let from = from.trim();
            if from.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a source field".into(),
                ));
            }
            let to = to.trim();
            if to.is_empty() {
                return Err(CassieError::Planner(
                    "ALTER TABLE RENAME COLUMN requires a target field".into(),
                ));
            }
            if from.eq_ignore_ascii_case(to) {
                return Err(CassieError::Planner(
                    "ALTER TABLE cannot rename column to same name".into(),
                ));
            }
            if !existing_fields.contains(&from.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "ALTER TABLE '{table}' has no field '{from}'"
                )));
            }
            if existing_fields.contains(&to.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "cannot rename column to existing field '{to}' on collection '{table}'"
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

fn bind_select(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
) -> Result<SelectStatement, CassieError> {
    bind_select_with_lateral_fields(select, catalog, outer_scope, &HashSet::new())
}

fn bind_select_with_lateral_fields(
    select: SelectStatement,
    catalog: &Catalog,
    outer_scope: &CteScope,
    lateral_fields: &HashSet<String>,
) -> Result<SelectStatement, CassieError> {
    let mut scope = outer_scope.clone();
    let mut local_names = HashSet::new();
    let mut select = select;
    let ctes = mem::take(&mut select.ctes);
    let set = mem::take(&mut select.set);

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
                CteQuery::Simple(Box::new(bind_statement(*next, catalog, &scope)?))
            }
            CteQuery::Recursive { base, recursive } => {
                if cte.aliases.is_empty() {
                    return Err(CassieError::Planner(format!(
                        "recursive CTE '{cte_name}' requires column aliases"
                    )));
                }

                let mut recursive_scope = scope.clone();
                recursive_scope.insert(cte_name_lc.clone(), cte.aliases.clone());

                let bound_base = bind_statement(*base, catalog, &recursive_scope)?;
                let bound_recursive = bind_statement(*recursive, catalog, &recursive_scope)?;

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

        let visible_fields = cte_output_fields(&query)?;
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

    let source = bind_query_source_with_lateral_fields(
        select.source.clone(),
        catalog,
        &scope,
        lateral_fields,
    )?;
    let mut known_fields = source_fields(catalog, &source, &scope)?;
    known_fields.extend(lateral_fields.iter().cloned());
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
    for group_expr in &select.group_by {
        validate_expression(group_expr, &known_fields, &projection_aliases, false)?;
    }
    for distinct_expr in &select.distinct_on {
        validate_expression(distinct_expr, &known_fields, &projection_aliases, false)?;
    }
    validate_expression_references(
        select.having.as_ref(),
        &known_fields,
        &projection_aliases,
        false,
    )?;
    validate_order_by_references(&select.order, &known_fields, &projection_aliases)?;
    validate_distinct_on_order_prefix(&select.distinct_on, &select.order)?;

    if let Some(set) = set {
        let right = bind_select(*set.right, catalog, &scope)?;
        select.set = Some(Box::new(SelectSet {
            operator: set.operator,
            right: Box::new(right),
        }));
    }

    validate_functions(&select, catalog)?;

    Ok(select)
}

fn bind_create_function(
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

    if !statement.if_not_exists && catalog.get_function(&name).is_some() {
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

fn bind_drop_function(
    mut statement: DropFunctionStatement,
    catalog: &Catalog,
) -> Result<DropFunctionStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP FUNCTION requires a name".into()));
    }

    if !statement.if_exists && catalog.get_function(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "function '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

fn bind_create_procedure(
    mut statement: CreateProcedureStatement,
    catalog: &Catalog,
) -> Result<CreateProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_not_exists && catalog.get_procedure(&name).is_some() {
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

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a body".into(),
        ));
    }

    let parsed_body = crate::sql::parse_statement(&body).map_err(|error| {
        CassieError::Planner(format!("invalid procedure body for '{name}': {}", error.0))
    })?;
    if matches!(parsed_body.statement, QueryStatement::Transaction(_)) {
        return Err(CassieError::Unsupported(
            "transaction control statements inside procedures are not supported in this version"
                .into(),
        ));
    }

    let body_parameter_count = crate::sql::parameter_count(&parsed_body);
    if body_parameter_count > statement.args.len() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' body references ${body_parameter_count} but only {} args are declared",
            statement.args.len()
        )));
    }

    statement.name = name;
    statement.body = body;
    Ok(statement)
}

fn bind_drop_procedure(
    mut statement: DropProcedureStatement,
    catalog: &Catalog,
) -> Result<DropProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_exists && catalog.get_procedure(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

fn bind_call_procedure(
    statement: CallProcedureStatement,
    catalog: &Catalog,
) -> Result<CallProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("CALL requires a name".into()));
    }

    let Some(metadata) = catalog.get_procedure(&name) else {
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
        Expr::Not { expr } => function_body_references(expr, function_name),
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

fn cte_output_fields(cte_query: &CteQuery) -> Result<Vec<String>, CassieError> {
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
            SelectItem::Expr { alias, .. } => {
                alias.as_deref().unwrap_or("expr").to_ascii_lowercase()
            }
            SelectItem::WindowFunction { function, alias } => alias
                .as_deref()
                .unwrap_or(&function.name)
                .to_ascii_lowercase(),
        })
        .collect()
}

fn matches_wildcard(item: &SelectItem) -> bool {
    matches!(item, SelectItem::Wildcard)
}

fn bind_query_source_with_lateral_fields(
    source: QuerySource,
    catalog: &Catalog,
    scope: &CteScope,
    lateral_fields: &HashSet<String>,
) -> Result<QuerySource, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            let source_name_lc = name.to_ascii_lowercase();
            if scope.contains_key(&source_name_lc) {
                Ok(QuerySource::Cte(name))
            } else if catalog.relation_exists(&name) || virtual_views::schema(&name).is_some() {
                Ok(QuerySource::Collection(name))
            } else {
                Err(CassieError::CollectionNotFound(name))
            }
        }
        QuerySource::Cte(name) => Ok(QuerySource::Cte(name)),
        QuerySource::SingleRow => Ok(QuerySource::SingleRow),
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => {
            let empty = HashSet::new();
            let visible_lateral_fields = if lateral { lateral_fields } else { &empty };
            let select =
                bind_select_with_lateral_fields(*select, catalog, scope, visible_lateral_fields)?;
            Ok(QuerySource::Subquery {
                alias,
                select: Box::new(select),
                lateral,
            })
        }
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left =
                bind_query_source_with_lateral_fields(*left, catalog, scope, lateral_fields)?;
            let mut right_lateral_fields = lateral_fields.clone();
            right_lateral_fields.extend(source_fields(catalog, &left, scope)?);
            let right = bind_query_source_with_lateral_fields(
                *right,
                catalog,
                scope,
                &right_lateral_fields,
            )?;
            let joined = QuerySource::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind,
                on: on.clone(),
            };
            let known_fields = source_fields(catalog, &joined, scope)?;
            validate_expression(&on, &known_fields, &HashSet::new(), false)?;
            Ok(joined)
        }
    }
}

fn source_fields(
    catalog: &Catalog,
    source: &QuerySource,
    scope: &CteScope,
) -> Result<HashSet<String>, CassieError> {
    match source {
        QuerySource::Collection(name) => {
            if let Some(fields) = virtual_views::schema(name) {
                Ok(qualified_fields(
                    name,
                    fields.into_iter().map(|(field, _)| field),
                ))
            } else {
                let schema = catalog
                    .get_schema(name)
                    .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
                Ok(qualified_fields(
                    name,
                    schema
                        .fields
                        .iter()
                        .map(|field| field.name.to_ascii_lowercase()),
                ))
            }
        }
        QuerySource::Cte(name) => scope
            .get(&name.to_ascii_lowercase())
            .cloned()
            .map(|fields| qualified_fields(name, fields))
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone())),
        QuerySource::SingleRow => Ok(HashSet::new()),
        QuerySource::Subquery { alias, select, .. } => Ok(qualified_fields(
            alias,
            projected_column_names(&select.projection),
        )),
        QuerySource::Join { left, right, .. } => {
            let mut fields = source_fields(catalog, left, scope)?;
            fields.extend(source_fields(catalog, right, scope)?);
            Ok(fields)
        }
    }
}

pub fn infer_select_schema(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<Schema, CassieError> {
    let user_functions = catalog
        .list_functions()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function))
        .collect::<HashMap<_, _>>();

    infer_select_schema_with_scope(select, catalog, &HashMap::new(), &user_functions)
}

fn infer_select_schema_with_scope(
    select: &SelectStatement,
    catalog: &Catalog,
    outer_ctes: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Schema, CassieError> {
    let mut cte_schemas = outer_ctes.clone();
    for cte in &select.ctes {
        let schema = infer_cte_schema(cte, catalog, &cte_schemas, user_functions)?;
        cte_schemas.insert(cte.name.to_ascii_lowercase(), schema);
    }

    let source_schema =
        infer_source_schema(&select.source, catalog, &cte_schemas, user_functions, false)?;
    let mut fields = infer_projection_schema(&select.projection, &source_schema, user_functions);

    if let Some(set) = &select.set {
        let right_schema =
            infer_select_schema_with_scope(&set.right, catalog, &cte_schemas, user_functions)?;
        if fields.fields.len() != right_schema.fields.len() {
            return Err(CassieError::Planner(format!(
                "set operation column count mismatch: {} != {}",
                fields.fields.len(),
                right_schema.fields.len()
            )));
        }
    }

    for group_expr in &select.group_by {
        if let Expr::Column(name) = group_expr {
            let _ = schema_field_type(&source_schema, name);
        }
    }

    fields.fields.iter_mut().for_each(|field| {
        field.nullable = true;
    });

    Ok(fields)
}

fn infer_cte_schema(
    cte: &CommonTableExpression,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<Schema, CassieError> {
    let query = match &cte.query {
        CteQuery::Simple(statement) => statement,
        CteQuery::Recursive { base, .. } => base,
    };

    let QueryStatement::Select(select) = &query.statement else {
        return Err(CassieError::Planner(
            "CTE body must be a SELECT statement".into(),
        ));
    };

    let mut schema = infer_select_schema_with_scope(select, catalog, cte_schemas, user_functions)?;

    if !cte.aliases.is_empty() {
        if schema.fields.len() != cte.aliases.len() {
            return Err(CassieError::Planner(format!(
                "CTE '{}' alias count does not match output columns",
                cte.name
            )));
        }

        for (field, alias) in schema.fields.iter_mut().zip(cte.aliases.iter()) {
            field.name = alias.clone();
        }
    }

    Ok(schema)
}

fn infer_source_schema(
    source: &QuerySource,
    catalog: &Catalog,
    cte_schemas: &HashMap<String, Schema>,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    qualify: bool,
) -> Result<Schema, CassieError> {
    let schema = match source {
        QuerySource::Collection(name) => relation_output_schema(catalog, name)?,
        QuerySource::Cte(name) => cte_schemas
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?,
        QuerySource::SingleRow => Schema { fields: Vec::new() },
        QuerySource::Subquery { alias, select, .. } => {
            let inner =
                infer_select_schema_with_scope(select, catalog, cte_schemas, user_functions)?;
            qualify_schema(&inner, alias)
        }
        QuerySource::Join { left, right, .. } => {
            let left = infer_source_schema(left, catalog, cte_schemas, user_functions, true)?;
            let right = infer_source_schema(right, catalog, cte_schemas, user_functions, true)?;
            let mut fields = left.fields;
            fields.extend(right.fields);
            Schema { fields }
        }
    };

    if qualify {
        Ok(match source {
            QuerySource::Collection(name) | QuerySource::Cte(name) => qualify_schema(&schema, name),
            QuerySource::SingleRow | QuerySource::Subquery { .. } | QuerySource::Join { .. } => {
                schema
            }
        })
    } else {
        Ok(schema)
    }
}

fn relation_output_schema(catalog: &Catalog, name: &str) -> Result<Schema, CassieError> {
    if let Some(fields) = virtual_views::schema(name) {
        return Ok(Schema {
            fields: fields
                .into_iter()
                .map(|(field_name, data_type)| FieldSchema {
                    name: field_name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        });
    }

    if let Some(view) = catalog.get_view(name) {
        return Ok(view.schema);
    }

    let schema = catalog
        .get_schema(name)
        .ok_or_else(|| CassieError::CollectionNotFound(name.to_string()))?;

    let mut fields = Vec::with_capacity(schema.fields.len() + 1);
    fields.push(FieldSchema {
        name: "id".to_string(),
        data_type: DataType::Text,
        nullable: true,
    });
    fields.extend(schema.fields.into_iter().map(|field| FieldSchema {
        name: field.name,
        data_type: field.data_type,
        nullable: true,
    }));

    Ok(Schema { fields })
}

fn qualify_schema(schema: &Schema, qualifier: &str) -> Schema {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut fields = Vec::with_capacity(schema.fields.len() * 2);
    for field in &schema.fields {
        fields.push(field.clone());
        fields.push(FieldSchema {
            name: format!("{qualifier}.{}", field.name),
            data_type: field.data_type.clone(),
            nullable: field.nullable,
        });
    }
    Schema { fields }
}

fn infer_projection_schema(
    projection: &[SelectItem],
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Schema {
    let mut fields = Vec::new();
    for item in projection {
        match item {
            SelectItem::Wildcard => fields.extend(source_schema.fields.iter().cloned()),
            SelectItem::Column { name, alias } => {
                let output_name = alias.clone().unwrap_or_else(|| name.clone());
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: schema_field_type(source_schema, name).unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Function { function, alias } => {
                let output_name = alias
                    .as_deref()
                    .unwrap_or(function.name.as_str())
                    .to_string();
                fields.push(FieldSchema {
                    name: output_name,
                    data_type: infer_function_return_type(function, source_schema, user_functions)
                        .unwrap_or(DataType::Text),
                    nullable: true,
                });
            }
            SelectItem::Expr { alias, .. } => {
                fields.push(FieldSchema {
                    name: alias.as_deref().unwrap_or("expr").to_string(),
                    data_type: DataType::Float,
                    nullable: true,
                });
            }
            SelectItem::WindowFunction { function, alias } => {
                fields.push(FieldSchema {
                    name: alias
                        .as_deref()
                        .unwrap_or(function.name.as_str())
                        .to_string(),
                    data_type: DataType::BigInt,
                    nullable: false,
                });
            }
        }
    }

    Schema { fields }
}

fn schema_field_type(schema: &Schema, name: &str) -> Option<DataType> {
    schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(name))
        .map(|field| field.data_type.clone())
}

fn infer_function_return_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Option<DataType> {
    let name = function.name.to_ascii_lowercase();
    if let Some(metadata) = user_functions.get(&name) {
        return Some(metadata.return_type.clone());
    }

    match name.as_str() {
        "count" => Some(DataType::Int),
        "sum" | "avg" => Some(DataType::Float),
        "min" | "max" => Some(DataType::Text),
        "length" | "len" => Some(DataType::Int),
        "lower" | "upper" | "substring" | "trim" | "concat" => Some(DataType::Text),
        "coalesce" => function
            .args
            .iter()
            .find_map(|arg| infer_expr_type(arg, source_schema))
            .filter(|data_type| !matches!(data_type, DataType::Null))
            .or(Some(DataType::Text)),
        "abs" => function
            .args
            .first()
            .and_then(|expr| infer_expr_type(expr, source_schema))
            .map(|data_type| match data_type {
                DataType::Int => DataType::Int,
                DataType::Float => DataType::Float,
                _ => DataType::Float,
            })
            .or(Some(DataType::Float)),
        "search" | "search_score" | "vector_distance" | "vector_score" | "cosine_distance"
        | "dot_product" | "hybrid_score" => Some(DataType::Float),
        "snippet" | "version" | "current_schema" | "current_database" | "current_user"
        | "session_user" | "current_role" => Some(DataType::Text),
        _ => None,
    }
}

fn infer_expr_type(expr: &Expr, source_schema: &Schema) -> Option<DataType> {
    match expr {
        Expr::Column(name) => schema_field_type(source_schema, name),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}

fn select_contains_parameters(select: &SelectStatement) -> bool {
    select.ctes.iter().any(cte_contains_parameters)
        || source_contains_parameters(&select.source)
        || select
            .projection
            .iter()
            .any(select_item_contains_parameters)
        || select.filter.as_ref().is_some_and(expr_contains_parameters)
        || select.distinct_on.iter().any(expr_contains_parameters)
        || select.group_by.iter().any(expr_contains_parameters)
        || select.having.as_ref().is_some_and(expr_contains_parameters)
        || select
            .order
            .iter()
            .any(|order| expr_contains_parameters(&order.expr))
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_contains_parameters(&set.right))
}

fn cte_contains_parameters(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_contains_parameters(statement.as_ref()),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_contains_parameters(base.as_ref())
                || parsed_statement_contains_parameters(recursive.as_ref())
        }
    }
}

fn parsed_statement_contains_parameters(statement: &ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Explain(statement) => {
            parsed_statement_contains_parameters(statement.statement.as_ref())
        }
        QueryStatement::Select(select) => select_contains_parameters(select),
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Insert(_)
        | QueryStatement::Update(_)
        | QueryStatement::Delete(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::DropView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_)
        | QueryStatement::CreateFunction(_)
        | QueryStatement::DropFunction(_)
        | QueryStatement::CreateProcedure(_)
        | QueryStatement::DropProcedure(_)
        | QueryStatement::CallProcedure(_) => false,
    }
}

fn select_item_contains_parameters(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard => false,
        SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function.args.iter().any(expr_contains_parameters),
        SelectItem::Expr { expr, .. } => expr_contains_parameters(expr),
        SelectItem::WindowFunction { function, .. } => {
            function.args.iter().any(expr_contains_parameters)
                || function.partition_by.iter().any(expr_contains_parameters)
                || function
                    .order_by
                    .iter()
                    .any(|order| expr_contains_parameters(&order.expr))
        }
    }
}

fn source_contains_parameters(source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        QuerySource::Subquery { select, .. } => select_contains_parameters(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_contains_parameters(left)
                || source_contains_parameters(right)
                || expr_contains_parameters(on)
        }
    }
}

fn expr_contains_parameters(expr: &Expr) -> bool {
    match expr {
        Expr::Param(_) => true,
        Expr::Binary { left, right, .. } => {
            expr_contains_parameters(left) || expr_contains_parameters(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_contains_parameters(expr),
        Expr::InList { expr, values, .. } => {
            expr_contains_parameters(expr) || values.iter().any(expr_contains_parameters)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_contains_parameters(expr)
                || expr_contains_parameters(low)
                || expr_contains_parameters(high)
        }
        Expr::Not { expr } => expr_contains_parameters(expr),
        Expr::Exists(statement) => parsed_statement_contains_parameters(statement),
        Expr::Function(function) => function.args.iter().any(expr_contains_parameters),
        Expr::Column(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => false,
    }
}

fn qualified_fields(qualifier: &str, fields: impl IntoIterator<Item = String>) -> HashSet<String> {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut out = HashSet::new();
    for field in fields {
        let field = field.to_ascii_lowercase();
        out.insert(field.clone());
        out.insert(format!("{qualifier}.{field}"));
    }
    out
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
            }
            | SelectItem::WindowFunction {
                alias: Some(alias), ..
            } => {
                aliases.insert(alias.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    aliases
}

fn validate_functions(statement: &SelectStatement, catalog: &Catalog) -> Result<(), CassieError> {
    let mut seen = Vec::new();
    collect_functions(statement, &mut seen);
    validate_function_calls(seen, catalog)
}

fn validate_function_calls(
    functions: Vec<FunctionCall>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut signatures = crate::sql::functions::registry()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function.arity))
        .collect::<HashMap<_, _>>();

    for function in catalog.list_functions() {
        signatures.insert(
            function.name.to_ascii_lowercase(),
            crate::sql::functions::FunctionArity::Exact(function.args.len()),
        );
    }

    for function in functions {
        if function.name.eq_ignore_ascii_case("cast") {
            if function.args.len() != 2 {
                return Err(CassieError::Planner(format!(
                    "function '{}' expects 2 args",
                    function.name
                )));
            }
            continue;
        }
        if let Some(arity) = crate::sql::functions::aggregate_arity(&function.name) {
            if function.args.len() != arity {
                return Err(CassieError::Planner(format!(
                    "aggregate function '{}' expects {} args, got {}",
                    function.name,
                    arity,
                    function.args.len()
                )));
            }
            continue;
        }
        let Some(arity) = signatures.get(&function.name.to_ascii_lowercase()) else {
            return Err(CassieError::Planner(format!(
                "unsupported function '{}'",
                function.name
            )));
        };
        if !arity.matches(function.args.len()) {
            return Err(CassieError::Planner(format!(
                "function '{}' expects {}, got {}",
                function.name,
                arity.describe(),
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
                if crate::sql::functions::is_aggregate_function(&function.name) {
                    validate_aggregate_function_args(function, known_fields)?;
                    continue;
                }
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression(expr, known_fields, &HashSet::new(), false)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
                for expr in &function.partition_by {
                    validate_expression(expr, known_fields, &HashSet::new(), false)?;
                }
                for order in &function.order_by {
                    validate_expression(&order.expr, known_fields, &HashSet::new(), false)?;
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

fn validate_distinct_on_order_prefix(
    distinct_on: &[Expr],
    order: &[OrderExpr],
) -> Result<(), CassieError> {
    if distinct_on.is_empty() {
        return Ok(());
    }
    if order.len() < distinct_on.len() {
        return Err(CassieError::Planner(
            "DISTINCT ON expressions must match the leading ORDER BY expressions".to_string(),
        ));
    }
    for (distinct_expr, order_expr) in distinct_on.iter().zip(order.iter()) {
        if !distinct_on_expr_matches_order(distinct_expr, &order_expr.expr) {
            return Err(CassieError::Planner(
                "DISTINCT ON expressions must match the leading ORDER BY expressions".to_string(),
            ));
        }
    }
    Ok(())
}

fn distinct_on_expr_matches_order(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Column(left), Expr::Column(right)) => left.eq_ignore_ascii_case(right),
        _ => format!("{left:?}") == format!("{right:?}"),
    }
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
        Expr::Not { expr } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::Cast { expr, .. } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::Exists(_) => Ok(()),
        Expr::Function(function) => {
            if crate::sql::functions::is_aggregate_function(&function.name) {
                validate_aggregate_function_args(function, known_fields)?;
                return Ok(());
            }
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

fn validate_aggregate_function_args(
    function: &FunctionCall,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let Some(arity) = crate::sql::functions::aggregate_arity(&function.name) else {
        return Ok(());
    };
    if function.args.len() != arity {
        return Err(CassieError::Planner(format!(
            "aggregate function '{}' expects {} args, got {}",
            function.name,
            arity,
            function.args.len()
        )));
    }
    if function.name.eq_ignore_ascii_case("count")
        && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*")
    {
        return Ok(());
    }
    for arg in &function.args {
        validate_expression(arg, known_fields, &HashSet::new(), false)?;
    }
    Ok(())
}

fn collect_functions(statement: &SelectStatement, out: &mut Vec<FunctionCall>) {
    for item in &statement.projection {
        collect_item(item, out);
    }
    if let Some(expr) = &statement.filter {
        collect_expr(expr, out);
    }
    if let Some(expr) = &statement.having {
        collect_expr(expr, out);
    }
    for expr in &statement.distinct_on {
        collect_expr(expr, out);
    }
    for expr in &statement.group_by {
        collect_expr(expr, out);
    }
    for order in &statement.order {
        collect_expr(&order.expr, out);
    }
    if let Some(set) = &statement.set {
        collect_functions(&set.right, out);
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
    match item {
        SelectItem::Function { function, .. } => {
            out.push(function.clone());
            for arg in &function.args {
                collect_expr(arg, out);
            }
        }
        SelectItem::WindowFunction { function, .. } => {
            for arg in &function.args {
                collect_expr(arg, out);
            }
            for expr in &function.partition_by {
                collect_expr(expr, out);
            }
            for order in &function.order_by {
                collect_expr(&order.expr, out);
            }
        }
        SelectItem::Expr { expr, .. } => {
            collect_expr(expr, out);
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
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
    if let Expr::Not { expr } = expr {
        collect_expr(expr, out);
    }
}

fn recursive_cte_references_self(statement: &ParsedStatement, cte_name: &str) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => source_references_cte(&select.source, cte_name),
        _ => false,
    }
}

fn source_references_cte(source: &QuerySource, cte_name: &str) -> bool {
    match source {
        QuerySource::Cte(name) | QuerySource::Collection(name) => {
            name.eq_ignore_ascii_case(cte_name)
        }
        QuerySource::SingleRow => false,
        QuerySource::Subquery { select, .. } => source_references_cte(&select.source, cte_name),
        QuerySource::Join { left, right, .. } => {
            source_references_cte(left, cte_name) || source_references_cte(right, cte_name)
        }
    }
}
