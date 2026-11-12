//! Finite pgvector catalog rows derived from the active database's extension identities.

use crate::catalog::object_identity::{normal_oid, stored_namespace_oid, type_oid_for_database};
use crate::catalog::{Catalog, VectorAnnCatalogIdentityIds};
use crate::types::{DataType, ObjectIdKind};

use super::{bool_value, int_value, string, VirtualRow};

const FLOAT8_BTREE_OPERATOR_FAMILY_OID: i64 = 1_970;
// pgvector 0.8.7 declares each distance member `FOR ORDER BY float_ops`; this is
// the PostgreSQL 18.6 built-in btree family identity from the pinned catalog input.

#[derive(Clone, Copy)]
struct AnnMethodIdentity {
    name: &'static str,
    access_method_oid: i64,
    family_oids: [i64; 3],
    operator_class_oids: [i64; 3],
}

#[derive(Clone, Copy)]
struct DistanceOperatorIdentity {
    name: &'static str,
    oid: i64,
}

struct InstalledVectorCatalog {
    namespace_oid: i64,
    owner_oid: i64,
    type_oid: i64,
    operators: [DistanceOperatorIdentity; 3],
    methods: [AnnMethodIdentity; 2],
}

#[derive(Clone, Copy)]
struct DistanceClassIdentity {
    class_name: &'static str,
    operator: DistanceOperatorIdentity,
    family_oid: i64,
    operator_class_oid: i64,
    is_default: bool,
}

pub(in crate::catalog::virtual_views) fn operator_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        name("oprname"),
        oid("oprnamespace"),
        oid("oprowner"),
        text("oprkind"),
        boolean("oprcanmerge"),
        boolean("oprcanhash"),
        oid("oprleft"),
        oid("oprright"),
        oid("oprresult"),
        oid("oprcom"),
        oid("oprnegate"),
    ]
}

pub(in crate::catalog::virtual_views) fn operator_family_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        oid("opfmethod"),
        name("opfname"),
        oid("opfnamespace"),
        oid("opfowner"),
    ]
}

pub(in crate::catalog::virtual_views) fn operator_class_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        oid("opcmethod"),
        name("opcname"),
        oid("opcnamespace"),
        oid("opcowner"),
        oid("opcfamily"),
        oid("opcintype"),
        boolean("opcdefault"),
        oid("opckeytype"),
    ]
}

pub(in crate::catalog::virtual_views) fn access_method_operator_schema() -> Vec<(String, DataType)> {
    vec![
        oid("amopfamily"),
        oid("amoplefttype"),
        oid("amoprighttype"),
        ("amopstrategy".to_owned(), DataType::SmallInt),
        text("amoppurpose"),
        oid("amopopr"),
        oid("amopmethod"),
        oid("amopsortfamily"),
    ]
}

/// Return built-in access methods and the two methods owned by installed vector.
pub(in crate::catalog::virtual_views) fn access_method_rows(
    catalog: &Catalog,
    database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = vec![
        vec![int_value("oid", 2), string("amname", "heap")],
        vec![int_value("oid", 403), string("amname", "btree")],
        vec![int_value("oid", 405), string("amname", "hash")],
        vec![int_value("oid", 2742), string("amname", "gin")],
    ];
    if let Some(vector) = InstalledVectorCatalog::load(catalog, database) {
        rows.extend(vector.methods.into_iter().map(|method| {
            vec![
                int_value("oid", method.access_method_oid),
                string("amname", method.name),
            ]
        }));
    }
    rows
}

pub(in crate::catalog::virtual_views) fn operator_rows(
    catalog: &Catalog,
    database: Option<&str>,
) -> Vec<VirtualRow> {
    let Some(vector) = InstalledVectorCatalog::load(catalog, database) else {
        return Vec::new();
    };
    vector
        .operators
        .into_iter()
        .map(|operator| {
            vec![
                int_value("oid", operator.oid),
                string("oprname", operator.name),
                int_value("oprnamespace", vector.namespace_oid),
                int_value("oprowner", vector.owner_oid),
                string("oprkind", "b"),
                bool_value("oprcanmerge", false),
                bool_value("oprcanhash", false),
                int_value("oprleft", vector.type_oid),
                int_value("oprright", vector.type_oid),
                int_value("oprresult", DataType::Float.type_oid()),
                // pgvector declares each distance operator as its own commutator.
                int_value("oprcom", operator.oid),
                int_value("oprnegate", 0),
            ]
        })
        .collect()
}

pub(in crate::catalog::virtual_views) fn operator_family_rows(
    catalog: &Catalog,
    database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = vec![vec![
        int_value("oid", FLOAT8_BTREE_OPERATOR_FAMILY_OID),
        int_value("opfmethod", 403),
        string("opfname", "float_ops"),
        int_value("opfnamespace", crate::catalog::builtin_identity::PG_CATALOG_NAMESPACE_OID),
        int_value("opfowner", 10),
    ]];
    let Some(vector) = InstalledVectorCatalog::load(catalog, database) else {
        return rows;
    };
    rows.extend(vector
        .methods
        .into_iter()
        .flat_map(|method| {
            distance_classes(&vector, method).map(move |class| {
                vec![
                    int_value("oid", class.family_oid),
                    int_value("opfmethod", method.access_method_oid),
                    string("opfname", class.class_name),
                    int_value("opfnamespace", vector.namespace_oid),
                    int_value("opfowner", vector.owner_oid),
                ]
            })
        }));
    rows
}

