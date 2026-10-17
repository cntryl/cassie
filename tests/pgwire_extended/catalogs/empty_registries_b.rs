//! Native catalog column names across text/binary empty exports; native domain OIDs remain pending.
use crate::support_bind_planning as planning;
use crate::support_catalog_read_model_wire as fixture;

#[test]
fn should_describe_pg_seclabel_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_seclabel LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_seclabel", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &["objoid", "classoid", "objsubid", "provider", "label"],
    );
}

#[test]
fn should_describe_pg_shdescription_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_shdescription LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_shdescription", sql);
    // Assert
    fixture::assert_empty(&batches, &["objoid", "classoid", "description"]);
}

#[test]
fn should_describe_pg_shseclabel_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_shseclabel LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_shseclabel", sql);
    // Assert
    fixture::assert_empty(&batches, &["objoid", "classoid", "provider", "label"]);
}

#[test]
fn should_describe_pg_subscription_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_subscription LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_subscription", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
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
        ],
    );
}

#[test]
fn should_describe_pg_subscription_rel_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_subscription_rel LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_subscription_rel", sql);
    // Assert
    fixture::assert_empty(&batches, &["srsubid", "srrelid", "srsubstate", "srsublsn"]);
}

#[test]
fn should_describe_pg_transform_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_transform LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_transform", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &["oid", "trftype", "trflang", "trffromsql", "trftosql"],
    );
}

#[test]
fn should_describe_pg_user_mapping_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_user_mapping LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_user_mapping", sql);
    // Assert
    fixture::assert_empty(&batches, &["oid", "umuser", "umserver", "umoptions"]);
}

#[test]
fn should_describe_pg_enum_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_enum LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_enum", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &["oid", "enumtypid", "enumsortorder", "enumlabel"],
    );
}

#[test]
fn should_describe_pg_partitioned_table_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_partitioned_table LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_partitioned_table", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "partrelid",
            "partstrat",
            "partnatts",
            "partdefid",
            "partattrs",
            "partclass",
            "partcollation",
            "partexprs",
        ],
    );
}

#[test]
fn should_describe_pg_trigger_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_trigger LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_trigger", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "tgrelid",
            "tgparentid",
            "tgname",
            "tgfoid",
            "tgtype",
            "tgenabled",
            "tgisinternal",
            "tgconstrrelid",
            "tgconstrindid",
            "tgconstraint",
            "tgdeferrable",
            "tginitdeferred",
            "tgnargs",
            "tgattr",
            "tgargs",
            "tgqual",
            "tgoldtable",
            "tgnewtable",
        ],
    );
}

#[test]
fn should_describe_pg_policy_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_policy LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_policy", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "polname",
            "polrelid",
            "polcmd",
            "polpermissive",
            "polroles",
            "polqual",
            "polwithcheck",
        ],
    );
}

#[test]
fn should_describe_pg_event_trigger_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_event_trigger LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_event_trigger", sql);
    // Assert
    fixture::assert_empty(
        &batches,
        &[
            "oid",
            "evtname",
            "evtevent",
            "evtowner",
            "evtfoid",
            "evtenabled",
            "evttags",
        ],
    );
}

#[test]
fn should_describe_pg_parameter_acl_given_no_objects_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_parameter_acl LIMIT 0";
    // Act
    let batches = planning::pruned("catalog209-wire-pg_parameter_acl", sql);
    // Assert
    fixture::assert_empty(&batches, &["oid", "parname", "paracl"]);
}
