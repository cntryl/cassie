//! The NOTICE of `ALTER TYPE ... ADD VALUE IF NOT EXISTS` on an existing label (#827).

use super::{Cassie, CassieSession};
use crate::sql::ast::{ParsedStatement, QueryStatement, TypeAction, TypeAlteration};

impl Cassie {
    /// PostgreSQL's `enum label "x" already exists, skipping` when `parsed` is
    /// `ADD VALUE IF NOT EXISTS` of a label the type already has, judged
    /// against the catalog `session` reads.
    pub(crate) fn enum_add_value_skip_notice(
        &self,
        session: &CassieSession,
        parsed: &ParsedStatement,
    ) -> Option<String> {
        let QueryStatement::Type(statement) = &parsed.statement else {
            return None;
        };
        let TypeAction::Alter {
            name,
            alteration:
                TypeAlteration::AddValue {
                    label,
                    if_not_exists: true,
                    ..
                },
        } = &statement.action
        else {
            return None;
        };
        let context = self.binding_context_for_session(Some(session));
        let metadata = self.type_catalog(Some(session)).resolve_enum_type(
            &context.database,
            &context.relation_search_order(),
            name,
        )?;
        metadata
            .members
            .iter()
            .any(|member| &member.label == label)
            .then(|| format!("enum label \"{label}\" already exists, skipping"))
    }
}

#[cfg(test)]
#[path = "enum_notice_tests.rs"]
mod tests;
