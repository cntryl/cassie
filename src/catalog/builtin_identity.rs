//! PostgreSQL 18.6 built-in catalog identities.
//!
//! The tables under `builtin_identity/` are exported verbatim from a
//! PostgreSQL 18.6 `initdb` cluster (`pg_type`, `pg_proc`, `pg_operator`,
//! `pg_class`, the portable `pg_collation` rows and the text-search
//! catalogs). They give built-in objects their real OIDs and names, so `reg*`
//! input and output resolve exactly as PostgreSQL resolves them.
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const PG_CATALOG_NAMESPACE_OID: u32 = 11;
pub const PG_TOAST_NAMESPACE_OID: u32 = 99;
pub const PUBLIC_NAMESPACE_OID: u32 = 2200;
pub const INFORMATION_SCHEMA_NAMESPACE_OID: u32 = 13_699;
/// The bootstrap superuser.
pub const BOOTSTRAP_ROLE_OID: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinTypeRow {
    pub oid: u32,
    pub name: String,
    pub typlen: i16,
    pub typtype: char,
    pub category: char,
    pub element: u32,
    pub array: u32,
    /// `format_type(oid, NULL)`, e.g. `integer` or `character varying`.
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinProcRow {
    pub oid: u32,
    pub name: String,
    pub kind: char,
    pub returns_set: bool,
    pub return_type: u32,
    pub argument_types: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinOperatorRow {
    pub oid: u32,
    pub name: String,
    pub kind: char,
    pub left: u32,
    pub right: u32,
    pub result: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinRelationRow {
    pub oid: u32,
    pub namespace: String,
    pub name: String,
    pub kind: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinNamedRow {
    pub oid: u32,
    pub name: String,
}

pub struct BuiltinIdentities {
    pub types: Vec<BuiltinTypeRow>,
    pub procs: Vec<BuiltinProcRow>,
    pub operators: Vec<BuiltinOperatorRow>,
    pub relations: Vec<BuiltinRelationRow>,
    pub collations: Vec<BuiltinNamedRow>,
    pub text_search_configs: Vec<BuiltinNamedRow>,
    pub text_search_dictionaries: Vec<BuiltinNamedRow>,
    pub quoted_keywords: HashSet<String>,
    types_by_oid: HashMap<u32, usize>,
    types_by_name: HashMap<String, usize>,
    procs_by_oid: HashMap<u32, usize>,
    procs_by_name: HashMap<String, Vec<usize>>,
    operators_by_oid: HashMap<u32, usize>,
    operators_by_name: HashMap<String, Vec<usize>>,
}

impl BuiltinIdentities {
    #[must_use]
    pub fn type_by_oid(&self, oid: u32) -> Option<&BuiltinTypeRow> {
        self.types_by_oid.get(&oid).map(|index| &self.types[*index])
    }

    #[must_use]
    pub fn type_by_name(&self, name: &str) -> Option<&BuiltinTypeRow> {
        self.types_by_name
            .get(name)
            .map(|index| &self.types[*index])
    }

    #[must_use]
    pub fn proc_by_oid(&self, oid: u32) -> Option<&BuiltinProcRow> {
        self.procs_by_oid.get(&oid).map(|index| &self.procs[*index])
    }

    pub fn procs_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a BuiltinProcRow> {
        self.procs_by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.procs[*index])
    }

    #[must_use]
    pub fn operator_by_oid(&self, oid: u32) -> Option<&BuiltinOperatorRow> {
        self.operators_by_oid
            .get(&oid)
            .map(|index| &self.operators[*index])
    }

    pub fn operators_named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a BuiltinOperatorRow> {
        self.operators_by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.operators[*index])
    }
}

/// The parsed tables, loaded once per process.
#[must_use]
pub fn builtin_identities() -> &'static BuiltinIdentities {
    static IDENTITIES: OnceLock<BuiltinIdentities> = OnceLock::new();
    IDENTITIES.get_or_init(load)
}

fn load() -> BuiltinIdentities {
    let types = rows(include_str!("builtin_identity/pg_type.tsv"))
        .map(|fields| BuiltinTypeRow {
            oid: number(fields[0]),
            name: fields[1].to_string(),
            typlen: fields[2].parse().unwrap_or(-1),
            typtype: character(fields[3]),
            category: character(fields[4]),
            element: number(fields[5]),
            array: number(fields[6]),
            display: fields[7].to_string(),
        })
        .collect::<Vec<_>>();
    let procs = rows(include_str!("builtin_identity/pg_proc.tsv"))
        .map(|fields| BuiltinProcRow {
            oid: number(fields[0]),
            name: fields[1].to_string(),
            kind: character(fields[2]),
            returns_set: fields[3] == "t",
            return_type: number(fields[4]),
            argument_types: fields
                .get(5)
                .map(|args| args.split_whitespace().map(number).collect())
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let operators = rows(include_str!("builtin_identity/pg_operator.tsv"))
        .map(|fields| BuiltinOperatorRow {
            oid: number(fields[0]),
            name: fields[1].to_string(),
            kind: character(fields[2]),
            left: number(fields[3]),
            right: number(fields[4]),
            result: number(fields[5]),
        })
        .collect::<Vec<_>>();
    let relations = rows(include_str!("builtin_identity/pg_class.tsv"))
        .map(|fields| BuiltinRelationRow {
            oid: number(fields[0]),
            namespace: fields[1].to_string(),
            name: fields[2].to_string(),
            kind: character(fields[3]),
        })
        .collect();
    BuiltinIdentities {
        types_by_oid: index_by(&types, |row| row.oid),
        types_by_name: index_by(&types, |row| row.name.clone()),
        procs_by_oid: index_by(&procs, |row| row.oid),
        procs_by_name: group_by(&procs, |row| row.name.clone()),
        operators_by_oid: index_by(&operators, |row| row.oid),
        operators_by_name: group_by(&operators, |row| row.name.clone()),
        types,
        procs,
        operators,
        relations,
        collations: named_rows(include_str!("builtin_identity/pg_collation.tsv")),
        text_search_configs: named_rows(include_str!("builtin_identity/pg_ts_config.tsv")),
        text_search_dictionaries: named_rows(include_str!("builtin_identity/pg_ts_dict.tsv")),
        quoted_keywords: include_str!("builtin_identity/quoted_keywords.txt")
            .lines()
            .map(str::to_string)
            .collect(),
    }
}

fn rows(table: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    table
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.split('\t').collect())
}

fn named_rows(table: &'static str) -> Vec<BuiltinNamedRow> {
    rows(table)
        .map(|fields| BuiltinNamedRow {
            oid: number(fields[0]),
            name: fields[1].to_string(),
        })
        .collect()
}

fn number(field: &str) -> u32 {
    field.parse().unwrap_or(0)
}

fn character(field: &str) -> char {
    field.chars().next().unwrap_or(' ')
}

fn index_by<T, K: std::hash::Hash + Eq>(items: &[T], key: impl Fn(&T) -> K) -> HashMap<K, usize> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| (key(item), index))
        .collect()
}

fn group_by<T, K: std::hash::Hash + Eq>(
    items: &[T],
    key: impl Fn(&T) -> K,
) -> HashMap<K, Vec<usize>> {
    let mut grouped: HashMap<K, Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        grouped.entry(key(item)).or_default().push(index);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::builtin_identities;

    #[test]
    fn should_load_postgresql_builtin_identities() {
        // Arrange
        let identities = builtin_identities();

        // Act
        let int4 = identities.type_by_name("int4");
        let abs = identities
            .procs_named("abs")
            .find(|row| row.argument_types == [23]);

        // Assert
        assert_eq!(
            int4.map(|row| (row.oid, row.display.as_str())),
            Some((23, "integer"))
        );
        assert_eq!(abs.map(|row| row.oid), Some(1397));
        assert!(identities.quoted_keywords.contains("default"));
    }
}
