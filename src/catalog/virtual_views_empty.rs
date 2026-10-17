//! Experimental column-name shapes for object families absent from the current catalog.
//! These are existing logical carriers, not a claim of native catalog-domain ABI.
use crate::types::DataType;

pub(super) fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    Some(
        fields(name)?
            .iter()
            .map(|(name, oid)| ((*name).to_owned(), carrier(*oid)))
            .collect(),
    )
}

type Fields = &'static [(&'static str, u32)];

const REGISTRIES: &[(&str, Fields)] = &[
    (
        "pg_catalog.pg_auth_members",
        &[
            ("oid", 26),
            ("roleid", 26),
            ("member", 26),
            ("grantor", 26),
            ("admin_option", 16),
            ("inherit_option", 16),
            ("set_option", 16),
        ],
    ),
    (
        "pg_catalog.pg_db_role_setting",
        &[("setdatabase", 26), ("setrole", 26), ("setconfig", 1009)],
    ),
    (
        "pg_catalog.pg_default_acl",
        &[
            ("oid", 26),
            ("defaclrole", 26),
            ("defaclnamespace", 26),
            ("defaclobjtype", 18),
            ("defaclacl", 1034),
        ],
    ),
    (
        "pg_catalog.pg_foreign_data_wrapper",
        &[
            ("oid", 26),
            ("fdwname", 19),
            ("fdwowner", 26),
            ("fdwhandler", 26),
            ("fdwvalidator", 26),
            ("fdwacl", 1034),
            ("fdwoptions", 1009),
        ],
    ),
    (
        "pg_catalog.pg_foreign_server",
        &[
            ("oid", 26),
            ("srvname", 19),
            ("srvowner", 26),
            ("srvfdw", 26),
            ("srvtype", 25),
            ("srvversion", 25),
            ("srvacl", 1034),
            ("srvoptions", 1009),
        ],
    ),
    (
        "pg_catalog.pg_foreign_table",
        &[("ftrelid", 26), ("ftserver", 26), ("ftoptions", 1009)],
    ),
    (
        "pg_catalog.pg_init_privs",
        &[
            ("objoid", 26),
            ("classoid", 26),
            ("objsubid", 23),
            ("privtype", 18),
            ("initprivs", 1034),
        ],
    ),
    (
        "pg_catalog.pg_largeobject",
        &[("loid", 26), ("pageno", 23), ("data", 17)],
    ),
    (
        "pg_catalog.pg_largeobject_metadata",
        &[("oid", 26), ("lomowner", 26), ("lomacl", 1034)],
    ),
    (
        "pg_catalog.pg_publication",
        &[
            ("oid", 26),
            ("pubname", 19),
            ("pubowner", 26),
            ("puballtables", 16),
            ("pubinsert", 16),
            ("pubupdate", 16),
            ("pubdelete", 16),
            ("pubtruncate", 16),
            ("pubviaroot", 16),
            ("pubgencols", 18),
        ],
    ),
    (
        "pg_catalog.pg_publication_namespace",
        &[("oid", 26), ("pnpubid", 26), ("pnnspid", 26)],
    ),
    (
        "pg_catalog.pg_publication_rel",
        &[
            ("oid", 26),
            ("prpubid", 26),
            ("prrelid", 26),
            ("prqual", 194),
            ("prattrs", 22),
        ],
    ),
    (
        "pg_catalog.pg_replication_origin",
        &[("roident", 26), ("roname", 25)],
    ),
    (
        "pg_catalog.pg_seclabel",
        &[
            ("objoid", 26),
            ("classoid", 26),
            ("objsubid", 23),
            ("provider", 25),
            ("label", 25),
        ],
    ),
    (
        "pg_catalog.pg_shdescription",
        &[("objoid", 26), ("classoid", 26), ("description", 25)],
    ),
    (
        "pg_catalog.pg_shseclabel",
        &[
            ("objoid", 26),
            ("classoid", 26),
            ("provider", 25),
            ("label", 25),
        ],
    ),
    (
        "pg_catalog.pg_subscription",
        &[
            ("oid", 26),
            ("subdbid", 26),
            ("subskiplsn", 3220),
            ("subname", 19),
            ("subowner", 26),
            ("subenabled", 16),
            ("subbinary", 16),
            ("substream", 18),
            ("subtwophasestate", 18),
            ("subdisableonerr", 16),
            ("subpasswordrequired", 16),
            ("subrunasowner", 16),
            ("subfailover", 16),
            ("subconninfo", 25),
            ("subslotname", 19),
            ("subsynccommit", 25),
            ("subpublications", 1009),
            ("suborigin", 25),
        ],
    ),
    (
        "pg_catalog.pg_subscription_rel",
        &[
            ("srsubid", 26),
            ("srrelid", 26),
            ("srsubstate", 18),
            ("srsublsn", 3220),
        ],
    ),
    (
        "pg_catalog.pg_transform",
        &[
            ("oid", 26),
            ("trftype", 26),
            ("trflang", 26),
            ("trffromsql", 24),
            ("trftosql", 24),
        ],
    ),
    (
        "pg_catalog.pg_user_mapping",
        &[
            ("oid", 26),
            ("umuser", 26),
            ("umserver", 26),
            ("umoptions", 1009),
        ],
    ),
    (
        "pg_catalog.pg_enum",
        &[
            ("oid", 26),
            ("enumtypid", 26),
            ("enumsortorder", 700),
            ("enumlabel", 19),
        ],
    ),
    (
        "pg_catalog.pg_partitioned_table",
        &[
            ("partrelid", 26),
            ("partstrat", 18),
            ("partnatts", 21),
            ("partdefid", 26),
            ("partattrs", 22),
            ("partclass", 30),
            ("partcollation", 30),
            ("partexprs", 194),
        ],
    ),
    (
        "pg_catalog.pg_trigger",
        &[
            ("oid", 26),
            ("tgrelid", 26),
            ("tgparentid", 26),
            ("tgname", 19),
            ("tgfoid", 26),
            ("tgtype", 21),
            ("tgenabled", 18),
            ("tgisinternal", 16),
            ("tgconstrrelid", 26),
            ("tgconstrindid", 26),
            ("tgconstraint", 26),
            ("tgdeferrable", 16),
            ("tginitdeferred", 16),
            ("tgnargs", 21),
            ("tgattr", 22),
            ("tgargs", 17),
            ("tgqual", 194),
            ("tgoldtable", 19),
            ("tgnewtable", 19),
        ],
    ),
    (
        "pg_catalog.pg_policy",
        &[
            ("oid", 26),
            ("polname", 19),
            ("polrelid", 26),
            ("polcmd", 18),
            ("polpermissive", 16),
            ("polroles", 1028),
            ("polqual", 194),
            ("polwithcheck", 194),
        ],
    ),
    (
        "pg_catalog.pg_event_trigger",
        &[
            ("oid", 26),
            ("evtname", 19),
            ("evtevent", 19),
            ("evtowner", 26),
            ("evtfoid", 26),
            ("evtenabled", 18),
            ("evttags", 1009),
        ],
    ),
    (
        "pg_catalog.pg_parameter_acl",
        &[("oid", 26), ("parname", 25), ("paracl", 1034)],
    ),
];

fn fields(name: &str) -> Option<Fields> {
    REGISTRIES
        .iter()
        .find_map(|(relation, fields)| (*relation == name).then_some(*fields))
}

pub(super) fn rows(name: &str) -> Option<Vec<super::VirtualRow>> {
    fields(name).map(|_| Vec::new())
}

fn carrier(oid: u32) -> DataType {
    match oid {
        16 => DataType::Boolean,
        17 => DataType::Bytea,
        20 | 26 => DataType::BigInt,
        21 => DataType::SmallInt,
        23 => DataType::Int,
        700 | 701 => DataType::Float,
        1005 => DataType::Array(Box::new(DataType::SmallInt)),
        1009 => DataType::Array(Box::new(DataType::Text)),
        // Opaque catalog identities have no admitted value here because these
        // registries have no objects. Their native type/codecs remain #793 work.
        _ => DataType::Text,
    }
}
