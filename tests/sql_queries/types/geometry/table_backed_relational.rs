//! Table-backed PostgreSQL geometry behavior tracked by #907.

use cassie::types::Value;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

const SETUP: [&str; 4] = [
    "CREATE TABLE issue_907_relational_points (\
        id INT4 PRIMARY KEY, position POINT, cohort INT4\
    )",
    "INSERT INTO issue_907_relational_points VALUES \
        (1, point '(1,1)', 10), (2, point '(3,1)', 10), \
        (3, point '(5,1)', 20), (4, NULL, 20), (5, point '(2,1)', 10)",
    "CREATE TABLE issue_907_relational_regions (\
        region_id INT4 PRIMARY KEY, footprint BOX, cohort INT4\
    )",
    "INSERT INTO issue_907_relational_regions VALUES \
        (10, '((0,0),(2,2))', 10), (20, '((2,0),(4,2))', 10), \
        (30, '((4,0),(6,2))', 20), (40, NULL, 20)",
];

struct GeometryDirectory(String);

impl Drop for GeometryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_wire_join(label: &str) -> Vec<(u8, Vec<u8>)> {
    let path = wire::data_dir(label);
    let _directory = GeometryDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let frames = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create geometry fixture");
        cassie.startup().expect("start geometry fixture");
        let session = cassie.create_session("tester", None);
        for sql in SETUP {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed table-backed geometry values");
        }
        drop(session);

        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect to Cassie pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT p.id, r.region_id, p.position, r.footprint, \
                        p.position <@ r.footprint \
                 FROM issue_907_relational_points AS p \
                 LEFT JOIN issue_907_relational_regions AS r \
                   ON r.footprint @> p.position \
                 ORDER BY p.id, r.region_id NULLS LAST",
            )],
        )
        .await;
        let frames =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        drop(reader);
        drop(writer);
        server.stop().await;
        frames
    });
    drop(runtime);
    frames
}

#[test]
fn should_join_point_rows_to_each_containing_box() {
    // Arrange
    let fixture = sql_fixture("issue-907-relational-box-containment-join", &SETUP);

    // Act
    let result = fixture.execute(
        "SELECT p.id, r.region_id, p.position <@ r.footprint \
         FROM issue_907_relational_points AS p \
         LEFT JOIN issue_907_relational_regions AS r \
           ON r.footprint @> p.position \
         ORDER BY p.id, r.region_id NULLS LAST",
    );

    // Assert
    assert_eq!(
        result.expect("box containment join over point rows").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10), Value::Bool(true)],
            vec![Value::Int64(2), Value::Int64(20), Value::Bool(true)],
            vec![Value::Int64(3), Value::Int64(30), Value::Bool(true)],
            vec![Value::Int64(4), Value::Null, Value::Null],
            vec![Value::Int64(5), Value::Int64(10), Value::Bool(true)],
            vec![Value::Int64(5), Value::Int64(20), Value::Bool(true)],
        ]
    );
}

#[test]
fn should_filter_point_rows_by_box_boundary_inclusion() {
    // Arrange
    let fixture = sql_fixture("issue-907-relational-box-point-filter", &SETUP);

    // Act
    let result = fixture.execute(
        "SELECT id FROM issue_907_relational_points \
         WHERE position <@ '((0,0),(4,2))'::box ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("box containment filter over point rows")
            .rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(5)],
        ]
    );
}

#[test]
fn should_group_rows_by_overlapping_point_boxes() {
    // Arrange
    let fixture = sql_fixture("issue-907-relational-box-overlap-groups", &SETUP);

    // Act
    let result = fixture.execute(
        "SELECT r.region_id, count(*) \
         FROM issue_907_relational_points AS p \
         JOIN issue_907_relational_regions AS r \
           ON r.footprint && box(p.position, p.position) \
         WHERE p.position IS NOT NULL \
         GROUP BY r.region_id ORDER BY r.region_id",
    );

    // Assert
    assert_eq!(
        result
            .expect("group point rows by overlapping boxes")
            .rows(),
        &[
            vec![Value::Int64(10), Value::Int64(2)],
            vec![Value::Int64(20), Value::Int64(2)],
            vec![Value::Int64(30), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_order_non_null_point_rows_by_distance_then_identifier() {
    // Arrange
    let fixture = sql_fixture("issue-907-relational-point-distance-order", &SETUP);

    // Act
    let result = fixture.execute(
        "SELECT id FROM issue_907_relational_points \
         WHERE position IS NOT NULL \
         ORDER BY position <-> point '(4,1)', id DESC",
    );

    // Assert
    assert_eq!(
        result.expect("distance ordering over point rows").rows(),
        &[
            vec![Value::Int64(3)],
            vec![Value::Int64(2)],
            vec![Value::Int64(5)],
            vec![Value::Int64(1)],
        ]
    );
}

#[test]
fn should_describe_table_backed_geometry_join_types_over_pgwire() {
    // Arrange
    let frames = run_wire_join("issue-907-relational-geometry-pgwire");

    // Act
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("geometry join RowDescription");
    let columns = wire::parse_row_description(&description.1);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    assert_eq!(
        columns
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ))
            .collect::<Vec<_>>(),
        vec![
            (23, 4, -1, 0),
            (23, 4, -1, 0),
            (600, 16, -1, 0),
            (603, 32, -1, 0),
            (16, 1, -1, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("(1,1)".to_owned()),
                Some("(2,2),(0,0)".to_owned()),
                Some("t".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("20".to_owned()),
                Some("(3,1)".to_owned()),
                Some("(4,2),(2,0)".to_owned()),
                Some("t".to_owned()),
            ],
            vec![
                Some("3".to_owned()),
                Some("30".to_owned()),
                Some("(5,1)".to_owned()),
                Some("(6,2),(4,0)".to_owned()),
                Some("t".to_owned()),
            ],
            vec![Some("4".to_owned()), None, None, None, None,],
            vec![
                Some("5".to_owned()),
                Some("10".to_owned()),
                Some("(2,1)".to_owned()),
                Some("(2,2),(0,0)".to_owned()),
                Some("t".to_owned()),
            ],
            vec![
                Some("5".to_owned()),
                Some("20".to_owned()),
                Some("(2,1)".to_owned()),
                Some("(4,2),(2,0)".to_owned()),
                Some("t".to_owned()),
            ],
        ]
    );
}
