//! `regproc` and `regprocedure` over the built-in `pg_proc` rows together with
//! the database's user-defined functions and procedures.
//!
//! A user routine's OID comes from the `object_id` allocated when it was
//! created, so it survives restart and `CREATE OR REPLACE` and is never reused.
//! A routine stored without an `object_id` publishes no OID.
use super::names::{builtin_type_oid, qualified_name, quote_identifier, signature};
use super::ObjectScope;
use crate::catalog::builtin_identity::{builtin_array_oid, builtin_identities, BuiltinProcRow};
use crate::catalog::{parse_name, ParsedName, DEFAULT_SCHEMA};
use crate::types::{DataType, ObjectIdKind, ObjectIdentifierError};

const PG_CATALOG: &str = "pg_catalog";
const FUNCTION_KIND: char = 'f';
const PROCEDURE_KIND: char = 'p';

/// One routine a name can denote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Routine {
    pub(super) oid: u32,
    pub(super) schema: String,
    pub(super) name: String,
    /// `pg_proc.prokind`: `f` function, `p` procedure, `a` aggregate, `w` window.
    pub(super) kind: char,
    pub(super) arguments: Vec<u32>,
}

impl Routine {
    fn builtin(row: &BuiltinProcRow) -> Self {
        Self {
            oid: row.oid,
            schema: PG_CATALOG.to_string(),
            name: row.name.clone(),
            kind: row.kind,
            arguments: row.argument_types.clone(),
        }
    }
}

