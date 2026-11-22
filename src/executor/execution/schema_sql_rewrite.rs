//! Renames a schema inside stored SQL text (views, materialized projections).
//!
//! Only genuine schema qualifiers are rewritten. The parsed statement says
//! which dotted names are relations; a qualifier is renamed when it starts a
//! referenced relation name (`a.t`, `db.a.t`), a function call (`a.f(`) or a
//! cast target (`::a.t`, `AS a.t`). Aliases, column qualifiers and names inside
//! string literals of any form, quoted identifiers or comments are untouched,
//! and bare names fold to lower case the way PostgreSQL reads them.

use std::collections::BTreeSet;

use super::domain_view_dependents::parse_view;
use super::sql_text_tokens::{chains, tokenize, Token};
use super::Cassie;
use crate::catalog::object_identity::quote_identifier;
use crate::sql::ast::QueryStatement;

/// The folded identifier parts of every relation the statement reads.
fn referenced_relations(cassie: &Cassie, raw: &str) -> Option<BTreeSet<Vec<String>>> {
    let parsed = parse_view(cassie, raw, None)?;
    let QueryStatement::Select(select) = parsed.statement else {
        return None;
    };
    Some(
        crate::sql::binder::select_referenced_relations(&select)
            .iter()
            .map(|written| {
                let tokens = tokenize(written);
                tokens
                    .iter()
                    .filter_map(|token| token.identifier(written))
                    .collect()
            })
            .collect(),
    )
}

/// Whether the chain at `parts` (token indexes) qualifies a relation, function
/// or type whose schema part is at `schema_part`.
fn qualifies_object(
    sql: &str,
    tokens: &[Token],
    parts: &[usize],
    schema_part: usize,
    relations: Option<&BTreeSet<Vec<String>>>,
    folded: &[String],
) -> bool {
    let name_part = schema_part + 1;
    let Some(&name_token) = parts.get(name_part) else {
        return false;
    };
    let Some(relations) = relations else {
        // No parsed form: any dotted chain led by the schema is a qualifier.
        return true;
    };
    let last_prefix = &folded[..=name_part];
    relations.contains(last_prefix)
        || tokens
            .get(name_token + 1)
            .is_some_and(|next| next.is_punct(sql, b'('))
        || is_cast_target(sql, tokens, parts[0])
}

/// Whether the token at `first` starts a cast target name.
fn is_cast_target(sql: &str, tokens: &[Token], first: usize) -> bool {
    first >= 1
        && (tokens[first - 1].is_keyword(sql, "as")
            || (first >= 2
                && tokens[first - 1].is_punct(sql, b':')
                && tokens[first - 2].is_punct(sql, b':')))
}

fn spelled_like(original: &Token, sql: &str, next_local: &str) -> String {
    let bare = next_local
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && next_local
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
    if bare && !original.text(sql).starts_with('"') {
        next_local.to_owned()
    } else {
        quote_identifier(next_local)
    }
}

/// `raw` with every schema qualifier of `current_local` renamed to `next_local`.
pub(super) fn rewrite_schema_qualified_sql(
    cassie: &Cassie,
    raw: &str,
    current_local: &str,
    next_local: &str,
) -> String {
    let relations = referenced_relations(cassie, raw);
    let tokens = tokenize(raw);
    let mut rewritten = String::with_capacity(raw.len());
    let mut copied = 0;
    for chain in chains(raw, &tokens) {
        let folded = chain
            .parts
            .iter()
            .filter_map(|&index| tokens[index].identifier(raw))
            .collect::<Vec<_>>();
        for schema_part in [0, 1] {
            let matches = folded.get(schema_part).is_some_and(|part| {
                if tokens[chain.parts[schema_part]].text(raw).starts_with('"') {
                    part == current_local
                } else {
                    part.eq_ignore_ascii_case(current_local)
                }
            });
            if !matches
                || !qualifies_object(
                    raw,
                    &tokens,
                    &chain.parts,
                    schema_part,
                    relations.as_ref(),
                    &folded,
                )
            {
                continue;
            }
            let token = tokens[chain.parts[schema_part]];
            rewritten.push_str(&raw[copied..token.start]);
            rewritten.push_str(&spelled_like(&token, raw, next_local));
            copied = token.end;
            break;
        }
    }
    rewritten.push_str(&raw[copied..]);
    rewritten
}
