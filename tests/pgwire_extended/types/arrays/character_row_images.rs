//! OLD/NEW character arrays retain native shape and logical element bytes.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> [(&'static str, Vec<cassie::types::Value>); 2] {
    [
        (
            "CREATE TABLE char_images(id INT PRIMARY KEY,items CHAR(3)[])",
            vec![],
        ),
        (
            "INSERT INTO char_images VALUES(1,'[-2:-1]={x,NULL}'::CHAR(3)[])",
            vec![],
        ),
    ]
}

#[test]
fn should_preserve_character_array_images_for_joined_update_returning() {
    // Arrange
    let sql = "UPDATE char_images t SET items='[-2:-1]={y,NULL}'::CHAR(3)[] FROM (VALUES(1)) s(id) WHERE t.id=s.id RETURNING WITH(OLD AS o,NEW AS n) o.items,n.items";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("char-array-joined-images", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[-2:-1]={\"x  \",NULL}".into()),
            Some("[-2:-1]={\"y  \",NULL}".into())
        ]]
    );
}

#[test]
fn should_publish_binary_character_array_old_images_with_lower_bounds() {
    // Arrange
    let sql = "UPDATE char_images t SET id=2 FROM (VALUES(1)) s(id) WHERE t.id=s.id RETURNING WITH(OLD AS o,NEW AS n) o.items";
    let cycles = vec![fixture::execute_cycle(sql, None, 1, true)];
    let mut binary = Vec::new();
    for value in [1_i32, 1, 1042, 2, -2, 3] {
        binary.extend_from_slice(&value.to_be_bytes());
    }
    binary.extend_from_slice(b"x  ");
    binary.extend_from_slice(&(-1_i32).to_be_bytes());

    // Act
    let batches = fixture::run_cycles("char-array-binary-old-image", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1014, -1, 7), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&binary)]
    );
}

#[test]
fn should_distinguish_absent_character_array_row_images_from_null_elements() {
    // Arrange
    let cases = [
        ("DELETE FROM char_images t USING (VALUES(1)) s(id) WHERE t.id=s.id RETURNING WITH(OLD AS o,NEW AS n) o.items,n.items", vec![Some("[-2:-1]={\"x  \",NULL}".to_owned()),None]),
        ("INSERT INTO char_images VALUES(2,'[-2:-1]={y,NULL}'::CHAR(3)[]) RETURNING WITH(OLD AS o,NEW AS n) o.items,n.items", vec![None,Some("[-2:-1]={\"y  \",NULL}".to_owned())]),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("char-array-null-row-images", &setup(), cycles);

    // Assert
    for ((_, expected), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        assert_eq!(wire::data_rows(frames), vec![expected]);
    }
}

#[test]
fn should_preserve_character_array_images_for_merge_returning() {
    // Arrange
    let sql = "MERGE INTO char_images t USING (VALUES(1,'[-2:-1]={y,NULL}'::CHAR(3)[])) s(id,items) ON t.id=s.id WHEN MATCHED THEN UPDATE SET items=s.items RETURNING WITH(OLD AS o,NEW AS n) o.items,n.items";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("char-array-merge-images", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[-2:-1]={\"x  \",NULL}".into()),
            Some("[-2:-1]={\"y  \",NULL}".into())
        ]]
    );
}

#[test]
fn should_reconstruct_character_arrays_for_conflict_old_new_images() {
    // Arrange
    let sql = "INSERT INTO char_images VALUES(1,'[-2:-1]={y,NULL}'::CHAR(3)[]) ON CONFLICT(id) DO UPDATE SET items=EXCLUDED.items RETURNING WITH(OLD AS o,NEW AS n) o.items,n.items";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("char-array-conflict-images", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[-2:-1]={\"x  \",NULL}".into()),
            Some("[-2:-1]={\"y  \",NULL}".into())
        ]]
    );
}

#[test]
fn should_prepare_character_array_images_before_exists_returning_projection() {
    // Arrange
    let sql = "UPDATE char_images t SET id=2 FROM (VALUES(1)) s(id) WHERE t.id=s.id RETURNING WITH(OLD AS o,NEW AS n) array_reverse(o.items),EXISTS(SELECT 1)";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("char-array-exists-images", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[-2:-1]={NULL,\"x  \"}".into()),
            Some("t".into())
        ]]
    );
}

#[test]
fn should_align_character_image_metadata_across_merge_actions() {
    // Arrange
    let mut setup = setup().to_vec();
    setup.push((
        "INSERT INTO char_images VALUES(2,'[-2:-1]={d,NULL}'::CHAR(3)[])",
        vec![],
    ));
    let sql = "MERGE INTO char_images t USING (VALUES(1,'UPDATE','[-2:-1]={y,NULL}'::CHAR(3)[]),(2,'DELETE',NULL::CHAR(3)[]),(3,'INSERT','[-2:-1]={z,NULL}'::CHAR(3)[])) s(id,action,items) ON t.id=s.id WHEN MATCHED AND s.action='UPDATE' THEN UPDATE SET items=s.items WHEN MATCHED AND s.action='DELETE' THEN DELETE WHEN NOT MATCHED THEN INSERT(id,items) VALUES(s.id,s.items) RETURNING WITH(OLD AS o,NEW AS n) merge_action(),o.items,n.items";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("char-array-mixed-merge-images", &setup, cycles);
    let mut images = wire::data_rows(&batches[0]);
    images.sort_by_key(|row| row[0].clone());

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        images,
        vec![
            vec![
                Some("DELETE".into()),
                Some("[-2:-1]={\"d  \",NULL}".into()),
                None
            ],
            vec![
                Some("INSERT".into()),
                None,
                Some("[-2:-1]={\"z  \",NULL}".into())
            ],
            vec![
                Some("UPDATE".into()),
                Some("[-2:-1]={\"x  \",NULL}".into()),
                Some("[-2:-1]={\"y  \",NULL}".into())
            ],
        ]
    );
}
