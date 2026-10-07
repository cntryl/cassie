use super::{
    inference, Analyzer, CassieError, CommonTableExpression, CteQuery, CteScope, DataType, Expr,
    OutputField, OutputFields, QuerySource, QueryStatement, SelectItem, SelectStatement,
    SetOperator,
};
use crate::types::row_identity::{
    is_legacy_id_column, is_row_identity_column, LEGACY_ID_COLUMN, ROW_IDENTITY_COLUMN,
};

pub(super) struct SourceScope {
    pub(super) output: OutputFields,
    pub(super) lookup: OutputFields,
}

impl Analyzer<'_> {
    pub(super) fn select(
        &self,
        select: &SelectStatement,
        outer_ctes: &CteScope,
        outer_fields: Option<&[OutputField]>,
    ) -> Result<OutputFields, CassieError> {
        self.check_controls()?;
        let mut ctes = outer_ctes.clone();
        for cte in &select.ctes {
            let fields = self.cte(cte, &ctes)?;
            ctes.insert(cte.name.to_ascii_lowercase(), fields);
        }
        let mut source = self.source(&select.source, &ctes, outer_fields)?;
        if let Some(outer) = outer_fields {
            // No outer fields enter wildcard output; local unqualified fields
            // precede the explicitly qualified outer lookup.
            source.lookup.extend_from_slice(outer);
        }
        let mut output = self.project(&select.projection, &source.output, &source.lookup)?;
        if let Some(set) = &select.set {
            let right = self.select(&set.right, &ctes, outer_fields)?;
            require_width(&output, &right)?;
            match set.operator {
                SetOperator::Union | SetOperator::UnionAll => join_fields(&mut output, &right),
                // These operators return existing left rows, not RHS values.
                // Source: execution/source_rows.rs301/318. RHS is still
                // analyzed for metadata shape, but its origins do not export.
                SetOperator::Intersect | SetOperator::Except => {}
            }
        }
        Ok(output)
    }

    fn cte(
        &self,
        cte: &CommonTableExpression,
        outer_ctes: &CteScope,
    ) -> Result<OutputFields, CassieError> {
        let base = match &cte.query {
            CteQuery::Simple(statement) => statement,
            CteQuery::Recursive { base, .. } => base,
        };
        let QueryStatement::Select(base) = &base.statement else {
            return Err(CassieError::Planner(
                "CTE body must be a SELECT statement".to_string(),
            ));
        };
        let mut fields = self.select(base, outer_ctes, None)?;
        rename_cte_fields(&mut fields, cte)?;
        if let CteQuery::Recursive { recursive, .. } = &cte.query {
            let QueryStatement::Select(recursive) = &recursive.statement else {
                return Err(CassieError::Planner(
                    "recursive CTE term must be a SELECT statement".to_string(),
                ));
            };
            // Monotone finite lattice: each candidate type comes from the
            // finite bound AST/catalog or the two adapter carrier families;
            // origin has only false->true. This is schema analysis, not row
            // recursion, and does not inherit a data-dependent depth limit.
            loop {
                self.check_controls()?;
                let previous = fields.clone();
                let mut scope = outer_ctes.clone();
                scope.insert(cte.name.to_ascii_lowercase(), fields.clone());
                let mut next = self.select(recursive, &scope, None)?;
                rename_cte_fields(&mut next, cte)?;
                require_width(&fields, &next)?;
                join_fields(&mut fields, &next);
                if fields == previous {
                    break;
                }
            }
        }
        Ok(fields)
    }

    pub(super) fn source(
        &self,
        source: &QuerySource,
        ctes: &CteScope,
        outer_fields: Option<&[OutputField]>,
    ) -> Result<SourceScope, CassieError> {
        self.check_controls()?;
        match source {
            QuerySource::Aliased { source, alias, .. } => {
                let inner = self.source(source, ctes, outer_fields)?;
                let qualifier = super::super::aliases::qualifier(alias);
                let mut result = exported_scope(inner.output, std::slice::from_ref(&qualifier));
                let lookup = inner
                    .lookup
                    .into_iter()
                    .filter(|field| {
                        crate::sql::ColumnIdentifierPath::parse(&field.name)
                            .is_ok_and(|path| !path.is_qualified())
                    })
                    .collect();
                result.lookup = exported_scope(lookup, &[qualifier]).lookup;
                Ok(result)
            }
            QuerySource::SingleRow => Ok(SourceScope {
                output: Vec::new(),
                lookup: Vec::new(),
            }),
            QuerySource::Collection(name) => self.collection_source(name, ctes),
            QuerySource::Cte(name) => {
                let fields = ctes
                    .get(&name.to_ascii_lowercase())
                    .cloned()
                    .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
                Ok(exported_scope(fields, std::slice::from_ref(name)))
            }
            QuerySource::TableFunction { name, .. } => {
                let fields = super::super::select::table_function_columns(name)
                    .into_iter()
                    .map(|(name, data_type)| OutputField {
                        name,
                        value: super::OutputType::fixed(data_type),
                        nullable: true,
                        wildcard_identity: false,
                    })
                    .collect();
                Ok(exported_scope(fields, std::slice::from_ref(name)))
            }
            QuerySource::Subquery {
                select,
                alias,
                lateral,
            } => {
                let fields =
                    self.select(select, ctes, if *lateral { outer_fields } else { None })?;
                Ok(exported_scope(fields, std::slice::from_ref(alias)))
            }
            QuerySource::Join { left, right, .. } => {
                let mut left = self.source(left, ctes, outer_fields)?;
                let mut lateral = left.lookup.clone();
                if let Some(outer) = outer_fields {
                    lateral.extend_from_slice(outer);
                }
                let right = self.source(right, ctes, Some(&lateral))?;
                left.output.extend(right.output);
                left.lookup.extend(right.lookup);
                Ok(left)
            }
        }
    }

    fn collection_source(&self, name: &str, ctes: &CteScope) -> Result<SourceScope, CassieError> {
        // Mirrors wildcard::source_fields, including a bound CTE
        // whose source is still represented as a collection.
        if let Some(fields) = ctes.get(&name.to_ascii_lowercase()) {
            return Ok(exported_scope(fields.clone(), &[name.to_string()]));
        }
        let schema = inference::relation_output_schema(self.catalog, name)?;
        let mut fields = schema
            .fields
            .into_iter()
            .map(OutputField::from_schema)
            .collect::<Vec<_>>();
        let base_schema = self.catalog.get_schema(name).filter(|_| {
            crate::catalog::virtual_views::schema(name).is_none()
                && self.catalog.get_view(name).is_none()
                && self.catalog.get_materialized_projection(name).is_none()
        });
        if base_schema
            .as_ref()
            .is_some_and(|schema| !schema.declares_id())
        {
            if let Some(first) = fields.first_mut() {
                first.name = ROW_IDENTITY_COLUMN.to_string();
            }
        }
        let qualifiers = crate::catalog::qualifier_variants(name);
        let mut scope = exported_scope(fields, &qualifiers);
        if let Some(schema) = base_schema {
            // Physical _id always exists for a base table. Only a base
            // without declared id contributes the legacy id lookup.
            // These are lookup-only facts, never extra output fields.
            let identity = OutputField {
                name: if schema.declares_id() {
                    ROW_IDENTITY_COLUMN
                } else {
                    LEGACY_ID_COLUMN
                }
                .to_string(),
                value: super::OutputType::fixed(DataType::Text),
                nullable: true,
                wildcard_identity: false,
            };
            scope
                .lookup
                .extend(exported_scope(vec![identity], &qualifiers).lookup);
        }
        Ok(scope)
    }

    pub(super) fn project(
        &self,
        projection: &[SelectItem],
        source: &[OutputField],
        lookup: &[OutputField],
    ) -> Result<OutputFields, CassieError> {
        let mut output = Vec::new();
        for item in projection {
            self.check_controls()?;
            if matches!(item, SelectItem::Wildcard) {
                let has_id = source.iter().any(|field| is_legacy_id_column(&field.name));
                output.extend(
                    source
                        .iter()
                        .filter(|field| !has_id || !is_row_identity_column(&field.name))
                        .cloned()
                        .map(|mut field| {
                            field.wildcard_identity = is_row_identity_column(&field.name);
                            field
                        }),
                );
                continue;
            }
            let (name, value, nullable) = match item {
                SelectItem::Column { name, alias } => {
                    let output_name = alias.clone().unwrap_or_else(|| {
                        crate::sql::ColumnIdentifierPath::parse(name)
                            .map_or_else(|_| name.clone(), |column| column.display_name())
                    });
                    (
                        output_name,
                        self.expression(&Expr::Column(name.clone()), lookup)?,
                        true,
                    )
                }
                SelectItem::Expr { expr, alias } => (
                    alias.as_deref().unwrap_or("expr").to_string(),
                    self.expression(expr, lookup)?,
                    true,
                ),
                SelectItem::Function { function, alias } => (
                    alias.as_deref().unwrap_or(&function.name).to_string(),
                    self.function(function, lookup)?,
                    true,
                ),
                SelectItem::WindowFunction { function, alias } => {
                    let value = match function.name.to_ascii_lowercase().as_str() {
                        "lag" | "lead" | "first_value" | "last_value" => function
                            .args
                            .first()
                            .map(|argument| self.expression(argument, lookup))
                            .transpose()?
                            .unwrap_or_else(|| super::OutputType::fixed(DataType::Text)),
                        _ => super::OutputType::fixed(DataType::BigInt),
                    };
                    // Matches current inference, including lag/lead's current
                    // nullable descriptor policy; no new nullable ABI here.
                    (
                        alias.as_deref().unwrap_or(&function.name).to_string(),
                        value,
                        false,
                    )
                }
                SelectItem::Wildcard => unreachable!(),
            };
            output.push(OutputField {
                name,
                value,
                nullable,
                wildcard_identity: false,
            });
        }
        Ok(output)
    }
}

