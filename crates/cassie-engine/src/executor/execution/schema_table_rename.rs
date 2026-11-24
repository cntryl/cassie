//! `ALTER TABLE ... RENAME COLUMN`, `RENAME TO` and `SET SCHEMA`.

use super::{refresh_table_cardinality_stats, schema_foreign_keys, schema_sequence_rename};
use crate::app::{Cassie, CassieSession};
use crate::executor::execution::QueryError;

pub(super) fn alter_table_rename_column(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), QueryError> {
    let rewritten_views = prepare_column_view_renames(cassie, table, from, to)?;
    let prepublication = cassie.catalog.detached_transaction_snapshot();
    prepublication.rename_collection_field(table, from, to);
    for view in &rewritten_views {
        prepublication.register_view(view.clone());
        validate_rewritten_view_in_catalog(cassie, &prepublication, view, "column")?;
    }
    cassie
        .midge
        .alter_collection_rename_column_with_views(table, from, to, &rewritten_views)
        .map_err(QueryError::Cassie)?;
    cassie.catalog.rename_collection_field(table, from, to);
    for view in rewritten_views {
        cassie.catalog.register_view(view);
    }
    schema_foreign_keys::rename_referenced_field(cassie, table, from, to)?;
    crate::executor::execution::property_graph_dependents::rename_column(cassie, table, from, to)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    crate::executor::execution::materialized_projection::mark_source_projections_stale(
        cassie, table,
    )?;
    refresh_table_cardinality_stats(cassie, table)
}

pub(super) fn validate_rewritten_view(
    cassie: &Cassie,
    view: &crate::catalog::ViewMeta,
    kind: &str,
) -> Result<(), QueryError> {
    validate_rewritten_view_in_catalog(cassie, &cassie.catalog, view, kind)
}

pub(super) fn validate_rewritten_view_in_catalog(
    cassie: &Cassie,
    catalog: &crate::catalog::Catalog,
    view: &crate::catalog::ViewMeta,
    kind: &str,
) -> Result<(), QueryError> {
    let context = view_binding_context(cassie, catalog, view);
    let parsed = super::super::domain_view_dependents::parse_view(cassie, &view.query, None)
        .ok_or_else(|| unsafe_dependent_view_error(&view.name, kind))?;
    let bound = crate::sql::binder::bind_with_context(parsed, catalog, &context)
        .map_err(|_| unsafe_dependent_view_error(&view.name, kind))?;
    let crate::sql::ast::QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(unsafe_dependent_view_error(&view.name, kind));
    };
    let schema = crate::sql::binder::infer_select_schema_with_context(select, catalog, &context)
        .map_err(|_| unsafe_dependent_view_error(&view.name, kind))?;
    if schema.fields.len() != view.schema.fields.len()
        || schema
            .fields
            .iter()
            .zip(&view.schema.fields)
            .any(|(actual, stored)| actual.data_type != stored.data_type)
    {
        return Err(unsafe_dependent_view_error(&view.name, kind));
    }
    Ok(())
}

fn view_binding_context(
    cassie: &Cassie,
    catalog: &crate::catalog::Catalog,
    view: &crate::catalog::ViewMeta,
) -> crate::sql::binder::BindingContext {
    let raw_catalog_names =
        super::super::domain_view_dependents::parse_view(cassie, &view.query, None).is_some_and(
            |parsed| {
                let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
                    return false;
                };
                crate::sql::binder::select_referenced_relations(&select)
                    .iter()
                    .any(|name| catalog.get_collection(name).is_some())
            },
        );
    let search_schemas = view
        .function_dependency_search_path
        .iter()
        .map(|schema| schema.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let legacy_function_names = catalog.list_functions().iter().any(|function| {
        matches!(
            crate::catalog::parse_name(&function.name),
            Ok(crate::catalog::ParsedName::SchemaQualified { schema, .. })
                if search_schemas.contains(&schema.to_ascii_lowercase())
        )
    });
    let legacy_graph_names = catalog.list_property_graphs().iter().any(|graph| {
        matches!(
            crate::catalog::parse_name(&graph.name),
            Ok(crate::catalog::ParsedName::SchemaQualified { .. }
                | crate::catalog::ParsedName::Unqualified(_))
        )
    });
    let retained_function_dependencies = catalog
        .list_functions()
        .iter()
        .any(|function| view.function_dependency_ids.contains(&function.object_id));
    if catalog.list_namespaces().is_empty()
        || raw_catalog_names
        || legacy_function_names
        || legacy_graph_names
        || retained_function_dependencies
    {
        let database = if view.function_dependency_database.is_empty() {
            cassie.default_database.clone()
        } else {
            view.function_dependency_database.clone()
        };
        let search_path = if view.function_dependency_search_path.is_empty() {
            vec![crate::catalog::DEFAULT_SCHEMA.to_string()]
        } else {
            view.function_dependency_search_path.clone()
        };
        crate::sql::binder::BindingContext::unscoped(database, search_path)
    } else if view.function_dependency_database.is_empty() {
        cassie.binding_context_for_session(None)
    } else {
        crate::sql::binder::BindingContext::scoped(
            view.function_dependency_database.clone(),
            view.function_dependency_search_path.clone(),
        )
    }
}