impl ObjectScope<'_> {
    /// Resolves `regproc` text: a routine name that must denote one routine.
    pub(super) fn resolve_proc(&self, text: &str) -> Result<Option<u32>, ObjectIdentifierError> {
        let (qualifier, name) = self.routine_name(text)?;
        let mut candidates = self
            .visible_routines(qualifier.as_deref(), &name)
            .into_iter();
        match (candidates.next(), candidates.next()) {
            (None, _) => Ok(None),
            (Some(routine), None) => Ok(Some(routine.oid)),
            (Some(_), Some(_)) => Err(ObjectIdentifierError::Ambiguous {
                kind: ObjectIdKind::RegProc,
                name: text.trim().to_string(),
            }),
        }
    }

    /// Resolves `regprocedure` text: a routine name and its exact argument types.
    pub(super) fn resolve_procedure(
        &self,
        text: &str,
    ) -> Result<Option<u32>, ObjectIdentifierError> {
        let (name, arguments) = signature(text)?;
        let argument_types = arguments
            .iter()
            .map(|argument| self.argument_type_oid(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let (qualifier, name) = self.routine_name(&name)?;
        Ok(self
            .visible_routines(qualifier.as_deref(), &name)
            .into_iter()
            .find(|routine| routine.arguments == argument_types)
            .map(|routine| routine.oid))
    }

    /// `regproc` output: the name, schema-qualified unless it alone finds the
    /// routine through the search path.
    pub(super) fn proc_label(&self, oid: u32) -> Option<String> {
        let routine = self.routine_by_oid(oid)?;
        let candidates = self.visible_routines(None, &routine.name);
        let unique = matches!(candidates.as_slice(), [only] if only.oid == oid);
        Some(self.routine_display_name(&routine, unique))
    }

    /// `regprocedure` output: the name with its argument type list.
    pub(super) fn procedure_label(&self, oid: u32) -> Option<String> {
        let routine = self.routine_by_oid(oid)?;
        let visible = self
            .visible_routines(None, &routine.name)
            .into_iter()
            .find(|candidate| candidate.arguments == routine.arguments)
            .is_some_and(|candidate| candidate.oid == oid);
        Some(format!(
            "{}({})",
            self.routine_display_name(&routine, visible),
            self.argument_list(&routine)
        ))
    }

    /// The routine with `oid`, built-in or user-defined.
    pub(super) fn routine_by_oid(&self, oid: u32) -> Option<Routine> {
        if let Some(row) = builtin_identities().proc_by_oid(oid) {
            return Some(Routine::builtin(row));
        }
        self.user_routines()
            .into_iter()
            .find(|routine| routine.oid == oid)
    }

    /// The comma-joined `regtype` output of a routine's argument types.
    pub(super) fn argument_list(&self, routine: &Routine) -> String {
        routine
            .arguments
            .iter()
            .map(|oid| self.label(ObjectIdKind::RegType, *oid))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn routine_display_name(&self, routine: &Routine, bare: bool) -> String {
        if bare {
            quote_identifier(&routine.name)
        } else {
            format!(
                "{}.{}",
                quote_identifier(&routine.schema),
                quote_identifier(&routine.name)
            )
        }
    }

    /// Splits routine name text into an optional schema and the name.
    fn routine_name(&self, text: &str) -> Result<(Option<String>, String), ObjectIdentifierError> {
        let parts = qualified_name(text)?;
        match parts.as_slice() {
            [name] => Ok((None, name.value.clone())),
            [schema, name] => Ok((Some(schema.value.clone()), name.value.clone())),
            [database, schema, name]
                if database.value.eq_ignore_ascii_case(&self.context.database) =>
            {
                Ok((Some(schema.value.clone()), name.value.clone()))
            }
            _ => Err(ObjectIdentifierError::InvalidName(text.to_string())),
        }
    }

    /// Routines named `name` that the search path (or `qualifier`) exposes, in
    /// path order. A routine whose argument types repeat an earlier one is
    /// shadowed by it.
    fn visible_routines(&self, qualifier: Option<&str>, name: &str) -> Vec<Routine> {
        let schemas = qualifier.map_or_else(
            || {
                self.context
                    .relation_search_order()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            },
            |schema| vec![schema.to_string()],
        );
        let user = self.user_routines();
        let mut found: Vec<Routine> = Vec::new();
        for schema in schemas {
            let builtin = schema
                .eq_ignore_ascii_case(PG_CATALOG)
                .then(|| builtin_identities().procs_named(name).map(Routine::builtin))
                .into_iter()
                .flatten();
            let defined = user
                .iter()
                .filter(|routine| {
                    routine.schema.eq_ignore_ascii_case(&schema)
                        && routine.name.eq_ignore_ascii_case(name)
                })
                .cloned();
            for routine in builtin.chain(defined) {
                if !found
                    .iter()
                    .any(|known| known.arguments == routine.arguments)
                {
                    found.push(routine);
                }
            }
        }
        found
    }

    /// The user-defined functions and procedures of the session's database.
    pub(super) fn user_routines(&self) -> Vec<Routine> {
        let functions = self.catalog.list_functions().into_iter().map(|function| {
            (
                function.name,
                function.args,
                function.object_id,
                FUNCTION_KIND,
            )
        });
        let procedures = self.catalog.list_procedures().into_iter().map(|procedure| {
            (
                procedure.name,
                procedure.args,
                procedure.object_id,
                PROCEDURE_KIND,
            )
        });
        functions
            .chain(procedures)
            .filter_map(|(canonical, arguments, object_id, kind)| {
                let (schema, name) = self.routine_location(&canonical)?;
                Some(Routine {
                    oid: super::normal_oid(object_id)?,
                    schema,
                    name,
                    kind,
                    arguments: arguments
                        .iter()
                        .map(|argument| self.data_type_oid(&argument.data_type))
                        .collect(),
                })
            })
            .collect()
    }

    /// The schema and local name of a stored routine in the session's database.
    fn routine_location(&self, canonical: &str) -> Option<(String, String)> {
        let unscoped = self.catalog.list_databases().is_empty();
        match parse_name(canonical).ok()? {
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => database
                .eq_ignore_ascii_case(&self.context.database)
                .then_some((schema, name)),
            ParsedName::SchemaQualified { schema, name } if unscoped => Some((schema, name)),
            ParsedName::Unqualified(name) if unscoped => Some((DEFAULT_SCHEMA.to_string(), name)),
            _ => None,
        }
    }

    /// The OID of a type named in a routine signature.
    fn argument_type_oid(&self, text: &str) -> Result<u32, ObjectIdentifierError> {
        if let Some(oid) = builtin_type_oid(text)? {
            return Ok(oid);
        }
        self.resolve_user_type(text)
            .ok_or_else(|| ObjectIdentifierError::Undefined {
                kind: ObjectIdKind::RegType,
                name: text.to_string(),
            })
    }

    /// The `pg_type.oid` a declared argument type has; array dimensions
    /// collapse to the one-dimensional array type as in PostgreSQL.
    fn data_type_oid(&self, data_type: &DataType) -> u32 {
        let mut element = data_type;
        let mut is_array = false;
        while let DataType::Array(inner) = element {
            element = inner;
            is_array = true;
        }
        if let DataType::Composite(descriptor) = element {
            return if is_array {
                descriptor.array_oid
            } else {
                descriptor.oid
            };
        }
        let oid = super::type_oid_for_database(self.catalog, &self.context.database, element)
            .and_then(|oid| u32::try_from(oid).ok())
            .unwrap_or(0);
        if is_array {
            builtin_array_oid(oid).unwrap_or(0)
        } else {
            oid
        }
    }
}

#[cfg(test)]
#[path = "user_routines_tests.rs"]
mod tests;
