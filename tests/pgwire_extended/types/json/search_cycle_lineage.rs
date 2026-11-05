use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_jsonb_lineage_result_descriptors_for_search_cycle_rows() {
    // Arrange
    let orders = [("BREADTH", 2249), ("DEPTH", 2287)];
    let cycles = orders
        .iter()
        .map(|(order, _)| {
            vec![wire::simple_query_frame(&format!(
                "WITH RECURSIVE walk(payload,depth) AS \
            (SELECT '1.0'::JSONB,0 UNION ALL SELECT '1.00'::JSONB,depth+1 FROM walk WHERE depth<2) \
            SEARCH {order} FIRST BY payload SET sequence CYCLE payload SET is_cycle USING path \
            SELECT payload,depth,is_cycle,sequence,path FROM walk ORDER BY depth"
            ))]
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("jsonb-search-cycle-descriptors", &[], cycles);
    // Assert
    for (frames, (_, search_oid)) in batches.iter().zip(orders) {
        fixture::assert_success(frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("row descriptor");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(
            columns
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [3802, 23, 16, search_oid, 2287]
        );
        let rows = wire::data_rows(frames);
        assert_eq!(rows.len(), 2);
        assert_eq!(&rows[0][1..3], &[Some("0".into()), Some("f".into())]);
        assert_eq!(&rows[1][1..3], &[Some("1".into()), Some("t".into())]);
    }
}