fn prepare_column_view_renames(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<Vec<crate::catalog::ViewMeta>, QueryError> {
    let target = cassie
        .catalog
        .get_collection(table)
        .map(|collection| collection.name)
        .ok_or_else(|| QueryError::General(format!("table '{table}' does not exist")))?;
    let dependents =
        crate::executor::execution::view_dependents::views_using_column(cassie, table, from);
    let mut rewritten = Vec::with_capacity(dependents.len());
    for mut view in dependents {
        let Some(select) =
            crate::executor::execution::view_dependents::parsed_select(cassie, &view)
        else {
            return Err(unsafe_dependent_view_error(&view.name, "column"));
        };
        let mut qualifiers = std::collections::BTreeSet::new();
        collect_target_qualifiers(cassie, &select.source, &target, &mut qualifiers);
        let references = crate::sql::binder::select_referenced_relations(&select);
        let unqualified_safe = references.iter().all(|written| {
            let Some(collection) = cassie.catalog.get_collection(written) else {
                if let Some(candidate) = cassie.catalog.get_view(written) {
                    return !candidate.schema.fields.iter().any(|field| {
                        crate::sql::ColumnIdentifierPath::matches_stored_field(from, &field.name)
                    });
                }
                return true;
            };
            if collection.name == target {
                return true;
            }
            let fields: Vec<String> = cassie
                .catalog
                .get_view(written)
                .map(|candidate| {
                    candidate
                        .schema
                        .fields
                        .into_iter()
                        .map(|field| field.name)
                        .collect()
                })
                .or_else(|| {
                    cassie
                        .midge
                        .row_schema(&collection.name)
                        .ok()
                        .map(|schema| schema.fields.into_iter().map(|field| field.name).collect())
                })
                .unwrap_or_default();
            !fields
                .iter()
                .any(|field| crate::sql::ColumnIdentifierPath::matches_stored_field(from, field))
        });
        let query = if select.projection.len() == 1
            && matches!(
                select.projection.first(),
                Some(crate::sql::ast::SelectItem::Wildcard)
            )
            && references.iter().all(|written| {
                cassie
                    .catalog
                    .get_collection(written)
                    .is_none_or(|collection| collection.name == target)
            }) {
            expand_renamed_column_wildcard(&view, from, to)?
        } else {
            rewrite_column_tokens(&view.query, from, to, &qualifiers, unqualified_safe)
                .ok_or_else(|| unsafe_dependent_view_error(&view.name, "column"))?
        };
        if super::super::domain_view_dependents::parse_view(cassie, &query, None).is_none() {
            return Err(unsafe_dependent_view_error(&view.name, "column"));
        }
        view.query = query;
        rewritten.push(view);
    }
    Ok(rewritten)
}

fn collect_target_qualifiers(
    cassie: &Cassie,
    source: &crate::sql::ast::QuerySource,
    target: &str,
    qualifiers: &mut std::collections::BTreeSet<String>,
) -> bool {
    use crate::sql::ast::QuerySource;

    match source {
        QuerySource::Collection(path) => {
            let written = path.to_string();
            if cassie
                .catalog
                .get_collection(&written)
                .is_some_and(|collection| collection.name == target)
            {
                qualifiers.insert(crate::catalog::local_name(&written).to_ascii_lowercase());
                qualifiers.insert(written.to_ascii_lowercase());
                true
            } else {
                false
            }
        }
        QuerySource::Aliased { source, alias, .. } => {
            let mut inner_qualifiers = std::collections::BTreeSet::new();
            let target_found =
                collect_target_qualifiers(cassie, source, target, &mut inner_qualifiers);
            if target_found {
                qualifiers.insert(alias.to_ascii_lowercase());
            } else {
                qualifiers.extend(inner_qualifiers);
            }
            target_found
        }
        QuerySource::Join { left, right, .. } => {
            let left_found = collect_target_qualifiers(cassie, left, target, qualifiers);
            let right_found = collect_target_qualifiers(cassie, right, target, qualifiers);
            left_found || right_found
        }
        QuerySource::TableSample { source, .. } => {
            collect_target_qualifiers(cassie, source, target, qualifiers)
        }
        _ => false,
    }
}

fn expand_renamed_column_wildcard(
    view: &crate::catalog::ViewMeta,
    from: &str,
    to: &str,
) -> Result<String, QueryError> {
    use crate::executor::execution::sql_text_tokens::{tokenize, Kind};

    let tokens = tokenize(&view.query);
    let Some(star) = tokens
        .iter()
        .find(|token| token.kind == Kind::Punct && token.is_punct(&view.query, b'*'))
    else {
        return Err(unsafe_dependent_view_error(&view.name, "column"));
    };
    let projection = view
        .schema
        .fields
        .iter()
        .map(|field| {
            if crate::sql::ColumnIdentifierPath::matches_stored_field(from, &field.name) {
                crate::catalog::object_identity::quote_identifier(to)
            } else {
                crate::catalog::object_identity::quote_identifier(&field.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut rewritten = String::with_capacity(view.query.len() + projection.len());
    rewritten.push_str(&view.query[..star.start]);
    rewritten.push_str(&projection);
    rewritten.push_str(&view.query[star.end..]);
    Ok(rewritten)
}

fn rewrite_column_tokens(
    sql: &str,
    from: &str,
    to: &str,
    qualifiers: &std::collections::BTreeSet<String>,
    unqualified_safe: bool,
) -> Option<String> {
    use crate::executor::execution::sql_text_tokens::{tokenize, Kind};

    let tokens = tokenize(sql);
    let mut replacements = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, Kind::Word | Kind::Quoted)
            || !crate::sql::ColumnIdentifierPath::matches_stored_field(token.text(sql), from)
        {
            continue;
        }
        if tokens
            .get(index.wrapping_sub(1))
            .is_some_and(|previous| previous.is_keyword(sql, "as") || previous.is_punct(sql, b':'))
            || tokens
                .get(index + 1)
                .is_some_and(|next| next.is_punct(sql, b'(') || next.is_punct(sql, b'.'))
        {
            continue;
        }
        let qualified = index >= 2 && tokens[index - 1].is_punct(sql, b'.');
        let belongs_to_target = if qualified {
            tokens[index - 2]
                .identifier(sql)
                .is_some_and(|qualifier| qualifiers.contains(&qualifier.to_ascii_lowercase()))
        } else {
            unqualified_safe
                && !index.checked_sub(1).is_some_and(|previous| {
                    tokens[previous].is_keyword(sql, "from")
                        || tokens[previous].is_keyword(sql, "join")
                })
        };
        if !belongs_to_target {
            continue;
        }
        let replacement = if token.text(sql).starts_with('"') {
            crate::catalog::object_identity::quote_identifier(to)
        } else {
            to.to_owned()
        };
        replacements.push((token.start, token.end, replacement));
    }
    if replacements.is_empty() {
        return None;
    }
    let mut rewritten = String::with_capacity(sql.len());
    let mut copied = 0;
    for (start, end, replacement) in replacements {
        rewritten.push_str(&sql[copied..start]);
        rewritten.push_str(&replacement);
        copied = end;
    }
    rewritten.push_str(&sql[copied..]);
    Some(rewritten)
}

pub(super) fn alter_table_rename_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    let rewritten_views = prepare_table_view_renames(cassie, table, next_table)?;
    cassie.catalog.reject_relation_move_target(next_table)?;
    if cassie.catalog.exists(next_table) {
        return Err(QueryError::Cassie(
            crate::app::DiagnosticError::new(
                "42P07",
                format!(
                    "relation \"{}\" already exists",
                    crate::catalog::local_name(next_table)
                ),
            )
            .into(),
        ));
    }
    let prepublication = cassie.catalog.detached_transaction_snapshot();
    prepublication
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    for view in &rewritten_views {
        prepublication.unregister_view(&view.name);
        prepublication.register_view(view.clone());
        validate_rewritten_view_in_catalog(cassie, &prepublication, view, "table")?;
    }
    cassie
        .midge
        .rename_collection(table, next_table)
        .map_err(QueryError::Cassie)?;
    for view in &rewritten_views {
        cassie
            .midge
            .put_view(view)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    cassie
        .catalog
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    for view in rewritten_views {
        cassie.catalog.register_view(view);
    }
    schema_sequence_rename::rename_owned_table_sequences(cassie, next_table)?;
    crate::executor::execution::property_graph_dependents::rename_table(cassie, table, next_table)?;
    schema_foreign_keys::rename_referenced_table(cassie, table, next_table)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)
}

fn prepare_table_view_renames(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<Vec<crate::catalog::ViewMeta>, QueryError> {
    let target = cassie
        .catalog
        .get_collection(table)
        .map(|collection| collection.name)
        .ok_or_else(|| QueryError::General(format!("table '{table}' does not exist")))?;
    let mut rewritten = Vec::new();
    for mut view in cassie.catalog.list_views() {
        let Some(select) =
            crate::executor::execution::view_dependents::parsed_select(cassie, &view)
        else {
            if possible_table_reference(&view.query, &crate::catalog::local_name(&target)) {
                return Err(unsafe_dependent_view_error(&view.name, "table"));
            }
            continue;
        };
        let relation_references = crate::sql::binder::select_referenced_relations(&select);
        let context = view_binding_context(cassie, &cassie.catalog, &view);
        let resolved_target =
            crate::sql::binder::resolve_relation_name(&target, &cassie.catalog, &context)
                .unwrap_or_else(|_| target.clone());
        let references_target = relation_references.iter().any(|written| {
            written == &target
                || crate::sql::binder::resolve_relation_name(written, &cassie.catalog, &context)
                    .is_ok_and(|resolved| resolved == resolved_target)
        });
        if !references_target {
            continue;
        }
        let Some(query) = rewrite_relation_references(
            &view.query,
            &relation_references,
            &resolved_target,
            next_table,
            cassie,
            &context,
        ) else {
            return Err(unsafe_dependent_view_error(&view.name, "table"));
        };
        if crate::executor::execution::domain_view_dependents::parse_view(cassie, &query, None)
            .is_none()
        {
            return Err(unsafe_dependent_view_error(&view.name, "table"));
        }
        view.query = query;
        rewritten.push(view);
    }
    Ok(rewritten)
}

fn possible_table_reference(sql: &str, table: &str) -> bool {
    let tokens = crate::executor::execution::sql_text_tokens::tokenize(sql);
    crate::executor::execution::sql_text_tokens::chains(sql, &tokens)
        .into_iter()
        .any(|chain| {
            let name_matches = chain.parts.iter().any(|part| {
                let token = tokens[*part];
                token.identifier(sql).is_some_and(|identifier| {
                    if token.text(sql).starts_with('"') {
                        identifier == table
                    } else {
                        identifier.eq_ignore_ascii_case(table)
                    }
                })
            });
            name_matches
                && (chain.parts.len() > 1
                    || chain.parts.first().is_some_and(|first| {
                        first.checked_sub(1).is_some_and(|previous| {
                            tokens[previous].is_keyword(sql, "from")
                                || tokens[previous].is_keyword(sql, "join")
                        })
                    }))
        })
}

fn rewrite_relation_references(
    sql: &str,
    references: &[String],
    target: &str,
    next_relation: &str,
    cassie: &Cassie,
    context: &crate::sql::binder::BindingContext,
) -> Option<String> {
    use crate::executor::execution::sql_text_tokens::{chains, tokenize};

    let names = RelationRewriteNames::new(target, next_relation);
    let target_references =
        relation_reference_tokens(references, target, &names.target_local, cassie, context);
    let tokens = tokenize(sql);
    let mut replacements = Vec::new();
    for chain in chains(sql, &tokens) {
        for relation in &target_references {
            if chain.parts.len() < relation.len()
                || !chain.parts.iter().take(relation.len()).zip(relation).all(
                    |(index, expected)| {
                        tokens[*index]
                            .identifier(sql)
                            .is_some_and(|found| found == *expected)
                    },
                )
            {
                continue;
            }
            let relation_end = chain.parts[relation.len() - 1];
            let is_qualified_column = chain.parts.len() > relation.len()
                && tokens[chain.parts[relation.len() - 1] + 1].is_punct(sql, b'.')
                && !tokens
                    .get(chain.parts[relation.len()] + 1)
                    .is_some_and(|token| token.is_punct(sql, b'('));
            let is_source = chain
                .parts
                .first()
                .and_then(|first| first.checked_sub(1))
                .is_some_and(|previous| {
                    tokens[previous].is_keyword(sql, "from")
                        || tokens[previous].is_keyword(sql, "join")
                });
            if !is_qualified_column && !is_source {
                continue;
            }
            let token = tokens[relation_end];
            let replacement = if relation.len() == 1 && names.schema_changed {
                let next_schema = names
                    .next_schema
                    .as_deref()
                    .expect("schema change identity");
                let schema = if chain
                    .parts
                    .first()
                    .is_some_and(|&index| tokens[index].text(sql).starts_with('"'))
                {
                    crate::catalog::object_identity::quote_identifier(next_schema)
                } else {
                    next_schema.to_owned()
                };
                let local = if token.text(sql).starts_with('"') {
                    crate::catalog::object_identity::quote_identifier(&names.next_local)
                } else {
                    names.next_local.clone()
                };
                format!("{schema}.{local}")
            } else if token.text(sql).starts_with('"') {
                crate::catalog::object_identity::quote_identifier(&names.next_local)
            } else {
                names.next_local.clone()
            };
            replacements.push((token.start, token.end, replacement));
            if relation.len() >= 2 && names.schema_changed {
                let next_schema = names
                    .next_schema
                    .as_deref()
                    .expect("schema change identity");
                let schema_index = relation.len() - 2;
                let schema_token = tokens[chain.parts[schema_index]];
                let schema_replacement = if schema_token.text(sql).starts_with('"') {
                    crate::catalog::object_identity::quote_identifier(next_schema)
                } else {
                    next_schema.to_owned()
                };
                replacements.push((schema_token.start, schema_token.end, schema_replacement));
            }
            break;
        }
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    replacements.dedup_by_key(|(start, _, _)| *start);
    apply_relation_replacements(sql, replacements)
}

fn relation_reference_tokens(
    references: &[String],
    target: &str,
    target_local: &str,
    cassie: &Cassie,
    context: &crate::sql::binder::BindingContext,
) -> Vec<Vec<String>> {
    use crate::executor::execution::sql_text_tokens::tokenize;

    let mut found = references
        .iter()
        .filter(|written| {
            *written == target
                || crate::sql::binder::resolve_relation_name(written, &cassie.catalog, context)
                    .is_ok_and(|resolved| resolved == target)
        })
        .map(|written| {
            tokenize(written)
                .iter()
                .filter_map(|token| token.identifier(written))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if !found.iter().any(|reference| reference.len() == 1) {
        found.push(vec![target_local.to_string()]);
    }
    found
}

fn apply_relation_replacements(
    sql: &str,
    replacements: Vec<(usize, usize, String)>,
) -> Option<String> {
    if replacements.is_empty() {
        return None;
    }
    let mut result = String::with_capacity(sql.len());
    let mut copied = 0;
    for (start, end, replacement) in replacements {
        result.push_str(&sql[copied..start]);
        result.push_str(&replacement);
        copied = end;
    }
    result.push_str(&sql[copied..]);
    Some(result)
}

struct RelationRewriteNames {
    target_local: String,
    next_schema: Option<String>,
    next_local: String,
    schema_changed: bool,
}

impl RelationRewriteNames {
    fn new(target: &str, next_relation: &str) -> Self {
        let target_id = crate::catalog::RelationId::parse_canonical(target);
        let next_id = crate::catalog::RelationId::parse_canonical(next_relation);
        let target_schema =
            relation_schema(target, target_id.as_ref().map(|id| id.schema.as_str()));
        let next_schema =
            relation_schema(next_relation, next_id.as_ref().map(|id| id.schema.as_str()));
        Self {
            target_local: target_id
                .as_ref()
                .map_or_else(|| crate::catalog::local_name(target), |id| id.name.clone()),
            next_schema: next_schema.clone(),
            next_local: next_id.as_ref().map_or_else(
                || crate::catalog::local_name(next_relation),
                |id| id.name.clone(),
            ),
            schema_changed: target_schema != next_schema,
        }
    }
}

fn relation_schema(relation: &str, canonical: Option<&str>) -> Option<String> {
    canonical.map(str::to_string).or_else(|| {
        crate::sql::ast::IdentifierPath::parse(relation)
            .ok()
            .and_then(|path| {
                let components = path.components();
                components
                    .len()
                    .checked_sub(2)
                    .and_then(|index| components.get(index))
                    .map(|component| component.value().to_string())
            })
    })
}

fn unsafe_dependent_view_error(view: &str, kind: &str) -> QueryError {
    QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "2BP01",
            format!("cannot rename {kind} because a dependent view cannot be safely rewritten"),
        )
        .with_detail(format!("dependent object: view {view}"))
        .into(),
    )
}

pub(super) fn alter_table_set_schema(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    target_schema: &str,
) -> Result<(), QueryError> {
    let relation = crate::catalog::RelationId::parse_canonical(table).unwrap_or_else(|| {
        let path =
            crate::sql::ast::IdentifierPath::parse(table).expect("binder admitted relation path");
        match path.parsed_name().expect("binder admitted relation name") {
            crate::catalog::ParsedName::Unqualified(name) => crate::catalog::RelationId::new(
                cassie.default_database.clone(),
                crate::catalog::DEFAULT_SCHEMA.to_string(),
                name,
            ),
            crate::catalog::ParsedName::SchemaQualified { schema, name } => {
                crate::catalog::RelationId::new(cassie.default_database.clone(), schema, name)
            }
            crate::catalog::ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => crate::catalog::RelationId::new(database, schema, name),
        }
    });
    let schema = crate::catalog::SchemaId::parse_canonical(target_schema).unwrap_or_else(|| {
        crate::catalog::SchemaId::new(cassie.default_database.clone(), target_schema)
    });
    let next_table =
        crate::catalog::RelationId::new(relation.database.clone(), schema.schema, relation.name)
            .canonical_name();
    if table == next_table {
        return Ok(());
    }
    cassie.catalog.reject_relation_move_target(&next_table)?;
    if cassie.catalog.exists(&next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    let sequence_move =
        schema_sequence_rename::prepare_owned_table_sequence_move(cassie, table, &next_table)?;
    let rewritten_views = prepare_table_view_renames(cassie, table, &next_table)?;
    let prepublication = cassie.catalog.detached_transaction_snapshot();
    prepublication
        .rename_collection(table, &next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    for view in &rewritten_views {
        prepublication.unregister_view(&view.name);
        prepublication.register_view(view.clone());
        validate_rewritten_view_in_catalog(cassie, &prepublication, view, "schema")?;
    }

    cassie
        .midge
        .rename_collection(table, &next_table)
        .map_err(QueryError::Cassie)?;
    for view in &rewritten_views {
        cassie
            .midge
            .put_view(view)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    cassie
        .catalog
        .rename_collection(table, &next_table)
        .map_err(QueryError::Cassie)?;
    for view in rewritten_views {
        cassie.catalog.register_view(view);
    }
    if let Some(session) = session {
        session
            .rename_transaction_collections(&[(table.to_owned(), next_table.clone())])
            .map_err(QueryError::Cassie)?;
    }
    schema_sequence_rename::move_owned_table_sequences(cassie, sequence_move)?;
    crate::executor::execution::property_graph_dependents::rename_table(
        cassie,
        table,
        &next_table,
    )?;
    schema_foreign_keys::rename_referenced_table(cassie, table, &next_table)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    crate::executor::execution::materialized_projection::mark_source_projections_stale(
        cassie, table,
    )?;
    Ok(())
}
