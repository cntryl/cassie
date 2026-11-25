//! Native catalog column shapes and empty registries for absent object families.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_report_no_pg_auth_members_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_auth_members", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_auth_members");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_db_role_setting_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_db_role_setting", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_db_role_setting");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_default_acl_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_default_acl", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_default_acl");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_data_wrapper_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_data_wrapper", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_data_wrapper");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_server_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_server", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_server");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_table_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_table", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_table");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_init_privs_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_init_privs", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_init_privs");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_largeobject_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_largeobject", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_largeobject");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_largeobject_metadata_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_largeobject_metadata", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_largeobject_metadata");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_namespace_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication_namespace", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication_namespace");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_rel_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication_rel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication_rel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_replication_origin_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_replication_origin", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_replication_origin");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_seclabel_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_seclabel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_seclabel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_shdescription_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_shdescription", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_shdescription");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_shseclabel_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_shseclabel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_shseclabel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_subscription_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_subscription", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_subscription");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_subscription_rel_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_subscription_rel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_subscription_rel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_transform_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_transform", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_transform");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_user_mapping_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_user_mapping", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_user_mapping");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_partitioned_table_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_partitioned_table", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_partitioned_table");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_partitioned_table_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_partitioned_table", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_partitioned_table LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "partrelid",
            "partstrat",
            "partnatts",
            "partdefid",
            "partattrs",
            "partclass",
            "partcollation",
            "partexprs"
        ]
    );
}

#[test]
fn should_report_no_pg_trigger_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_trigger");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_trigger_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_trigger LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
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
            "tgnewtable"
        ]
    );
}

#[test]
fn should_report_no_pg_policy_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_policy", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_policy");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_policy_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_policy", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_policy LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "oid",
            "polname",
            "polrelid",
            "polcmd",
            "polpermissive",
            "polroles",
            "polqual",
            "polwithcheck"
        ]
    );
}

#[test]
fn should_report_no_pg_event_trigger_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_event_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_event_trigger");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_event_trigger_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_event_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_event_trigger LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "oid",
            "evtname",
            "evtevent",
            "evtowner",
            "evtfoid",
            "evtenabled",
            "evttags"
        ]
    );
}

#[test]
fn should_report_no_pg_parameter_acl_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_parameter_acl", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_parameter_acl");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_parameter_acl_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_parameter_acl", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_parameter_acl LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["oid", "parname", "paracl"]
    );
}

#[test]
fn should_read_empty_object_families_with_the_psql_table_description_queries() {
    // Arrange
    let fixture = sql_fixture(
        "psql-describe-absent-registries",
        &["CREATE TABLE describe_empty (id INTEGER)"],
    );
    let result = fixture
        .execute("SELECT oid FROM pg_catalog.pg_class WHERE relname = 'describe_empty'")
        .expect("table OID");
    let Value::Int64(oid) = result.rows()[0][0] else {
        panic!("expected a table OID")
    };
    let queries = [
        "SELECT pol.polname, pol.polpermissive,
  CASE WHEN pol.polroles = '{0}' THEN NULL ELSE pg_catalog.array_to_string(array(select rolname from pg_catalog.pg_roles where oid = any (pol.polroles) order by 1),',') END,
  pg_catalog.pg_get_expr(pol.polqual, pol.polrelid),
  pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid),
  CASE pol.polcmd
    WHEN 'r' THEN 'SELECT'
    WHEN 'a' THEN 'INSERT'
    WHEN 'w' THEN 'UPDATE'
    WHEN 'd' THEN 'DELETE'
    END AS cmd
FROM pg_catalog.pg_policy pol
WHERE pol.polrelid = '{oid}' ORDER BY 1",
        "SELECT oid, stxrelid::pg_catalog.regclass, stxnamespace::pg_catalog.regnamespace::pg_catalog.text AS nsp, stxname,
pg_catalog.pg_get_statisticsobjdef_columns(oid) AS columns,
  'd' = any(stxkind) AS ndist_enabled,
  'f' = any(stxkind) AS deps_enabled,
  'm' = any(stxkind) AS mcv_enabled,
stxstattarget
FROM pg_catalog.pg_statistic_ext
WHERE stxrelid = '{oid}'
ORDER BY nsp, stxname",
        "SELECT pubname
     , NULL
     , NULL
FROM pg_catalog.pg_publication p
     JOIN pg_catalog.pg_publication_namespace pn ON p.oid = pn.pnpubid
     JOIN pg_catalog.pg_class pc ON pc.relnamespace = pn.pnnspid
WHERE pc.oid ='{oid}' and pg_catalog.pg_relation_is_publishable('{oid}')
UNION
SELECT pubname
     , pg_get_expr(pr.prqual, c.oid)
     , (CASE WHEN pr.prattrs IS NOT NULL THEN
         (SELECT string_agg(attname, ', ')
           FROM pg_catalog.generate_series(0, pg_catalog.array_upper(pr.prattrs::pg_catalog.int2[], 1)) s,
                pg_catalog.pg_attribute
          WHERE attrelid = pr.prrelid AND attnum = prattrs[s])
        ELSE NULL END) FROM pg_catalog.pg_publication p
     JOIN pg_catalog.pg_publication_rel pr ON p.oid = pr.prpubid
     JOIN pg_catalog.pg_class c ON c.oid = pr.prrelid
WHERE pr.prrelid = '{oid}'
UNION
SELECT pubname
     , NULL
     , NULL
FROM pg_catalog.pg_publication p
WHERE p.puballtables AND pg_catalog.pg_relation_is_publishable('{oid}')
ORDER BY 1",
        "SELECT c.oid::pg_catalog.regclass
FROM pg_catalog.pg_class c, pg_catalog.pg_inherits i
WHERE c.oid = i.inhparent AND i.inhrelid = '{oid}'
  AND c.relkind != 'p' AND c.relkind != 'I'
ORDER BY inhseqno",
        "SELECT c.oid::pg_catalog.regclass, c.relkind, inhdetachpending, pg_catalog.pg_get_expr(c.relpartbound, c.oid)
FROM pg_catalog.pg_class c, pg_catalog.pg_inherits i
WHERE c.oid = i.inhrelid AND i.inhparent = '{oid}'
ORDER BY pg_catalog.pg_get_expr(c.relpartbound, c.oid) = 'DEFAULT', c.oid::pg_catalog.regclass::pg_catalog.text",
    ];

    // Act
    for query in queries {
        let query = query.replace("{oid}", &oid.to_string());
        let result = fixture
            .execute(&query)
            .unwrap_or_else(|error| panic!("{query}: {error}"));

        // Assert
        assert!(result.rows().is_empty(), "{query}");
    }
}

