//! Shared dependencies backed by Cassie's recorded database CONNECT grants.

use crate::catalog::builtin_identity::BOOTSTRAP_ROLE_OID;
use crate::catalog::object_identity::normal_oid;
use crate::catalog::{Catalog, RoleMeta};
use crate::types::{DataType, ObjectIdKind, Value};

use super::VirtualRow;

const PG_AUTHID_OID: i64 = 1260;
const PG_DATABASE_OID: i64 = 1262;

pub(super) fn schema() -> Vec<(String, DataType)> {
    vec![
        oid("dbid"),
        oid("classid"),
        oid("objid"),
        int("objsubid"),
        oid("refclassid"),
        oid("refobjid"),
        ("deptype".to_owned(), DataType::Char { length: Some(1) }),
    ]
}

pub(super) fn rows(catalog: &Catalog) -> Vec<VirtualRow> {
    let databases = catalog.list_databases();
    let mut rows = Vec::new();

    for role in catalog.list_roles() {
        let Some(role_oid) = role_oid(&role) else {
            continue;
        };
        let Some(granted_databases) = role.database_grants.as_ref() else {
            continue;
        };

        for granted_database in granted_databases {
            let Some(database) = databases
                .iter()
                .find(|database| database.name.eq_ignore_ascii_case(granted_database))
            else {
                continue;
            };

            rows.push(vec![
                oid_value("dbid", 0),
                oid_value("classid", PG_DATABASE_OID),
                oid_value(
                    "objid",
                    super::virtual_views_pg::database_oid(&database.name),
                ),
                int_value("objsubid", 0),
                oid_value("refclassid", PG_AUTHID_OID),
                oid_value("refobjid", role_oid),
                string_value("deptype", "a"),
            ]);
        }
    }

    rows
}

fn role_oid(role: &RoleMeta) -> Option<i64> {
    let oid = if role.is_admin && role.object_id == 0 {
        BOOTSTRAP_ROLE_OID
    } else {
        normal_oid(role.object_id)?
    };
    Some(i64::from(oid))
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}

fn int(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Int)
}

fn oid_value(name: &str, value: i64) -> (String, Value) {
    (name.to_owned(), Value::Int64(value))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_owned(), Value::Int64(value))
}

fn string_value(name: &str, value: &str) -> (String, Value) {
    (name.to_owned(), Value::String(value.to_owned()))
}
