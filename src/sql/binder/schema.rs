use super::*;

#[path = "schema_index_options.rs"]
mod schema_index_options;

pub(super) fn bind_create_table(
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

    if matches!(
        statement.storage_mode,
        crate::catalog::CollectionStorageMode::ColumnIndexed
    ) {
        return Err(CassieError::Planner(
            "CREATE TABLE storage mode 'column_indexed' is derived and cannot be created explicitly"
                .into(),
        ));
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
            if let (Some(table), Some(reference_field)) = (
                constraint.references_table.as_deref(),
                constraint.references_field.as_deref(),
            ) {
                if !catalog.exists(table) {
                    return Err(CassieError::CollectionNotFound(table.to_string()));
                }
                let referenced_schema = catalog
                    .get_schema(table)
                    .ok_or_else(|| CassieError::CollectionNotFound(table.to_string()))?;
                if !referenced_schema
                    .fields
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(reference_field))
                {
                    return Err(CassieError::Planner(format!(
                        "foreign key on '{field_name}' references missing field '{reference_field}' on '{table}'"
                    )));
                }

                let references_supported =
                    catalog.get_constraints(table).into_iter().any(|candidate| {
                        candidate.field.eq_ignore_ascii_case(reference_field)
                            && (candidate.primary_key || candidate.unique)
                    }) || catalog
                        .list_indexes(table)
                        .into_iter()
                        .filter(|index| {
                            index.unique && index.kind == crate::catalog::IndexKind::Scalar
                        })
                        .any(|index| {
                            let fields = index.normalized_fields();
                            fields.len() == 1 && fields[0].eq_ignore_ascii_case(reference_field)
                        });

                if !references_supported {
                    return Err(CassieError::Planner(format!(
                        "foreign key on '{field_name}' must reference a primary or unique key on '{table}.{reference_field}'"
                    )));
                }
            }
        }

        field.name = field_name.to_string();
    }

    statement.table = name;
    Ok(statement)
}

pub(super) fn bind_create_view(
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

pub(super) fn bind_drop_view(
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

pub(super) fn bind_create_index(
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

    if !matches!(
        statement.kind,
        crate::catalog::IndexKind::Scalar | crate::catalog::IndexKind::Column
    ) && fields.len() + expressions.len() > 1
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
        schema_index_options::bind_vector_index_options(
            &mut statement,
            catalog,
            &schema,
            &table,
            &name,
            fields.as_slice(),
        )?;
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
                    | "tokenizer"
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
        statement
            .options
            .insert("tokenizer".to_string(), analyzer.tokenizer);
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

    if statement.kind == crate::catalog::IndexKind::Column {
        if fields.is_empty() {
            return Err(CassieError::Planner(
                "column index requires at least one field".into(),
            ));
        }
        if !expressions.is_empty() {
            return Err(CassieError::Planner(
                "column indexes do not support expressions".into(),
            ));
        }
        if statement.unique {
            return Err(CassieError::Planner(
                "column indexes cannot be unique".into(),
            ));
        }
        if !include_fields.is_empty() {
            return Err(CassieError::Planner(
                "column indexes do not support INCLUDE columns".into(),
            ));
        }
        if statement.predicate.is_some() {
            return Err(CassieError::Planner(
                "partial column indexes are not supported".into(),
            ));
        }

        let mut seen_fields = std::collections::BTreeSet::new();
        for field in &fields {
            if !seen_fields.insert(field.to_ascii_lowercase()) {
                return Err(CassieError::Planner(format!(
                    "column index field '{field}' is duplicated"
                )));
            }
        }

        let segment_size = parse_column_index_segment_size(statement.options.get("segment_size"))?;
        for key in statement.options.keys() {
            if key != "segment_size" {
                return Err(CassieError::Planner(format!(
                    "unsupported column index option '{key}' for '{name}' on collection '{table}'"
                )));
            }
        }
        statement
            .options
            .insert("segment_size".to_string(), segment_size.to_string());
    }

    if statement.kind == crate::catalog::IndexKind::TimeSeries {
        schema_index_options::bind_time_series_index_options(
            &mut statement,
            &schema,
            &table,
            &name,
            fields.as_slice(),
            expressions.as_slice(),
            include_fields.as_slice(),
        )?;
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

pub(super) fn validate_index_expression(
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

pub(super) fn parse_column_index_segment_size(
    value: Option<&String>,
) -> Result<usize, CassieError> {
    let value = value.map(String::as_str).unwrap_or("").trim();
    if value.is_empty() {
        return Ok(1024);
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| CassieError::Planner("invalid column index option 'segment_size'".into()))?;
    if !(1..=1_000_000).contains(&parsed) {
        return Err(CassieError::Planner(
            "column index option 'segment_size' must be in [1, 1000000]".into(),
        ));
    }
    Ok(parsed)
}

pub(super) fn parse_fulltext_index_float_option(
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

pub(super) fn bind_drop_index(
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

pub(super) fn bind_drop_schema(
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

pub(super) fn bind_alter_schema(
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

pub(super) fn bind_drop_table(
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

pub(super) fn bind_alter_table(
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

pub(super) fn validate_alter_schema(
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