#[test]
fn should_resolve_publishability_and_missing_extended_statistics_by_identity() {
    // Arrange
    let fixture = sql_fixture(
        "describe-object-identity",
        &[
            "CREATE TABLE published_kind (id INTEGER)",
            "CREATE INDEX published_kind_id ON published_kind (id)",
            "CREATE VIEW published_view AS SELECT id FROM published_kind",
        ],
    );

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_relation_is_publishable('published_kind'),
                pg_relation_is_publishable('published_kind_id'::regclass),
                pg_relation_is_publishable('pg_catalog.pg_class'),
                pg_relation_is_publishable('published_view'),
                pg_relation_is_publishable('0'), pg_relation_is_publishable(NULL),
                pg_catalog.pg_get_statisticsobjdef_columns(0::oid),
                pg_get_statisticsobjdef_columns(NULL)",
        )
        .expect("evaluate psql catalog helpers");

    // Assert
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null
        ]]
    );
}

#[test]
fn should_compare_a_catalog_oid_with_the_psql_quoted_oid_literal() {
    // Arrange
    let fixture = sql_fixture(
        "describe-oid-literal",
        &["CREATE TABLE described_oid (id INTEGER)"],
    );
    let table = fixture
        .execute("SELECT oid FROM pg_catalog.pg_class WHERE relname='described_oid'")
        .expect("lookup OID");
    let Value::Int64(oid) = table.rows()[0][0] else {
        panic!("expected an OID")
    };

    // Act
    let result = fixture
        .execute(&format!(
            "SELECT relname FROM pg_catalog.pg_class WHERE oid = '{oid}' AND '{oid}' = oid"
        ))
        .expect("compare OID");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![Value::String("described_oid".into())]]
    );
}

#[test]
fn should_expose_the_same_role_oid_to_policy_lookups_and_regrole() {
    // Arrange
    let fixture = sql_fixture("describe-policy-roles", &[]);
    fixture.cassie.startup().expect("initialize role catalog");
    fixture
        .execute("CREATE ROLE describe_reader NOLOGIN")
        .expect("create role");

    // Act
    let result = fixture
        .execute(
            "SELECT rolname FROM pg_catalog.pg_roles
         WHERE oid = ANY(ARRAY['root'::regrole::oid, 'describe_reader'::regrole::oid])
         ORDER BY rolname",
        )
        .expect("resolve policy role OIDs");

    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::String("describe_reader".into())],
            vec![Value::String("root".into())]
        ]
    );
}
