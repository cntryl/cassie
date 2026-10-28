use super::*;

struct EnumCatalogSnapshotObservation {
    suffix: &'static str,
    enum_oid: i64,
    expected_catalog_label: &'static str,
    before: (String, String, i64),
    after: (String, String, i64),
    output_type_oids: Vec<i64>,
}

async fn observe_enum_catalog_snapshot_case(
    reader: &tokio_postgres::Client,
    writer: &tokio_postgres::Client,
    suffix: &'static str,
    isolation: &str,
    expected_catalog_label: &'static str,
) -> EnumCatalogSnapshotObservation {
    let type_name = format!("compat_enum_catalog_snapshot_state_{suffix}");
    let table_name = format!("compat_enum_catalog_snapshot_values_{suffix}");
    writer
        .batch_execute(&format!("CREATE TYPE {type_name} AS ENUM ('old', 'later')"))
        .await
        .expect("create the enum for a catalog snapshot case");
    writer
        .batch_execute(&format!("CREATE TABLE {table_name} (value {type_name})"))
        .await
        .expect("create the table with the named enum type");
    writer
        .batch_execute(&format!("INSERT INTO {table_name} VALUES ('old')"))
        .await
        .expect("store the old enum label before its rename");
    let enum_oid = writer
        .query_one(
            &format!(
                "SELECT oid::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = '{type_name}'::regtype"
            ),
            &[],
        )
        .await
        .expect("read the enum type OID before its label rename")
        .get::<_, i64>(0);
    let select = format!(
        "SELECT value::TEXT AS enum_value, \
                (SELECT enumlabel FROM pg_catalog.pg_enum \
                 WHERE enumtypid = '{type_name}'::regtype \
                 ORDER BY enumsortorder LIMIT 1) AS catalog_label, \
                pg_typeof(value)::OID::BIGINT AS enum_type_oid \
         FROM {table_name}"
    );

    reader
        .batch_execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
        .await
        .expect("start the reader transaction at the selected isolation level");
    let before = reader
        .query_one(&select, &[])
        .await
        .expect("read the enum output and catalog label before the rename");
    writer
        .batch_execute(&format!(
            "BEGIN; ALTER TYPE {type_name} RENAME VALUE 'old' TO 'new'; COMMIT"
        ))
        .await
        .expect("commit the label rename in the writer session");
    let after = reader
        .query_one(&select, &[])
        .await
        .expect("repeat the enum output and catalog read in the same transaction");
    let before_values = (
        before.get::<_, String>(0),
        before.get::<_, String>(1),
        before.get::<_, i64>(2),
    );
    let after_values = (
        after.get::<_, String>(0),
        after.get::<_, String>(1),
        after.get::<_, i64>(2),
    );
    let output_type_oids = after
        .columns()
        .iter()
        .map(|column| i64::from(column.type_().oid()))
        .collect();

    reader
        .batch_execute("ROLLBACK")
        .await
        .expect("roll back the reader snapshot before fixture cleanup");
    writer
        .batch_execute(&format!("DROP TABLE {table_name}"))
        .await
        .expect("remove the table after the reader rollback");
    writer
        .batch_execute(&format!("DROP TYPE {type_name}"))
        .await
        .expect("remove the enum after the reader rollback");

    EnumCatalogSnapshotObservation {
        suffix,
        enum_oid,
        expected_catalog_label,
        before: before_values,
        after: after_values,
        output_type_oids,
    }
}