fn exported_scope(output: OutputFields, qualifiers: &[String]) -> SourceScope {
    let mut lookup = Vec::new();
    for field in &output {
        // CTEs and derived tables export their actual projected names. A
        // projected _id must not manufacture a competing id lookup.
        let mut plain = field.clone();
        let key = crate::sql::ColumnIdentifierPath::from_field_name(&plain.name).lookup_key();
        plain.name.clone_from(&key);
        plain.wildcard_identity = false;
        lookup.push(plain.clone());
        for qualifier in qualifiers {
            let mut qualified = plain.clone();
            qualified.name = format!("{}.{}", qualifier.to_ascii_lowercase(), key);
            lookup.push(qualified);
        }
    }
    SourceScope { output, lookup }
}

fn rename_cte_fields(
    fields: &mut [OutputField],
    cte: &CommonTableExpression,
) -> Result<(), CassieError> {
    let placeholder = cte.aliases.len() == 1 && cte.aliases[0] == "*";
    if !cte.aliases.is_empty() && !placeholder {
        let recursive = matches!(cte.query, CteQuery::Recursive { .. });
        if cte.aliases.len() > fields.len() || (recursive && cte.aliases.len() != fields.len()) {
            return Err(CassieError::Planner(
                "CTE alias count does not match output columns".to_string(),
            ));
        }
        for (field, alias) in fields.iter_mut().zip(&cte.aliases) {
            field.name.clone_from(alias);
            field.wildcard_identity = false;
        }
    }
    Ok(())
}

fn require_width(left: &[OutputField], right: &[OutputField]) -> Result<(), CassieError> {
    if left.len() != right.len() {
        return Err(CassieError::Planner(format!(
            "set operation column count mismatch: {} != {}",
            left.len(),
            right.len()
        )));
    }
    Ok(())
}

fn join_fields(left: &mut [OutputField], right: &[OutputField]) {
    for (left, right) in left.iter_mut().zip(right) {
        left.value.join(&right.value);
    }
}
