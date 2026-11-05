//! Declared JSONB identities in existing SEARCH/CYCLE lineage (#809).
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_order_breadth_first_jsonb_search_keys_by_numeric_value() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-search_jsonb_breadth", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(id,payload) AS (VALUES (1,'2'::JSONB),(2,'10'::JSONB) UNION ALL SELECT id,payload FROM walk WHERE FALSE) SEARCH BREADTH FIRST BY payload SET sequence SELECT id FROM walk ORDER BY sequence");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_order_depth_first_jsonb_search_keys_by_numeric_value() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-search_jsonb_depth", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(id,payload) AS (VALUES (1,'2'::JSONB),(2,'10'::JSONB) UNION ALL SELECT id,payload FROM walk WHERE FALSE) SEARCH DEPTH FIRST BY payload SET sequence SELECT id FROM walk ORDER BY sequence");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_preserve_null_origins_in_jsonb_search_order() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-search_null_origins", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(id,payload) AS (VALUES (1,NULL::JSONB),(2,'null'::JSONB),(3,'[]'::JSONB),(4,'2'::JSONB),(5,'10'::JSONB) UNION ALL SELECT id,payload FROM walk WHERE FALSE) SEARCH BREADTH FIRST BY payload SET sequence SELECT id FROM walk ORDER BY sequence");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(3)],
            vec![Value::Int64(2)],
            vec![Value::Int64(4)],
            vec![Value::Int64(5)],
            vec![Value::Int64(1)]
        ]
    );
}

#[test]
fn should_detect_jsonb_numeric_equivalence_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-cycle_jsonb", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(payload,depth) AS (SELECT '1.0'::JSONB,0 UNION ALL SELECT '1.00'::JSONB,depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(true)]
        ]
    );
}

#[test]
fn should_preserve_text_spelling_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-cycle_text", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(payload,depth) AS (SELECT '1.0'::TEXT,0 UNION ALL SELECT '1.00'::TEXT,depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(2), Value::Bool(true)]
        ]
    );
}

#[test]
fn should_distinguish_sql_null_from_document_null_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-cycle_null_origins", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(payload,depth) AS (SELECT NULL::JSONB,0 UNION ALL SELECT 'null'::JSONB,depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(2), Value::Bool(true)]
        ]
    );
}

#[test]
fn should_detect_jsonb_array_equivalence_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-cycle_jsonb_array", &[]);
    // Act
    let result = fixture.execute(r#"WITH RECURSIVE walk(payload,depth) AS (SELECT '[-1:0]={1.0,"null"}'::JSONB[],0 UNION ALL SELECT '[-1:0]={1.00,"null"}'::JSONB[],depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth"#);
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(true)]
        ]
    );
}

#[test]
fn should_preserve_sql_array_bounds_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-cycle_array_bounds", &[]);
    // Act
    let result = fixture.execute(r#"WITH RECURSIVE walk(payload,depth) AS (SELECT '[-1:0]={1.0,"null"}'::JSONB[],0 UNION ALL SELECT '[0:1]={1.00,"null"}'::JSONB[],depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth"#);
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(2), Value::Bool(true)]
        ]
    );
}

#[test]
fn should_apply_remaining_search_keys_after_jsonb_numeric_equality() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-search_multikey", &[]);
    // Act
    let result = fixture.execute(r"WITH RECURSIVE walk(id,payload) AS (VALUES (2,'2.0'::JSONB),(1,'2'::JSONB),(3,'10'::JSONB) UNION ALL SELECT id,payload FROM walk WHERE FALSE) SEARCH DEPTH FIRST BY payload,id SET sequence SELECT id FROM walk ORDER BY sequence");
    // Assert
    assert_eq!(
        result.expect("pinned PostgreSQL lineage behavior").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)]
        ]
    );
}

#[test]
fn should_preserve_sql_array_null_origins_in_cycle_lineage() {
    // Arrange
    let fixture = sql_fixture("jsonb-lineage-array-null-origins", &[]);
    // Act
    let result=fixture.execute(r#"WITH RECURSIVE walk(payload,depth) AS (SELECT '[-1:1]={1.0,"null",NULL}'::JSONB[],0 UNION ALL SELECT '[-1:1]={1.00,NULL,"null"}'::JSONB[],depth+1 FROM walk WHERE depth<2) CYCLE payload SET is_cycle USING path SELECT depth,is_cycle FROM walk ORDER BY depth"#);
    // Assert
    assert_eq!(
        result.expect("array null origins").rows(),
        &[
            vec![Value::Int64(0), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(2), Value::Bool(true)]
        ]
    );
}
