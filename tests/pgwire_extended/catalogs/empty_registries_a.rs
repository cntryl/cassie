//! Native catalog column names across text/binary empty exports; native domain OIDs remain pending.
use crate::support_bind_planning as planning;
use crate::support_catalog_read_model_wire as fixture;

#[test]
fn should_describe_pg_auth_members_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_auth_members LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_auth_members", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "roleid",
            "member",
            "grantor",
            "admin_option",
            "inherit_option",
            "set_option",
        ],
    );
}

#[test]
fn should_describe_pg_db_role_setting_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_db_role_setting LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_db_role_setting", sql);
    // Assert
    fixture::assert_empty(&batches, &["setdatabase", "setrole", "setconfig"]);
}

#[test]
fn should_describe_pg_default_acl_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_default_acl LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_default_acl", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "defaclrole",
            "defaclnamespace",
            "defaclobjtype",
            "defaclacl",
        ],
    );
}

#[test]
fn should_describe_pg_foreign_data_wrapper_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_foreign_data_wrapper LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_foreign_data_wrapper", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "fdwname",
            "fdwowner",
            "fdwhandler",
            "fdwvalidator",
            "fdwacl",
            "fdwoptions",
        ],
    );
}

#[test]
fn should_describe_pg_foreign_server_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_foreign_server LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_foreign_server", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "srvname",
            "srvowner",
            "srvfdw",
            "srvtype",
            "srvversion",
            "srvacl",
            "srvoptions",
        ],
    );
}

#[test]
fn should_describe_pg_foreign_table_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_foreign_table LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_foreign_table", sql);
    // Assert
    fixture::assert_empty(&batches, &["ftrelid", "ftserver", "ftoptions"]);
}

#[test]
fn should_describe_pg_init_privs_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_init_privs LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_init_privs", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &["objoid", "classoid", "objsubid", "privtype", "initprivs"],
    );
}

#[test]
fn should_describe_pg_largeobject_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_largeobject LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_largeobject", sql);
    // Assert
    fixture::assert_empty(&batches, &["loid", "pageno", "data"]);
}

#[test]
fn should_describe_pg_largeobject_metadata_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_largeobject_metadata LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_largeobject_metadata", sql);
    // Assert
    fixture::assert_empty(&batches, &["oid", "lomowner", "lomacl"]);
}

#[test]
fn should_describe_pg_publication_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_publication LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_publication", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
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
        ],
    );
}

#[test]
fn should_describe_pg_publication_namespace_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_publication_namespace LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_publication_namespace", sql);
    // Assert
    fixture::assert_empty(&batches, &["oid", "pnpubid", "pnnspid"]);
}

#[test]
fn should_describe_pg_publication_rel_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_publication_rel LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_publication_rel", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &["oid", "prpubid", "prrelid", "prqual", "prattrs"],
    );
}

#[test]
fn should_describe_pg_replication_origin_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_replication_origin LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_replication_origin", sql);
    // Assert
    fixture::assert_empty(&batches, &["roident", "roname"]);
}