pub(in crate::catalog::virtual_views) fn operator_class_rows(
    catalog: &Catalog,
    database: Option<&str>,
) -> Vec<VirtualRow> {
    let Some(vector) = InstalledVectorCatalog::load(catalog, database) else {
        return Vec::new();
    };
    vector
        .methods
        .into_iter()
        .flat_map(|method| {
            distance_classes(&vector, method).map(move |class| {
                vec![
                    int_value("oid", class.operator_class_oid),
                    int_value("opcmethod", method.access_method_oid),
                    string("opcname", class.class_name),
                    int_value("opcnamespace", vector.namespace_oid),
                    int_value("opcowner", vector.owner_oid),
                    int_value("opcfamily", class.family_oid),
                    int_value("opcintype", vector.type_oid),
                    bool_value("opcdefault", class.is_default),
                    int_value("opckeytype", 0),
                ]
            })
        })
        .collect()
}

pub(in crate::catalog::virtual_views) fn access_method_operator_rows(
    catalog: &Catalog,
    database: Option<&str>,
) -> Vec<VirtualRow> {
    let Some(vector) = InstalledVectorCatalog::load(catalog, database) else {
        return Vec::new();
    };
    vector
        .methods
        .into_iter()
        .flat_map(|method| {
            distance_classes(&vector, method).map(move |class| {
                vec![
                    int_value("amopfamily", class.family_oid),
                    int_value("amoplefttype", vector.type_oid),
                    int_value("amoprighttype", vector.type_oid),
                    int_value("amopstrategy", 1),
                    string("amoppurpose", "o"),
                    int_value("amopopr", class.operator.oid),
                    int_value("amopmethod", method.access_method_oid),
                    int_value(
                        "amopsortfamily",
                        FLOAT8_BTREE_OPERATOR_FAMILY_OID,
                    ),
                ]
            })
        })
        .collect()
}

impl InstalledVectorCatalog {
    fn load(catalog: &Catalog, database: Option<&str>) -> Option<Self> {
        let database = database?;
        let extension = catalog.get_extension(database, "vector")?;
        if !extension.name.eq_ignore_ascii_case("vector") || extension.version != "0.8.7" {
            return None;
        }
        let identities = extension.vector_catalog_identities.as_ref()?;
        let type_oid = type_oid_for_database(catalog, database, &DataType::Vector(None))?;
        let namespace_oid = i64::from(stored_namespace_oid(catalog, &extension.schema)?);
        let operators = DistanceOperatorIdentity {
            name: "<->",
            oid: catalog_oid(identities.operators.l2)?,
        };
        let inner_product = DistanceOperatorIdentity {
            name: "<#>",
            oid: catalog_oid(identities.operators.inner_product)?,
        };
        let cosine = DistanceOperatorIdentity {
            name: "<=>",
            oid: catalog_oid(identities.operators.cosine)?,
        };
        Some(Self {
            namespace_oid,
            owner_oid: i64::from(extension.owner_oid),
            type_oid,
            operators: [operators, inner_product, cosine],
            methods: [
                ann_method_identity("hnsw", &identities.hnsw)?,
                ann_method_identity("ivfflat", &identities.ivfflat)?,
            ],
        })
    }
}

fn ann_method_identity(
    name: &'static str,
    identities: &VectorAnnCatalogIdentityIds,
) -> Option<AnnMethodIdentity> {
    Some(AnnMethodIdentity {
        name,
        access_method_oid: catalog_oid(identities.access_method)?,
        family_oids: [
            catalog_oid(identities.l2_operator_family)?,
            catalog_oid(identities.inner_product_operator_family)?,
            catalog_oid(identities.cosine_operator_family)?,
        ],
        operator_class_oids: [
            catalog_oid(identities.l2_operator_class)?,
            catalog_oid(identities.inner_product_operator_class)?,
            catalog_oid(identities.cosine_operator_class)?,
        ],
    })
}

fn distance_classes(
    vector: &InstalledVectorCatalog,
    method: AnnMethodIdentity,
) -> [DistanceClassIdentity; 3] {
    [
        DistanceClassIdentity {
            class_name: "vector_l2_ops",
            operator: vector.operators[0],
            family_oid: method.family_oids[0],
            operator_class_oid: method.operator_class_oids[0],
            is_default: method.name == "ivfflat",
        },
        DistanceClassIdentity {
            class_name: "vector_ip_ops",
            operator: vector.operators[1],
            family_oid: method.family_oids[1],
            operator_class_oid: method.operator_class_oids[1],
            is_default: false,
        },
        DistanceClassIdentity {
            class_name: "vector_cosine_ops",
            operator: vector.operators[2],
            family_oid: method.family_oids[2],
            operator_class_oid: method.operator_class_oids[2],
            is_default: false,
        },
    ]
}

fn catalog_oid(object_id: u64) -> Option<i64> {
    normal_oid(object_id).map(i64::from)
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}

fn name(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Name)
}

fn text(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Text)
}

fn boolean(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Boolean)
}
