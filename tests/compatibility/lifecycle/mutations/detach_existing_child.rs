use super::*;

struct InheritanceDetachObservations {
    parent_rows: Vec<tokio_postgres::Row>,
    child_rows: Vec<tokio_postgres::Row>,
    grandchild_rows: Vec<tokio_postgres::Row>,
    inheritance_edges: Vec<tokio_postgres::Row>,
    inherited_attributes: Vec<tokio_postgres::Row>,
}

async fn read_inheritance_detach_state(
    client: &tokio_postgres::Client,
) -> InheritanceDetachObservations {
    let parent_rows = client
        .query(
            "SELECT id, label FROM compat_detach_parent ORDER BY id",
            &[],
        )
        .await
        .expect("read only parent rows after detachment");
    let child_rows = client
        .query("SELECT id, label FROM compat_detach_child ORDER BY id", &[])
        .await
        .expect("read the detached child and its remaining descendants");
    let grandchild_rows = client
        .query(
            "SELECT id, label FROM compat_detach_grandchild ORDER BY id",
            &[],
        )
        .await
        .expect("read the grandchild relation after parent detachment");
    let inheritance_edges = client
        .query(
            "SELECT parent.relname, child.relname, inheritance.inhseqno \
             FROM pg_catalog.pg_inherits AS inheritance \
             JOIN pg_catalog.pg_class AS parent \
               ON parent.oid = inheritance.inhparent \
             JOIN pg_catalog.pg_class AS child \
               ON child.oid = inheritance.inhrelid \
             WHERE parent.relname IN \
               ('compat_detach_parent', 'compat_detach_child') \
             ORDER BY parent.relname, child.relname",
            &[],
        )
        .await
        .expect("inspect the remaining direct inheritance edge");
    let inherited_attributes = client
        .query(
            "SELECT relation.relname, attribute.attname, \
                    attribute.attislocal, attribute.attinhcount \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation \
               ON relation.oid = attribute.attrelid \
             WHERE relation.relname IN \
               ('compat_detach_parent', 'compat_detach_child', \
                'compat_detach_grandchild') \
               AND attribute.attname IN ('id', 'label') \
               AND attribute.attnum > 0 \
             ORDER BY relation.relname, attribute.attnum",
            &[],
        )
        .await
        .expect("inspect inherited attribute state after detachment");
    InheritanceDetachObservations {
        parent_rows,
        child_rows,
        grandchild_rows,
        inheritance_edges,
        inherited_attributes,
    }
}

fn relation_rows(rows: &[tokio_postgres::Row]) -> Vec<(i32, String)> {
    rows.iter()
        .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
        .collect()
}

fn assert_inheritance_detach_state(observations: &InheritanceDetachObservations) {
    assert_eq!(
        relation_rows(&observations.parent_rows),
        vec![(1, "parent".to_owned())]
    );
    assert_eq!(
        relation_rows(&observations.child_rows),
        vec![(2, "child".to_owned()), (3, "grandchild".to_owned())]
    );
    assert_eq!(
        relation_rows(&observations.grandchild_rows),
        vec![(3, "grandchild".to_owned())]
    );
    assert_eq!(
        observations
            .inheritance_edges
            .iter()
            .map(|row| (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, i32>(2),
            ))
            .collect::<Vec<_>>(),
        vec![(
            "compat_detach_child".to_owned(),
            "compat_detach_grandchild".to_owned(),
            1,
        )]
    );
    assert_eq!(
        observations
            .inherited_attributes
            .iter()
            .map(|row| (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, bool>(2),
                row.get::<_, i16>(3),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("compat_detach_child".to_owned(), "id".to_owned(), true, 0,),
            (
                "compat_detach_child".to_owned(),
                "label".to_owned(),
                true,
                0,
            ),
            (
                "compat_detach_grandchild".to_owned(),
                "id".to_owned(),
                false,
                1,
            ),
            (
                "compat_detach_grandchild".to_owned(),
                "label".to_owned(),
                false,
                1,
            ),
            ("compat_detach_parent".to_owned(), "id".to_owned(), true, 0,),
            (
                "compat_detach_parent".to_owned(),
                "label".to_owned(),
                true,
                0,
            ),
        ]
    );
}

#[test]
fn should_preserve_grandchild_scan_after_middle_link_detachment() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inheritance_detach_existing_child").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_detach_parent (id INT, label TEXT)",
                "CREATE TABLE compat_detach_child (child_value TEXT) \
                   INHERITS (compat_detach_parent)",
                "CREATE TABLE compat_detach_grandchild (leaf INT) \
                   INHERITS (compat_detach_child)",
                "INSERT INTO compat_detach_parent VALUES (1, 'parent')",
                "INSERT INTO compat_detach_child VALUES (2, 'child', 'child-only')",
                "INSERT INTO compat_detach_grandchild \
                   VALUES (3, 'grandchild', 'grandchild-only', 30)",
            ],
        )
        .await
        .expect("create and seed a three-level inheritance tree");

        // Act
        client
            .batch_execute("ALTER TABLE compat_detach_child NO INHERIT compat_detach_parent")
            .await
            .expect("detach the existing child from its parent");
        let observations = read_inheritance_detach_state(&client).await;

        // Assert
        assert_inheritance_detach_state(&observations);

        drop(client);
        server.shutdown(connection).await;
    });
}
