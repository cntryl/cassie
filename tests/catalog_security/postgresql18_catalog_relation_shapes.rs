//! PostgreSQL 18.6 catalog relation descriptors tracked by #817.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn catalog_column_names(fixture: &SqlFixture, relation: &str) -> Vec<String> {
    let sql = format!("SELECT * FROM pg_catalog.{relation} LIMIT 0");
    fixture
        .execute(&sql)
        .unwrap_or_else(|error| panic!("read pg_catalog.{relation} descriptor: {error}"))
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

fn expected_columns(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn should_describe_pg_auth_members_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-auth-members-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_auth_members");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "roleid",
            "member",
            "grantor",
            "admin_option",
            "inherit_option",
            "set_option",
        ])
    );
}

#[test]
fn should_describe_pg_db_role_setting_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-db-role-setting-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_db_role_setting");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["setdatabase", "setrole", "setconfig"])
    );
}

#[test]
fn should_describe_pg_default_acl_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-default-acl-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_default_acl");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "defaclrole",
            "defaclnamespace",
            "defaclobjtype",
            "defaclacl",
        ])
    );
}

#[test]
fn should_describe_pg_depend_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-depend-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_depend");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "classid",
            "objid",
            "objsubid",
            "refclassid",
            "refobjid",
            "refobjsubid",
            "deptype",
        ])
    );
}

#[test]
fn should_describe_pg_foreign_data_wrapper_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-foreign-data-wrapper-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_foreign_data_wrapper");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "fdwname",
            "fdwowner",
            "fdwhandler",
            "fdwvalidator",
            "fdwacl",
            "fdwoptions",
        ])
    );
}

#[test]
fn should_describe_pg_foreign_server_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-foreign-server-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_foreign_server");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "srvname",
            "srvowner",
            "srvfdw",
            "srvtype",
            "srvversion",
            "srvacl",
            "srvoptions",
        ])
    );
}

#[test]
fn should_describe_pg_foreign_table_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-foreign-table-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_foreign_table");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["ftrelid", "ftserver", "ftoptions"])
    );
}

#[test]
fn should_describe_pg_init_privs_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-init-privs-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_init_privs");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["objoid", "classoid", "objsubid", "privtype", "initprivs"])
    );
}

#[test]
fn should_describe_pg_largeobject_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-largeobject-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_largeobject");

    // Assert
    assert_eq!(names, expected_columns(&["loid", "pageno", "data"]));
}

#[test]
fn should_describe_pg_largeobject_metadata_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-largeobject-metadata-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_largeobject_metadata");

    // Assert
    assert_eq!(names, expected_columns(&["oid", "lomowner", "lomacl"]));
}

#[test]
fn should_describe_pg_opfamily_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-opfamily-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_opfamily");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["oid", "opfmethod", "opfname", "opfnamespace", "opfowner"])
    );
}

#[test]
fn should_describe_pg_publication_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-publication-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_publication");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "pubname",
            "pubowner",
            "puballtables",
            "pubinsert",
            "pubupdate",
            "pubdelete",
            "pubtruncate",
            "pubviaroot",
            "pubgencols",
        ])
    );
}

#[test]
fn should_describe_pg_publication_namespace_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-publication-namespace-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_publication_namespace");

    // Assert
    assert_eq!(names, expected_columns(&["oid", "pnpubid", "pnnspid"]));
}

#[test]
fn should_describe_pg_publication_relation_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-publication-rel-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_publication_rel");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["oid", "prpubid", "prrelid", "prqual", "prattrs"])
    );
}

#[test]
fn should_describe_pg_replication_origin_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-replication-origin-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_replication_origin");

    // Assert
    assert_eq!(names, expected_columns(&["roident", "roname"]));
}

#[test]
fn should_describe_pg_rewrite_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-rewrite-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_rewrite");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "rulename",
            "ev_class",
            "ev_type",
            "ev_enabled",
            "is_instead",
            "ev_qual",
            "ev_action",
        ])
    );
}

#[test]
fn should_describe_pg_security_label_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-seclabel-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_seclabel");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["objoid", "classoid", "objsubid", "provider", "label"])
    );
}

#[test]
fn should_describe_pg_shared_dependencies_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-shdepend-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_shdepend");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "dbid",
            "classid",
            "objid",
            "objsubid",
            "refclassid",
            "refobjid",
            "deptype",
        ])
    );
}

#[test]
fn should_describe_pg_shared_descriptions_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-shdescription-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_shdescription");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["objoid", "classoid", "description"])
    );
}

#[test]
fn should_describe_pg_shared_security_label_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-shseclabel-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_shseclabel");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["objoid", "classoid", "provider", "label"])
    );
}

#[test]
fn should_describe_pg_subscription_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-subscription-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_subscription");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "subdbid",
            "subskiplsn",
            "subname",
            "subowner",
            "subenabled",
            "subbinary",
            "substream",
            "subtwophasestate",
            "subdisableonerr",
            "subpasswordrequired",
            "subrunasowner",
            "subfailover",
            "subconninfo",
            "subslotname",
            "subsynccommit",
            "subpublications",
            "suborigin",
        ])
    );
}

#[test]
fn should_describe_pg_subscription_relation_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-subscription-rel-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_subscription_rel");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["srsubid", "srrelid", "srsubstate", "srsublsn"])
    );
}

#[test]
fn should_describe_pg_transform_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-transform-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_transform");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["oid", "trftype", "trflang", "trffromsql", "trftosql"])
    );
}

#[test]
fn should_describe_pg_text_search_dictionary_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-ts-dict-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_ts_dict");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "dictname",
            "dictnamespace",
            "dictowner",
            "dicttemplate",
            "dictinitoption",
        ])
    );
}

#[test]
fn should_describe_pg_text_search_parser_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-ts-parser-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_ts_parser");

    // Assert
    assert_eq!(
        names,
        expected_columns(&[
            "oid",
            "prsname",
            "prsnamespace",
            "prsstart",
            "prstoken",
            "prsend",
            "prsheadline",
            "prslextype",
        ])
    );
}

#[test]
fn should_describe_pg_text_search_template_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-ts-template-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_ts_template");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["oid", "tmplname", "tmplnamespace", "tmplinit", "tmpllexize"])
    );
}

#[test]
fn should_describe_pg_user_mapping_columns() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-user-mapping-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let names = catalog_column_names(&fixture, "pg_user_mapping");

    // Assert
    assert_eq!(
        names,
        expected_columns(&["oid", "umuser", "umserver", "umoptions"])
    );
}
